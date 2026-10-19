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

1. **全盘扫描**：`scan_volume` 在阻塞线程执行并行 walker；完整快照缓存在
   `AppState.full_snapshot`（最多一份），并压缩写入 `<数据目录>\scans\<id>.json.gz`
   （保留最近 5 份）。前端只收到裁剪后的摘要（Top 目录 2 层、200 个大文件）。
2. **Treemap**：`get_treemap(snapshot_id)` 从完整快照生成初始 ≤2500 节点、每层最多
   50 个子节点的树；`get_treemap_children(snapshot_id, path)` 按点击的目录懒加载
   下一层（每层最多 200 个目录），实现不限层级的深度下钻。历史扫描若没有完整
   sidecar，会回退到 3 层摘要并返回 `full=false`。
3. **快速扫描**：`quick_scan` 加载内置 + 用户规则，按规则根目录遍历，生成
   `Finding`（含 E1–E8 证据）和精确文件列表；文件列表只留在 Rust 侧。
4. **计划**：`create_plan(finding_ids)` 重新校验每个文件仍在允许根目录内、不是
   reparse point、不命中红线；frontend 只收到汇总预览。
5. **执行**：`execute_plan(plan_id, permanent_delete, on_progress)` 二次校验后调用回收站/删除；
   回收站文件按 256 个一批交给同一次 IFileOperation，前端通过 Channel 收到处理数、字节、
   失败数与当前路径。仅安全/谨慎、可重建、非管理员项可勾选永久删除（二次确认），
   避免大缓存的回收站逐文件开销。逐项写审计；回收站清空优先通过 PowerShell 的
   `Clear-RecycleBin` 执行，PowerShell 不可用或失败时回退到 `SHEmptyRecycleBinW`，
   不可恢复。
6. **系统引导**：Windows.old/还原点/分页文件规则的 action 为 `Guided`；Tauri 层打开
   `cleanmgr.exe`、`SystemPropertiesProtection.exe` 或 `SystemPropertiesAdvanced.exe`，不直接删除。
7. **管理员执行/扫描**：普通项在主进程执行；管理员项写入 `<数据目录>\jobs\<job>.json`，
   优先通过 PowerShell `Start-Process -Verb RunAs` 启动 `disksage-helper.exe`；
   PowerShell 不存在、无法创建或在 UAC 前失败时，自动回退到
   `ShellExecuteExW("runas")`。用户取消 UAC 不会重试，避免二次弹窗。
   helper 校验 nonce/过期时间/规则 ID，重新加载编译进二进制的规则并提权扫描，
   只执行固定 `ActionSpec`；服务名白名单为 `wuauserv/bits/dosvc`，
   服务控制优先 PowerShell，失败时回退到 SCM API。结果写回 `<job>.result.json`，
   主进程合并并展示。
   清理建议页的「管理员扫描」使用同一 job 机制但 task 为 `quick_scan`：helper 只读扫描
   全部内置管理员规则，将 findings、精确文件列表和 resolved roots 写入 `<job>.scan.json`，
   主进程重新挂载 targets 后展示实测大小；取消 UAC 时保留普通扫描结果。
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
13. **数据目录**：`disksage-core::paths` 在非系统盘安装时选择
   `<安装盘>:\CCDiskSageData\<用户名>`，否则使用 `%LOCALAPPDATA%\cc-disksage`。
   Tauri 启动前通过 `appDirectoriesOverride` 把 WebView2 数据也放到同一目录；首次启动
   会复制并校验旧 C 盘数据，成功后再清理旧文件。
14. **持久化**：`src-tauri/storage.rs` 使用 bundled SQLite，在 `<数据目录>\disksage.db`
   保存扫描摘要与重复文件结果。扫描完成、重复扫描完成、重复删除后写入；启动时
   `get_latest_scan` / `get_latest_duplicate_scan` 恢复最近结果，`get_scan_history`
   供历史页展示。每个表保留最近 20 条。
15. **托盘与通知**：启动时创建托盘图标；关闭窗口隐藏到托盘，托盘菜单显示主界面/退出。
   扫描、快速扫描、清理、重复文件扫描/删除、手动自动清理完成后通过
   `tauri-plugin-notification` 发送 Windows 通知。
16. **仪表盘可清理摘要**：快速扫描/管理员扫描完成后，前端把安全/谨慎项汇总写入
    WebView2 localStorage；仪表盘读取该摘要并显示「去清理建议」入口，不保存文件路径。
17. **大文件操作**：仪表盘大文件列表使用快照 ID + 文件索引调用后端：
    - 打开目录：解析快照条目后调用 Explorer `/select`，只读；
    - 移到回收站：构造一个临时的 `large_file.user_selected` 候选，复用
      `create_plan/execute_plan` 的路径、红线、reparse 校验，只进入回收站；
      Windows/Program Files/ProgramData 由额外守卫拒绝；删除后从快照移除并持久化；
    - AI 分析：把文件元数据构造成临时 Finding（targets 中的文件名用 `<file>` 掩码），
      复用现有 LLM payload/校验链路；文件内容永不发送，AI 不能删除文件。

## 安全边界

- UI 始终 `asInvoker`；管理员动作交给 `disksage-helper.exe`，仅在用户确认计划后触发 UAC。
- 规则动作是固定枚举；服务名、DISM 参数硬编码。
- 路径安全模块在扫描、计划、执行三处复用。
- 浏览器/聊天数据红线在规则 exclude 与全局校验两层保护。
- 所有数字带 `size_provenance`；LLM 不能产生数字。

## 当前命令

`get_volumes`、`scan_volume`、`scan_volume_mft`、`get_latest_scan`、`get_scan_history`、
`get_treemap`、`quick_scan`、`quick_scan_admin`、`get_finding`、`create_plan`、`execute_plan`、
`get_audit_history`、
`find_duplicate_files`、`cancel_duplicate_scan`、`get_latest_duplicate_scan`、
`delete_duplicate_files`、`open_duplicate_file`、`llm_get_config`、`llm_set_config`、
`llm_set_key`、
`llm_clear_key`、`llm_test_connection`、`llm_preview_payload`、
`llm_assess_finding`、`llm_ask_followup`、`get_auto_clean_status`、
`set_auto_clean_config`、`run_auto_clean_now`、`open_large_file`、
`delete_large_file`、`llm_preview_large_file`、`llm_assess_large_file`。

## 后续

完整目录树快照持久化、自动更新。代码签名流水线已就绪，正式发布需要提供证书。
完整方案见 `.claude/plans/disk-cleaner-implementation.md`。
