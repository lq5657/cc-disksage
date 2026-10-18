# 实现状态（2026-10-18）

对应方案：`.claude/plans/disk-cleaner-implementation.md`。

## 已完成

| 模块 | 实现 |
|---|---|
| 工程骨架 | Rust workspace、Tauri 2、React 19、Vite、Tailwind 4、NSIS 打包 |
| 扫描 | 卷枚举（sysinfo）、并行全盘 walker、实时进度、取消、Top 目录、大文件 |
| 空间可视化 | ECharts treemap（≤2500 节点）、点击下钻、Top 目录卡片 |
| 规则引擎 | TOML schema、固定 ActionSpec、路径/红线校验、用户规则限制 |
| 内置规则 | 用户临时文件、缩略图、着色器缓存、Edge/Chrome/Firefox 缓存、npm/pnpm/yarn/pip/uv/go、VS Code/Teams/Discord/Slack；Cargo/Gradle/NuGet 谨慎项 |
| 快速扫描 | 按规则根目录遍历、E1–E8 证据链、可清理摘要 |
| 清理 | 计划预览、确认、回收站/永久删除、锁定文件跳过、审计 JSONL |
| 历史 | 读取本地审计日志并展示 |
| LLM | OpenAI 兼容、Ollama `/v1`、Credential Manager、payload 预览/同意、逐项分析、追问、严格 schema/证据 ID/数字校验、脱敏 |
| 管理员清理 | `disksage-helper.exe`（scan/clean 固定任务、nonce/expiry、提权检查、规则重扫、路径/红线校验）；Windows Temp、机器 WER、Minidump、传递优化、Windows Update 缓存、DISM Analyze/StartComponentCleanup、休眠文件 |
| 系统引导/回收站 | Windows.old → cleanmgr；还原点 → 系统保护；分页文件 → 系统属性；回收站 → 系统 API 清空 |
| 安装包 | offline WebView2、lite、helper 侧车、静默安装/启动/卸载验证 |
| 文档 | README、rule-authoring、privacy、architecture、验证清单 |

## 验证基线

- `cargo test --workspace`：通过（46 项，2 个 ignored）
- `cargo clippy --workspace --all-targets -- -D warnings`：通过
- `npm run test`：3 项通过
- `npm run build`：通过
- 真机 C 盘扫描：711,465 文件 / 198,923 目录 / 131.07 GB / 63.2 秒 / 102 个权限错误
- 快速扫描真机结果（含管理员/维护项，13 项，约 31 秒热缓存）：npm 971MB、Chrome 758MB、Go 516MB、pip 393MB、Temp 326MB、缩略图 24MB、VS Code 12.5MB、Edge 6.7MB、着色器 3.2MB、Windows Update 缓存 838MB（管理员）、Cargo 379MB（谨慎）；回收站/分页文件/还原点引导项已出现（默认不勾选）
- 回收站删除 API：真机通过
- Windows Credential Manager roundtrip：真机通过
- helper 非管理员环境拒绝执行并写出结果：真机通过
- NSIS 安装/启动/卸载：真机通过；安装目录包含 `disksage-helper.exe`

## 未完成（下一阶段）

1. NTFS MFT 秒扫（`mft`/`ntfs` crate），与 walker 基准对比。
2. 重复文件查找（大小分组 + 部分/完整哈希 + 硬链接去重）。
3. 定时/低空间自动清理（Task Scheduler）与托盘通知。
4. SQLite 快照持久化（当前快照在进程内存，重启丢失）。
5. 代码签名、自动更新、per-machine 安装变体、便携版。
6. 更多第三方应用规则（需真实环境核验 + positive/redline fixture）。

## 已知限制

- 全盘扫描无管理员权限时无法读取部分系统目录（显示错误计数）；管理员扫描 helper 未接入。
- treemap 使用逻辑大小；硬链接/稀疏文件的分配大小精确统计属于后续优化。
- 产品公开名 `DiskSage` 已被其他项目使用，发布前需改名/确认。
- 安装包未签名，SmartScreen 可能提示。
- Lite 安装包要求目标机已有 WebView2；缺失时 bootstrapper 会尝试联网。
