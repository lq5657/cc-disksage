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

- 卷枚举与全盘只读扫描（并行 walker，C 盘实测 71 万文件约 60 秒；带实时进度与取消）
- 快速扫描优化：release 冷缓存约 5 秒、热缓存约 2.7 秒（此前约 30 秒）
- 清理执行：回收站按批提交（每批 256 个文件），前端实时显示处理文件数、字节、失败数与当前路径
- 永久删除选项：安全/谨慎且可重建、非管理员的缓存项可勾选「永久删除」（单独开关 + 二次确认），大幅加快大缓存清理
- 管理员扫描入口：清理建议页「管理员扫描」通过 UAC helper 实测系统项；仪表盘「管理员全盘扫描」通过 MFT 查看系统目录占用
- 仪表盘可清理摘要：显示上次快速扫描的安全/谨慎项，并提供「去清理建议」入口
- 大文件操作：AI 元数据分析、打开所在目录、确认后移到回收站（系统安装目录会被拒绝）
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
- 历史记录页（扫描历史 + 本地审计日志）
- SQLite 持久化：扫描快照、重复文件结果、扫描历史；重启后恢复最近结果（保留最近 20 次）
- 数据目录跟随安装盘：安装在 D 盘时使用 `D:\CCDiskSageData\<用户名>`，并自动迁移旧 C 盘数据
- 系统托盘：关闭窗口隐藏到托盘；托盘菜单显示主界面/退出；扫描、清理、重复文件操作发送 Windows 通知
  （最终安装包已实测窗口隐藏时的快速扫描完成通知）
- 代码签名流水线：signtool + PFX/证书指纹 + RFC3161 时间戳；Tauri `signCommand` 签名主程序、helper 与安装包
- NSIS 安装包（offline WebView2 / lite 两种配置）；卸载时移除计划任务

尚未实现（后续里程碑）：

- 自动更新

## 安全模型

- 默认删除到回收站；永久删除只允许可重建缓存并需要二次确认。
- MFT 秒扫是只读操作：主程序保持 `asInvoker`，由 UAC 提权 helper 打开裸卷并写回快照；不执行任何删除。
- 重复文件删除只处理当前用户文档/下载/图片/视频/桌面范围，跳过 AppData、开发目录与聊天/腾讯数据目录；必须至少保留一个文件，删除只进回收站。
- 自动清理是显式启用的独立模式：只接受 `risk=safe`、默认勾选、`recoverability=regenerable`、无需管理员的文件规则；默认仍进入回收站，启用永久删除时 UI 会再次确认。计划任务由当前用户注册，不弹 UAC，可随时停用。
- 管理员动作只通过 UAC 提权 helper 执行：helper 重新加载编译进二进制的规则、重新扫描目标、再次校验红线，只执行固定动作；服务名有白名单，DISM 不使用 `/ResetBase`。
- 系统引导动作（Windows.old/还原点/分页文件）只打开 Windows 系统界面，不直接删除；回收站清空需要单独确认，清空后不可恢复。
- 大文件删除只接受快照 ID + 文件索引，后端重新解析路径并校验；只进回收站，拒绝系统安装目录。
  AI 只根据元数据给出建议，不能新增候选、勾选或删除任何文件。
- 规则只能引用固定动作枚举，TOML 中不存在任意命令或参数。
- 全局红线：WinSxS、Windows Installer、WindowsApps、System Volume Information、
  pagefile/hiberfil/swapfile、注册表 hive、浏览器凭据、聊天数据库/媒体。
- 永不跟随 reparse point / junction / symlink。
- 前端只能传 finding / plan / 重复文件组与文件 ID，不能传任意删除路径。
- 每条规则必须有 positive 与 redline fixture。
- 所有展示数字来自扫描器/helper，并标注 `size_provenance`。

## 安装包

当前可下载：<https://github.com/lq5657/cc-disksage/releases/tag/v0.1.1>

> 0.2.0 源码与功能验证已完成，但安装包构建因构建机内存不足中断，尚未发布。
> 下表是当前实际可用的 0.1.1 安装包；0.2.0 构建重试后更新。

| 文件 | 大小 | 适用场景 |
|---|---|---|
| `dist-installers/cc-disksage-0.1.1-offline-setup.exe` | 约 212 MB | 断网 Win10/11；内置 WebView2 离线安装器 |
| `dist-installers/cc-disksage-0.1.1-lite-setup.exe` | 约 4.9 MB | 已装 WebView2 的 Win11/企业环境；缺失时 bootstrapper 需联网 |

安装包同时包含 `disksage-helper.exe`（提权清理侧车）。管理员项执行时会弹出 UAC。
SHA-256 见 `dist-installers/SHA256SUMS.txt`。

默认安装为当前用户（`%LOCALAPPDATA%\CC DiskSage`），不需要管理员。当前未签名，
SmartScreen 可能提示。

安装到 D 盘可使用 NSIS 的 `/D` 参数（必须是最后一个参数）：

```powershell
Start-Process .\dist-installers\cc-disksage-0.1.1-offline-setup.exe -ArgumentList '/S','/D=D:\CCDiskSage' -Wait
```

程序安装在 D 盘时，数据目录自动使用 `D:\CCDiskSageData\<用户名>`；安装在系统盘时使用
`%LOCALAPPDATA%\cc-disksage`。首次从 C 盘旧位置启动新版时，会复制数据库、日志、规则、
LLM/自动清理配置和 WebView2 数据到新目录，迁移完成后再删除旧文件；迁移失败则继续使用旧目录。
API Key 仍在 Windows 凭据管理器中，不随数据目录迁移。

### 清理执行与管理员扫描

- 清理建议页的「管理员扫描」会请求一次 UAC，由 `disksage-helper.exe` 只读扫描
  Windows Temp、WER、更新缓存、传递优化等管理员规则，并回填实测大小。
  取消 UAC 会保留普通扫描结果；管理员项在普通扫描中显示「待管理员扫描」。
- 仪表盘「管理员全盘扫描」通过 NTFS MFT 查看包含系统目录的全盘占用（只读）。
- 执行清理时进度面板实时显示已处理/总文件数、字节、失败数和当前路径。
- 回收站删除按批提交；大缓存如需更快，可在确认页勾选「永久删除可重建缓存」，
  并勾选第二项不可恢复确认。该选项只对安全/谨慎、可重建、非管理员项开放。

### 大文件与 AI 分析

仪表盘的大文件列表（超过 500MB，按占用排序）支持：

- **AI 分析**：只发送脱敏元数据（目录模板、分配/逻辑大小、修改日期、扩展名），默认不发送文件名；
  发送前展示完整 payload 并要求勾选同意。需要先在设置页配置 LLM。AI 只给建议，不能删除文件。
- **打开目录**：在资源管理器中定位并选中文件，不删除。
- **移到回收站**：需要二次确认，只进入回收站；Windows、Program Files、Program Files (x86)、
  ProgramData 中的文件会被后端拒绝。该操作不来自规则缓存，请自行确认文件用途。
- 默认只展示前 10 个大文件，点击「显示更多」展开全部。

大文件删除与规则清理是两条不同的链路：前者是用户显式选择的单个文件，后者来自 TOML 规则和证据链。
两者都复用回收站删除与审计，但大文件不会自动勾选、不会批量执行，AI 也不能替用户决定删除。

### 自动清理

设置页可启用「自动清理（仅安全规则）」：

- 通过当前用户计划任务 `CC DiskSage Auto Clean` 运行 `cc-disksage.exe --auto-clean`；
  可选每天或每周日 12:00。
- 仅当系统盘剩余空间低于设定阈值（0 = 每次运行）时执行。
- 仅选择安全、默认勾选、可重建、无需管理员权限的规则；谨慎/危险/系统项永远不会自动执行。
- 只使用编译进二进制的内置规则，不加载用户自定义规则。
- 默认删除到回收站；可选「永久删除可重建缓存」，首次启用需勾选二次确认。
  计划任务运行时不会弹窗，永久删除的授权以启用时的确认为准。
- 每次运行写入 `<数据目录>\logs\auto-clean.jsonl`，并更新设置页最近运行状态。
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

### 本地持久化与托盘

数据目录：

- 程序安装在非系统盘（如 `D:\CCDiskSage`）：`D:\CCDiskSageData\<用户名>\`
- 程序安装在系统盘：`%LOCALAPPDATA%\cc-disksage\`
- 可用环境变量 `CC_DISKSAGE_DATA_DIR` 覆盖

数据目录包含：

- `disksage.db`：SQLite WAL，保存扫描摘要（目录树前 3 层、Top 大文件）和重复文件分组
- `logs/`：审计日志 `audit.jsonl`、自动清理日志 `auto-clean.jsonl`
- `llm.json`、`auto-clean.json`：LLM 与自动清理设置
- `rules/`、`jobs/`：用户规则和 helper job
- `EBWebView/`：WebView2 缓存/localStorage

数据只在本机，不上传；保留最近 20 次扫描与 20 次重复扫描。删除数据目录可清空历史和设置
（API Key 需在设置页单独删除）。

托盘：

- 关闭主窗口不会退出，而是隐藏到系统托盘
- 左键点击托盘图标或菜单「显示主界面」恢复窗口
- 右键菜单「退出」完全退出
- 扫描、快速扫描、清理、重复文件扫描/删除、手动自动清理完成时会发送 Windows 通知
- 计划任务 `--auto-clean` 无窗口运行，不经过托盘；结果写入自动清理日志

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
- **快速扫描优化**：release 冷缓存 4.97 秒 / 热缓存 2.72 秒，19 个候选项，结果与优化前一致
- **清理进度与批量删除**：400 个 fixture 文件回收站清理约 1.5 秒，进度面板显示文件数/字节/当前路径；永久删除 400 个文件即时完成
- **管理员扫描（UAC）**：清理建议页实测传递优化 5.1 GB / 55 文件、Windows 更新缓存 799 MB / 10,956 文件、Windows Temp 505 KB / 7 文件；管理员项显示「管理员实测」
- **SQLite 持久化**：文件数据库跨重启 roundtrip 通过；实际应用启动创建 `disksage.db`，表 `scans` / `duplicate_scans` 正常
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
