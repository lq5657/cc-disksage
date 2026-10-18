# CC DiskSage — Windows 智能磁盘清理

离线优先、规则驱动、证据优先的 Windows 磁盘清理工具。Rust + Tauri 2 + React。

每个清理候选都会展示：

- 规则来源与核验信息
- 实测大小、文件数、时间范围、文件类型分布
- 删除后的影响与可恢复性
- 本地证据链（E1–E8）
- 可选的 OpenAI 兼容 LLM 标注（只引用证据，不能删除任何东西）

## 当前状态

已实现：

- 卷枚举与全盘只读扫描（并行 walker，C 盘实测 71 万文件约 63 秒；带实时进度与取消）
- NTFS MFT 秒扫：通过 UAC 提权 helper 读取 `$MFT`，分块扫描、路径重建、目录聚合；非 NTFS 自动拒绝并建议普通扫描
- 重复文件查找：按大小 → 64KB 首尾快速哈希 → 完整 BLAKE3 分组；硬链接去重；默认只扫描文档/下载/图片/视频/桌面
- 重复文件清理：服务端保存扫描结果，前端只传组/文件 ID；至少保留一个，删除只进回收站，复用计划校验与审计
- 仪表盘：容量环形图、Top 目录、大文件列表
- 空间分析：ECharts treemap，可下钻
- 规则引擎：TOML 规则、路径安全、红线校验、positive/redline 测试
- 快速扫描：用户临时文件、缩略图、着色器缓存、浏览器缓存、开发缓存、常见应用缓存
- 清理计划预览 → 确认 → 回收站删除 → 本地审计日志
- 管理员级系统清理：Windows Temp、机器级 WER、Minidump、传递优化、Windows Update 下载缓存、DISM 分析/清理、休眠文件；通过 UAC 提权 `disksage-helper.exe` 执行
- 系统引导动作：Windows.old（打开磁盘清理）、系统还原点（系统保护）、分页文件（系统属性 → 虚拟内存）
- 回收站清空：通过系统 API 执行，明确不可恢复
- 定时 / 低空间自动清理：Windows 计划任务运行无窗口 `--auto-clean`，仅处理安全、默认勾选、可重建、无需管理员的缓存；可选永久删除或回收站
- LLM 设置（OpenAI 兼容 / Ollama `/v1`）、payload 预览与同意、逐项 AI 分析、继续追问
- 历史记录页（读取本地审计日志）
- 代码签名流水线：signtool + PFX/证书指纹 + RFC3161 时间戳；Tauri `signCommand` 签名主程序、helper 与安装包
- NSIS 安装包（offline WebView2 / lite 两种配置）；卸载时移除计划任务

尚未实现（后续里程碑）：

- 自动更新
- SQLite 快照持久化

## 安全模型

- 默认删除到回收站；永久删除只允许可重建缓存并需要二次确认。
- MFT 秒扫是只读操作：主程序保持 `asInvoker`，由 UAC 提权 helper 打开裸卷并写回快照；不执行任何删除。
- 重复文件删除只处理当前用户文档/下载/图片/视频/桌面范围，跳过 AppData、开发目录与聊天/腾讯数据目录；必须至少保留一个文件，删除只进回收站。
- 自动清理是显式启用的独立模式：只接受 `risk=safe`、默认勾选、`recoverability=regenerable`、无需管理员的文件规则；默认仍进入回收站，启用永久删除时 UI 会再次确认。计划任务由当前用户注册，不弹 UAC，可随时停用。
- 管理员动作只通过 UAC 提权 helper 执行：helper 重新加载编译进二进制的规则、重新扫描目标、再次校验红线，只执行固定动作；服务名有白名单，DISM 不使用 `/ResetBase`。
- 系统引导动作（Windows.old/还原点/分页文件）只打开 Windows 系统界面，不直接删除；回收站清空需要单独确认，清空后不可恢复。
- 规则只能引用固定动作枚举，TOML 中不存在任意命令或参数。
- 全局红线：WinSxS、Windows Installer、WindowsApps、System Volume Information、
  pagefile/hiberfil/swapfile、注册表 hive、浏览器凭据、聊天数据库/媒体。
- 永不跟随 reparse point / junction / symlink。
- 前端只能传 finding / plan / 重复文件组与文件 ID，不能传任意删除路径。
- 每条规则必须有 positive 与 redline fixture。
- 所有展示数字来自扫描器/helper，并标注 `size_provenance`。

## 安装包

| 文件 | 大小 | 适用场景 |
|---|---|---|
| `dist-installers/cc-disksage-0.1.0-offline-setup.exe` | 约 211 MB | 断网 Win10/11；内置 WebView2 离线安装器 |
| `dist-installers/cc-disksage-0.1.0-lite-setup.exe` | 约 3.9 MB | 已装 WebView2 的 Win11/企业环境；缺失时 bootstrapper 需联网 |

安装包同时包含 `disksage-helper.exe`（提权清理侧车）。管理员项执行时会弹出 UAC。
SHA-256 见 `dist-installers/SHA256SUMS.txt`。

安装为当前用户（`%LOCALAPPDATA%\CC DiskSage`），不需要管理员。当前未签名，
SmartScreen 可能提示。

### 自动清理

设置页可启用「自动清理（仅安全规则）」：

- 通过当前用户计划任务 `CC DiskSage Auto Clean` 运行 `cc-disksage.exe --auto-clean`；
  可选每天或每周日 12:00。
- 仅当系统盘剩余空间低于设定阈值（0 = 每次运行）时执行。
- 仅选择安全、默认勾选、可重建、无需管理员权限的规则；谨慎/危险/系统项永远不会自动执行。
- 只使用编译进二进制的内置规则，不加载用户自定义规则。
- 默认删除到回收站；可选「永久删除可重建缓存」，首次启用需勾选二次确认。
  计划任务运行时不会弹窗，永久删除的授权以启用时的确认为准。
- 每次运行写入 `%LOCALAPPDATA%\cc-disksage\logs\auto-clean.jsonl`，并更新设置页最近运行状态。
- 计划任务只在当前用户登录时运行；卸载安装包时会自动删除该任务。

### MFT 秒扫

仪表盘上的「MFT 秒扫」按钮会：

1. 校验目标卷存在且为 NTFS；
2. 写入受限 job 文件并启动 `disksage-helper.exe`（UAC 一次授权）；
3. helper 用 `ntfs-reader` 分块读取 `$MFT`，重建路径、聚合目录大小，并写出快照；
4. 主进程读取快照，treemap/Top 目录/大文件继续复用普通扫描的展示逻辑。

MFT 扫描只读、不执行删除；跳过 reparse point、NTFS 元数据、`$Recycle.Bin`、WindowsApps、
System Volume Information 等目录，但保留 `$WinREAgent` 等真实系统占用目录。
快照根据逻辑文件大小聚合，硬链接按 MFT 文件记录只计一次。

本机 C: 实测（2026-10-18）：

| 引擎 | 文件 | 目录 | 大小 | 耗时 | 读取错误 |
|---|---:|---:|---:|---:|---:|
| MFT | 652,696 | 204,702 | 122.57 GiB | **5.76s** | 0 |
| walker | 718,862 | 200,196 | 131.52 GiB | 59.6s | 102 |

MFT 总量较低主要是因为硬链接只计一次、跳过 reparse/部分系统元数据；walker 会按路径重复计
硬链接。两者都适合空间分析，MFT 更快，walker 可在非 NTFS/无管理员时使用。

### 重复文件

「重复文件」页默认扫描当前用户的文档、下载、图片、视频、桌面：

- 只比较达到最小大小阈值的普通文件；硬链接不会误报为重复。
- 先按大小分组，再计算首尾 64KB 快速哈希，最后对候选做完整 BLAKE3 哈希。
- 扫描是只读的；扫描结果只保存在本机内存。
- 跳过 AppData、node_modules、.git、构建目录、腾讯/微信/QQ/钉钉/WPS 云文档等高风险目录。
- 删除需要显式勾选文件与二次确认，至少保留一个副本，移至回收站；服务端重新校验路径、
  文件大小、红线与 reparse point。
- 结果最多保留 300 组、每组 50 个文件；硬链接文件（多个名称指向同一份数据）会被排除，
  避免把“同一文件的两个名称”误报为可释放的重复项。

### 代码签名

发布签名需要自备受信任的 Authenticode 证书。使用 PFX：

```powershell
$env:CC_DISKSAGE_SIGN_PFX = "C:\secrets\codesign.pfx"
$env:CC_DISKSAGE_SIGN_PFX_PASSWORD = "..."
powershell -File scripts/build-installer.ps1 -Sign
```

或使用证书存储中的指纹：

```powershell
$env:CC_DISKSAGE_SIGN_THUMBPRINT = "ABCDEF..."
powershell -File scripts/build-installer.ps1 -Lite -Sign
```

签名脚本会签名并验证 helper、主程序和 NSIS 安装包，使用 SHA-256 与 RFC3161 时间戳。
详细说明、CI 密钥管理与自签名测试流程见 `docs/code-signing.md`。

### 已验证

- 静默安装 → 启动（窗口正常）→ 静默卸载
- Windows Credential Manager API Key 写入/读取/删除
- 回收站删除 API（临时文件）
- C 盘真实只读扫描与快速扫描（含管理员规则）
- helper 非管理员环境拒绝执行并写出结果；服务/目录/DISM 动作单元测试通过
- 快速扫描出现 Windows.old / 还原点 / 分页文件 / 回收站维护项；引导程序路径已核验
- 计划任务非管理员创建/查询/删除（`schtasks.exe`，含空格路径与 `--auto-clean` 参数保留）
- 卸载钩子：安装后创建同名计划任务，静默卸载后任务被自动移除
- 无窗口 `--auto-clean`：未启用时退出码 1 并记录日志；高于阈值时跳过清理
- **真实 MFT 提权扫描（UAC）通过**：652,696 文件 / 204,702 目录 / 122.57 GiB / 5.76 秒 / 0 读取错误
- **walker 对比**：718,862 文件 / 200,196 目录 / 131.52 GiB / 59.6 秒 / 102 权限错误
- MFT 目录聚合、`$WinREAgent` 保留、NTFS 元数据/reparse 跳过、节点上限单测通过；非管理员 helper 拒绝执行
- 重复文件：真实用户内容目录只读扫描通过；硬链接、大小/阈值、跳过 AppData/腾讯数据、取消等测试通过
- **重复文件真实删除 fixture 通过**：至少保留一个副本，另一个移至回收站，审计写入
- **管理员 helper 清理（UAC）通过**：Windows Temp + 机器级 WER + Minidump，删除 233 个文件 / 约 160 MiB；3 个占用文件按失败返回；审计 elevation=true
- **DISM Analyze（UAC，只读）通过**：组件存储实际 23.44GB、备份/禁用功能 15.30GB、11 个可回收包；返回 3010 并提示需要重启，不自动重启
- 修复 DISM 在隐藏重启提示处卡死的问题：`/NoRestart` + 15 分钟超时终止
- 代码签名：用临时自签名证书完成 signtool 签名/验签；Tauri `signCommand` 成功签名 helper、主程序和 lite 安装包；**按当前决定暂不用于发布**
- 最终未签名包：安装后的主程序/helper 均为 NotSigned；测试证书与临时 PFX 已清理
- Rust workspace 全部测试 + 前端测试通过
- 待验证：offline 包在**干净 Windows 10（无 WebView2、断网）**上的安装（需要 VM）
- 待人工验证：自动清理实际到点运行（会清理真实缓存，建议在 VM 或确认后操作）
- 待人工验证：系统引导项点击流程与重复文件删除 UI 点击流程
- 待提供证书：正式发布签名（当前决定暂缓）

## 开发

前置（仅开发/构建机需要）：

- Windows 10/11 x64 + WebView2（Win11 自带）
- Rust stable MSVC（`rustup default stable-x86_64-pc-windows-msvc`）
- Visual Studio 2022 C++ Build Tools + Windows SDK
- Node.js 22+ / npm

```powershell
npm install
npm run tauri dev       # 开发
npm run build           # 前端类型检查 + 构建
npm run test            # Vitest
npm run sign:installers # 签名 dist-installers 下的安装包（需证书环境变量）
cargo test --workspace  # Rust 测试
cargo clippy -- -D warnings
```

安装包：

```powershell
npm run tauri build                                        # offline WebView2（主发布包）
npm run tauri build -- --config src-tauri/tauri.lite.conf.json  # lite（要求系统已有 WebView2）
```

产物位于 `src-tauri/target/release/bundle/nsis/`。

## LLM 配置

设置页选择 provider 预设或填写任意 OpenAI 兼容 Base URL / model，保存 API Key。
Key 存储在 Windows 凭据管理器（service `cc-disksage.llm`），不进入配置文件或日志。

默认只发送脱敏聚合元数据；首次分析前展示完整 payload 并要求确认。LLM 输出必须：

- 通过 JSON schema 校验
- 只引用给定 evidence ID
- 不含任何数字（数字统一由扫描器注入）
- 不含命令、路径或删除动作

失败会自动回退到本地规则说明。

## 项目结构

```
crates/disksage-core   领域类型、路径安全、证据模型
crates/disksage-scan   卷枚举、全盘扫描、treemap 快照
crates/disksage-rules  TOML 规则、匹配、快速扫描
crates/disksage-clean  计划、回收站执行、审计
crates/disksage-llm    OpenAI 兼容客户端、脱敏、校验
src-tauri              Tauri 命令与状态
src                    React UI
rules/builtin          内置规则
```

## 文档

- `docs/rule-authoring.md`
- `docs/privacy.md`
- `docs/architecture.md`
- `docs/code-signing.md`
- `.claude/plans/disk-cleaner-implementation.md`（完整方案）

## 说明

产品公开名称 `DiskSage` 已被其他开源项目使用；对外发布前需要确认改名或商标。
当前安装包未签名，Windows SmartScreen 可能提示；代码签名在后续里程碑接入。
