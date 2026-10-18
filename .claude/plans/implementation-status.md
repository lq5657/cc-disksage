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
| 快速扫描性能 | 单次遍历复用 metadata、缓存红线路径、按目标索引统计、rayon 并行规则；release 冷 4.97s / 热 2.72s |
| SQLite 持久化 | bundled SQLite；保存扫描摘要/重复结果/扫描历史；启动恢复最近结果；保留最近 20 次；`src-tauri/storage.rs` |
| 代码签名 | `sign-binary.ps1`、`sign-installers.ps1`、`tauri.signed.conf.json`、`sign-command.ps1`、PFX/指纹/时间戳、Tauri signCommand 签名 helper/主程序/安装包 |
| 安装包 | offline WebView2、lite、helper 侧车、静默安装/启动/卸载验证 |
| 文档 | README、rule-authoring、privacy、architecture、验证清单 |

## 验证基线

- `cargo test --workspace`：通过（63 项，6 个 ignored）
- `cargo clippy --workspace --all-targets -- -D warnings`：通过
- `npm run test`：3 项通过
- `npm run build`：通过
- 真机 C 盘扫描：711,465 文件 / 198,923 目录 / 131.07 GB / 63.2 秒 / 102 个权限错误
- 快速扫描真机结果（含管理员/维护项，13 项，约 31 秒热缓存）：npm 971MB、Chrome 758MB、Go 516MB、pip 393MB、Temp 326MB、缩略图 24MB、VS Code 12.5MB、Edge 6.7MB、着色器 3.2MB、Windows Update 缓存 838MB（管理员）、Cargo 379MB（谨慎）；回收站/分页文件/还原点引导项已出现（默认不勾选）
- 自动清理只读候选核验（`reports_auto_candidate_rules_read_only`）：仅 9 条安全规则，npm 971MB、Chrome 757MB、Go 516MB、pip 393MB、Temp 326MB、缩略图 25MB、VS Code 13MB、Edge 7MB、着色器 3.35MB；管理员/谨慎/引导/回收站项均被排除
- MFT 扫描核心单测：设备路径解析、目录聚合、大小写合并、跳过 reparse/WindowsApps/System Volume Information/NTFS 元数据、保留 `$WinREAgent`、节点上限
- MFT helper 非管理员环境拒绝执行，并写出失败结果
- **真实 MFT 提权扫描（UAC）**：652,696 文件 / 204,702 目录 / 122.57 GiB / 5.76 秒 / 0 读取错误
- **walker 对比**：718,862 文件 / 200,196 目录 / 131.52 GiB / 59.6 秒 / 102 权限错误；差异主要来自硬链接去重、跳过 reparse/部分系统元数据，以及 walker 对硬链接路径重复计数
- 重复文件真实只读扫描：首版用户目录 383,119 文件 / 4,563 候选 / 1,681 次哈希 / 20 组 / 约 2.35GB；收紧范围后文档/下载/图片/视频/桌面 714 文件、0 组（该机器当前内容目录无大于 1MB 的重复文件）
- 重复文件单测：真实重复/唯一内容、最小阈值、取消、跳过 AppData/Tencent/系统目录
- **重复文件真实删除 fixture**：临时目录中保留一个副本、删除另一个到回收站；验证保留文件存在、目标消失、审计写入
- **管理员 helper 清理（UAC）**：Windows Temp + 机器级 WER + Minidump，删除 233 个文件 / 约 160 MiB；3 个被占用文件按失败返回，审计 elevation=true
- **DISM Analyze（UAC，只读）**：退出码 3010（需要重启），组件存储实际 23.44GB、备份/禁用功能 15.30GB、11 个可回收包、建议清理；已修复 DISM 在隐藏重启提示处卡死的问题（`/NoRestart` + 15 分钟超时）
- 快速扫描优化：release 全规则（含管理员）冷缓存 4.97s、热缓存 2.72s，19 个候选项，大小/文件数与优化前一致
- SQLite：内存与文件数据库 roundtrip、重启读取、扫描历史排序通过；实际应用启动创建 `disksage.db`（`scans`、`duplicate_scans`，user_version=1）
- 代码签名：临时自签名证书 + 临时受信任根完成签名/验签；Tauri `signCommand` 构建 lite 包时成功签名 helper、主程序与安装包，`Get-AuthenticodeSignature` 为 Valid（测试证书已清理）；**按用户决定暂不用于发布**
- 回收站删除 API：真机通过
- Windows Credential Manager roundtrip：真机通过
- helper 非管理员环境拒绝执行并写出结果：真机通过
- 计划任务非管理员创建/查询/删除：真机通过（Rust `schtasks.exe` 参数，含空格路径 XML 校验；ignored 集成测试）
- 无窗口 `--auto-clean`：未启用退出码 1；系统盘剩余空间高于测试阈值时正确跳过，未触发删除
- NSIS 安装/启动/卸载：真机通过；安装目录包含 `disksage-helper.exe`；卸载后 `CC DiskSage Auto Clean` 计划任务被移除
- 最终未签名包复核：安装后的 `cc-disksage.exe` / `disksage-helper.exe` Authenticode 状态为 NotSigned；自签名测试证书与临时 PFX 已清理

## 未完成（下一阶段）

1. 自动清理的真实到点运行与托盘通知（需在 VM 或用户确认后验证，可能删除真实缓存；目前只验证了未启用/阈值跳过）。
2. 重启后恢复最近扫描/重复结果与历史页扫描列表的 UI 点击流程（存储层已验证）。
3. 重复文件删除的 Tauri UI 点击流程（核心 fixture 删除已验证，UI 尚未手工点通）。
3. 系统引导项点击流程（Windows.old/还原点/分页文件/回收站）与干净 Win10 断网安装。
4. 自动更新、per-machine 安装变体、便携版。
5. 正式发布代码签名（用户暂缓；流水线已完成，需要受信任证书）。
6. 更多第三方应用规则（需真实环境核验 + positive/redline fixture）。
7. 扫描快照只持久化前 3 层摘要；如果后续需要深层下钻，再改存压缩的完整树。

## 已知限制

- 全盘扫描无管理员权限时无法读取部分系统目录（显示错误计数）；MFT 扫描通过 UAC helper 读取。
- MFT 扫描只支持 NTFS；跳过 reparse point 与 NTFS 元数据，大小按逻辑文件大小聚合。
- MFT 对硬链接只计一次，因此总量可能低于 walker；walker 会把同一份硬链接数据按路径重复计数。两种引擎对 `$WinREAgent`/系统目录的取舍也略有不同。
- 管理员 Temp/WER/Minidump 清理实测 3 个占用文件返回失败；文件动作使用回收站，`freed_bytes` 更接近“已处理字节”，不一定立即反映磁盘可用空间。
- DISM 操作可能返回 3010（需要重启），不会自动重启；超时 15 分钟会终止并提示检查 `dism.log`。
- treemap 使用逻辑大小；硬链接/稀疏文件的分配大小精确统计属于后续优化。
- SQLite 只保存扫描摘要（前 3 层目录），不是完整目录树；当前 treemap 下钻深度与之一致。
- SQLite 每个表保留最近 20 条；重复文件结果含本地路径，删除 `disksage.db` 可清空。
- 重复文件默认只扫描文档/下载/图片/视频/桌面；不扫描 AppData 与应用内部目录，可能漏掉部分重复项。
- 重复扫描会读取候选文件内容做哈希；结果仅保存在内存，重启后需重新扫描。
- 自动清理只在计划任务触发时检查低空间阈值，不是常驻监控；任务仅在用户登录时运行。
- 自动清理默认进入回收站；开启永久删除后才会立即释放空间。计划任务被外部移除时，需要在设置页重新保存。
- 自动清理实际到点运行尚未验证；目前只验证了未启用/阈值跳过和底层 helper 清理。
- 代码签名按用户决定暂缓；流水线保留，当前发布包未签名。
- 产品公开名 `DiskSage` 已被其他项目使用，发布前需改名/确认。
- 安装包未签名，SmartScreen 可能提示。
- Lite 安装包要求目标机已有 WebView2；缺失时 bootstrapper 会尝试联网。
