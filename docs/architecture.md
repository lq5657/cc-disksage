# 架构

```
React + TypeScript + Vite + ECharts
        │ Tauri IPC（命令只收 ID，不收删除路径）
        ▼
src-tauri（Tauri 命令 / AppState）
        │
        ├─ disksage-scan    卷枚举、全盘 walker、NTFS MFT、重复文件、快照/treemap
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
5. **执行**：`execute_plan(plan_id)` 二次校验后调用回收站/删除；逐项写审计。回收站清空通过
   PowerShell 的 `Clear-RecycleBin` 系统 API 执行，不可恢复。
6. **系统引导**：Windows.old/还原点/分页文件规则的 action 为 `Guided`；Tauri 层打开
   `cleanmgr.exe`、`SystemPropertiesProtection.exe` 或 `SystemPropertiesAdvanced.exe`，不直接删除。
7. **管理员执行**：普通项在主进程执行；管理员项写入 `%LOCALAPPDATA%\cc-disksage\jobs\<job>.json`，
   通过 `Start-Process -Verb RunAs` 启动 `disksage-helper.exe`。helper 校验 nonce/过期时间/规则 ID，
   重新加载编译进二进制的规则并提权扫描，只执行固定 `ActionSpec`；服务名白名单为
   `wuauserv/bits/dosvc`。结果写回 `<job>.result.json`，主进程合并并展示。
8. **LLM**：`llm_preview_payload` 返回脱敏 payload；用户确认后
   `llm_assess_finding` 调用 provider，校验 evidence ID、数字与 schema；失败回退。
9. **自动清理**：设置页通过 `schtasks.exe` 注册当前用户任务
   `CC DiskSage Auto Clean`，运行 `<exe> --auto-clean`。进程在创建 Tauri 窗口前进入
   `auto_clean` 模块：检查系统盘剩余空间阈值 → 加载内置规则 → 只读快速扫描 → 只选择
   `safe + default_selected + regenerable + 非管理员` 的文件项 → 走同一套
   `create_plan/execute_plan` 校验与审计。默认回收站，可选永久删除；日志写入
   `auto-clean.jsonl`。卸载钩子删除计划任务。
10. **MFT 秒扫**：`scan_volume_mft` 校验 NTFS 后写 `mft_scan` job，通过 UAC 启动 helper。
   helper 用 `ntfs-reader` 分块读取 `$MFT`，重建目录树并写 `<job>.snapshot.json`；
   主进程读取快照、裁剪后存入 AppState，treemap 与普通扫描共用。
11. **重复文件**：`find_duplicate_files` 将前端 scope 映射为文档/下载/图片/视频/桌面，
   按大小 → 首尾 64KB BLAKE3 → 完整 BLAKE3 分组；硬链接按 Windows file ID 去重。
   结果保存在 AppState。`delete_duplicate_files` 只接受组/文件 ID，重新校验路径、
   大小与红线后，走 `create_plan/execute_plan` 移至回收站。
12. **代码签名**：`build-installer.ps1 -Sign` 合并 `tauri.signed.conf.json`，Tauri 的
   `signCommand` 调用 `src-tauri/sign-command.ps1` → `scripts/sign-binary.ps1`，
   用 signtool 签名并验证 helper、主程序与 NSIS 安装包。

## 安全边界

- UI 始终 `asInvoker`；管理员动作交给 `disksage-helper.exe`，仅在用户确认计划后触发 UAC。
- 规则动作是固定枚举；服务名、DISM 参数硬编码。
- 路径安全模块在扫描、计划、执行三处复用。
- 浏览器/聊天数据红线在规则 exclude 与全局校验两层保护。
- 所有数字带 `size_provenance`；LLM 不能产生数字。

## 当前命令

`get_volumes`、`scan_volume`、`scan_volume_mft`、`get_treemap`、`quick_scan`、
`get_finding`、`create_plan`、`execute_plan`、`get_audit_history`、
`find_duplicate_files`、`cancel_duplicate_scan`、`delete_duplicate_files`、
`open_duplicate_file`、`llm_get_config`、`llm_set_config`、`llm_set_key`、
`llm_clear_key`、`llm_test_connection`、`llm_preview_payload`、
`llm_assess_finding`、`llm_ask_followup`、`get_auto_clean_status`、
`set_auto_clean_config`、`run_auto_clean_now`。

## 后续

SQLite 快照持久化、自动更新、托盘通知。代码签名流水线已就绪，正式发布需要提供证书。
完整方案见 `.claude/plans/disk-cleaner-implementation.md`。
