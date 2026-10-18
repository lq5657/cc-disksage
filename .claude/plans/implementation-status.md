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
| 自动清理 | `--auto-clean` 无窗口模式、安全默认可重建非管理员规则过滤、低空间阈值、Task Scheduler 注册/删除、设置页、独立 JSONL 日志、卸载钩子删除任务 |
| MFT 秒扫 | `ntfs-reader` 分块读取 `$MFT`、路径/目录聚合、跳过 reparse/系统元数据、UAC helper job、快照回读、仪表盘入口 |
| 重复文件 | 大小 → 首尾 64KB BLAKE3 → 完整 BLAKE3、多硬链接文件排除、用户内容目录 scope、结果仅内存、回收站删除（至少保留一个）、ID-only IPC |
| 代码签名 | `sign-binary.ps1`、`sign-installers.ps1`、`tauri.signed.conf.json`、`sign-command.ps1`、PFX/指纹/时间戳、Tauri signCommand 签名 helper/主程序/安装包 |
| 安装包 | offline WebView2、lite、helper 侧车、静默安装/启动/卸载验证 |
| 文档 | README、rule-authoring、privacy、architecture、验证清单 |

## 验证基线

- `cargo test --workspace`：通过（57 项，5 个 ignored）
- `cargo clippy --workspace --all-targets -- -D warnings`：通过
- `npm run test`：3 项通过
- `npm run build`：通过
- 真机 C 盘扫描：711,465 文件 / 198,923 目录 / 131.07 GB / 63.2 秒 / 102 个权限错误
- 快速扫描真机结果（含管理员/维护项，13 项，约 31 秒热缓存）：npm 971MB、Chrome 758MB、Go 516MB、pip 393MB、Temp 326MB、缩略图 24MB、VS Code 12.5MB、Edge 6.7MB、着色器 3.2MB、Windows Update 缓存 838MB（管理员）、Cargo 379MB（谨慎）；回收站/分页文件/还原点引导项已出现（默认不勾选）
- 自动清理只读候选核验（`reports_auto_candidate_rules_read_only`）：仅 9 条安全规则，npm 971MB、Chrome 757MB、Go 516MB、pip 393MB、Temp 326MB、缩略图 25MB、VS Code 13MB、Edge 7MB、着色器 3.35MB；管理员/谨慎/引导/回收站项均被排除
- MFT 扫描核心单测：设备路径解析、目录聚合、大小写合并、跳过 reparse/WindowsApps/System Volume Information、节点上限
- MFT helper 非管理员环境拒绝执行，并写出失败结果；真实提权 MFT 扫描待 UAC 人工验证
- 重复文件真实只读扫描：首版用户目录 383,119 文件 / 4,563 候选 / 1,681 次哈希 / 20 组 / 约 2.35GB；收紧范围后文档/下载/图片/视频/桌面 714 文件、0 组（该机器当前内容目录无大于 1MB 的重复文件）
- 重复文件单测：真实重复/唯一内容、最小阈值、取消、跳过 AppData/Tencent/系统目录
- 代码签名：临时自签名证书 + 临时受信任根完成签名/验签；Tauri `signCommand` 构建 lite 包时成功签名 helper、主程序与安装包，`Get-AuthenticodeSignature` 为 Valid（测试证书已清理）
- 回收站删除 API：真机通过
- Windows Credential Manager roundtrip：真机通过
- helper 非管理员环境拒绝执行并写出结果：真机通过
- 计划任务非管理员创建/查询/删除：真机通过（Rust `schtasks.exe` 参数，含空格路径 XML 校验；ignored 集成测试）
- 无窗口 `--auto-clean`：未启用退出码 1；系统盘剩余空间高于测试阈值时正确跳过，未触发删除
- NSIS 安装/启动/卸载：真机通过；安装目录包含 `disksage-helper.exe`；卸载后 `CC DiskSage Auto Clean` 计划任务被移除
- 最终未签名包复核：安装后的 `cc-disksage.exe` / `disksage-helper.exe` Authenticode 状态为 NotSigned；自签名测试证书与临时 PFX 已清理

## 未完成（下一阶段）

1. 真实 MFT 提权扫描的基准对比（需要 UAC；与 walker 文件数/大小/耗时对比）。
2. 自动清理的真实到点运行与托盘通知（需在 VM 或用户确认后验证，可能删除真实缓存）。
3. SQLite 快照持久化（当前快照与重复结果在进程内存，重启丢失）。
4. 自动更新、per-machine 安装变体、便携版。
5. 正式发布代码签名（需要受信任证书；流水线已完成）。
6. 更多第三方应用规则（需真实环境核验 + positive/redline fixture）。

## 已知限制

- 全盘扫描无管理员权限时无法读取部分系统目录（显示错误计数）；MFT 扫描通过 UAC helper 读取。
- MFT 扫描只支持 NTFS；跳过 reparse point 与 NTFS 元数据，大小按逻辑文件大小聚合。
- treemap 使用逻辑大小；硬链接/稀疏文件的分配大小精确统计属于后续优化。
- 重复文件默认只扫描文档/下载/图片/视频/桌面；不扫描 AppData 与应用内部目录，可能漏掉部分重复项。
- 重复扫描会读取候选文件内容做哈希；结果仅保存在内存，重启后需重新扫描。
- 自动清理只在计划任务触发时检查低空间阈值，不是常驻监控；任务仅在用户登录时运行。
- 自动清理默认进入回收站；开启永久删除后才会立即释放空间。计划任务被外部移除时，需要在设置页重新保存。
- 代码签名流水线需要外部证书；当前 dist 包不再带测试签名（最终构建会重建未签名包）。
- 产品公开名 `DiskSage` 已被其他项目使用，发布前需改名/确认。
- 安装包未签名，SmartScreen 可能提示。
- Lite 安装包要求目标机已有 WebView2；缺失时 bootstrapper 会尝试联网。
