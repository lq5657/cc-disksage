# 架构

```
React + TypeScript + Vite + ECharts
        │ Tauri IPC（命令只收 ID，不收删除路径）
        ▼
src-tauri（Tauri 命令 / AppState）
        │
        ├─ disksage-scan    卷枚举、全盘 walker、快照/treemap
        ├─ disksage-rules   TOML 规则、路径安全、快速扫描、证据链
        ├─ disksage-clean   计划、回收站执行、审计
        └─ disksage-llm     OpenAI 兼容客户端、脱敏、严格校验
```

## 数据流

1. **全盘扫描**：`scan_volume` 在阻塞线程执行并行 walker；完整快照留在 Rust
   进程的 `AppState.snapshots`，前端只收到裁剪后的摘要（Top 目录 2 层、200 个大文件）。
2. **Treemap**：`get_treemap(snapshot_id)` 从完整快照按预算生成 ≤2500 节点的树，
   每层最多 50 个子节点；超出标记 `truncated`。
3. **快速扫描**：`quick_scan` 加载内置 + 用户规则，按规则根目录遍历，生成
   `Finding`（含 E1–E8 证据）和精确文件列表；文件列表只留在 Rust 侧。
4. **计划**：`create_plan(finding_ids)` 重新校验每个文件仍在允许根目录内、不是
   reparse point、不命中红线；frontend 只收到汇总预览。
5. **执行**：`execute_plan(plan_id)` 二次校验后调用回收站/删除；逐项写审计。
6. **管理员执行**：普通项在主进程执行；管理员项写入 `%LOCALAPPDATA%\cc-disksage\jobs\<job>.json`，
   通过 `Start-Process -Verb RunAs` 启动 `disksage-helper.exe`。helper 校验 nonce/过期时间/规则 ID，
   重新加载编译进二进制的规则并提权扫描，只执行固定 `ActionSpec`；服务名白名单为
   `wuauserv/bits/dosvc`。结果写回 `<job>.result.json`，主进程合并并展示。
7. **LLM**：`llm_preview_payload` 返回脱敏 payload；用户确认后
   `llm_assess_finding` 调用 provider，校验 evidence ID、数字与 schema；失败回退。

## 安全边界

- UI 始终 `asInvoker`；管理员动作交给 `disksage-helper.exe`，仅在用户确认计划后触发 UAC。
- 规则动作是固定枚举；服务名、DISM 参数硬编码。
- 路径安全模块在扫描、计划、执行三处复用。
- 浏览器/聊天数据红线在规则 exclude 与全局校验两层保护。
- 所有数字带 `size_provenance`；LLM 不能产生数字。

## 当前命令

`get_volumes`、`scan_volume`、`get_treemap`、`quick_scan`、`get_finding`、
`create_plan`、`execute_plan`、`get_audit_history`、`llm_get_config`、
`llm_set_config`、`llm_set_key`、`llm_clear_key`、`llm_test_connection`、
`llm_preview_payload`、`llm_assess_finding`、`llm_ask_followup`。

## 后续

MFT 快扫、重复文件、定时清理、SQLite 快照持久化、Windows.old/还原点引导、
代码签名与自动更新。完整方案见 `.claude/plans/disk-cleaner-implementation.md`。
