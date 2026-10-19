# Win10 提权失败与深度清理方案（分析稿）

- 日期：2026-10-19
- 基线：v0.2.0（`a80f75a docs: publish v0.2.0`）
- 状态：P0（提权/服务/回收站回退）已实现，待禁用 PowerShell 的 Win10 实测；深度清理部分未实现
- 对应问题：
  1. Windows 10 点击「管理员全盘扫描」提示「无法启动提权 helper：拒绝访问。(os error 5)」
  2. 扫描/清理不够深，希望发现更多可清理内容

> **2026-10-19 更新：用户已确认出问题的 Win10 环境无法使用 PowerShell。**
> 问题 1 的直接根因锁定：`launch_elevated_helper` 依赖 `powershell.exe`，
> 该环境无法创建/运行 PowerShell，因此在 UAC 之前就返回 `os error 5`。
>
> **方案调整（按用户要求）：不移除 PowerShell。** 绝大多数环境 PowerShell 可用，
> 保留它作为主路径；仅当 PowerShell 不存在、无法创建或启动失败时，自动回退到
> Win32 原生 API。回退覆盖提权入口、helper 服务控制、回收站清空和提权检查。
> 正常环境行为不变，禁用 PowerShell 的 Win10 环境仍可完成管理员扫描/清理。

---

## 一、问题 1：管理员全盘扫描无法启动 helper

### 1.1 错误链路

代码路径：

1. `src/App.tsx:496` `handleMftScan()` → `src/lib/api.ts:135` `scanVolumeMft()` → Tauri 命令 `scan_volume_mft`。
2. `src-tauri/src/lib.rs:1404` `scan_volume_mft` → `src-tauri/src/lib.rs:1343` `run_mft_scan_job()`。
3. `run_mft_scan_job` 先写 job 文件，再调用 `src-tauri/src/lib.rs:1696` `launch_elevated_helper()`。
4. `launch_elevated_helper()` 通过：

```rust
std::process::Command::new("powershell.exe")
    .args([... "-Command", &script])
    .output()
    .map_err(|error| format!("无法启动提权 helper：{error}"))?;
```

启动 PowerShell，再由 PowerShell 执行：

```powershell
Start-Process -FilePath <helper> -ArgumentList '--job',<job> -Verb RunAs -Wait -PassThru -WindowStyle Hidden
```

### 1.2 关键判断：失败发生在 UAC 之前

用户看到的完整文案是「无法启动提权 helper：拒绝访问。(os error 5)」。

- `os error 5` 是 Rust `std::io::Error` 在中文系统上的 `Display` 格式（`拒绝访问。 (os error 5)`）。
- 该前缀只出现在 `src-tauri/src/lib.rs:1714`，即 **Rust 连 `powershell.exe` 子进程都没有创建成功**。
- 如果 PowerShell 已经启动、只是 `Start-Process -Verb RunAs` 失败，代码会返回：
  - `提权 helper 失败（退出码 ...）：...`，或
  - `用户取消了管理员授权`（PowerShell 捕获异常后固定 `exit 1223`，见 `lib.rs:1700`）。
- 因此当前现象不是 helper 自身报错、不是规则/路径校验失败，也不是用户取消 UAC。

结论：**提权实现依赖 PowerShell 是本问题的结构性脆弱点。** 在 Win11 开发机正常，不代表 Win10 目标机正常。

### 1.3 Win10 上的可能原因（按可能性排序）

| 可能性 | 机制 | 现象 |
|---|---|---|
| A. 安全软件/EDR 阻止 PowerShell 子进程 | 360、火绒、企业 EDR、AppLocker/WDAC、SRP 阻止创建 `powershell.exe` | 创建进程即 `ERROR_ACCESS_DENIED`，最符合当前文案 |
| B. UAC 被关闭或策略禁止提权 | `EnableLUA=0`、标准用户、`ConsentPromptBehaviorUser=0`、域策略「自动拒绝提权」 | 若 PowerShell 能启动，`Start-Process` 抛「拒绝访问」；当前文案也可能被用户概括成同样的话 |
| C. Secondary Logon 服务被禁用 | 标准用户提权需要凭据提示，`seclogon` 停止 | 无法弹出凭据框，返回拒绝访问 |
| D. 安装/数据目录在提权进程不可见的盘 | 映射网络盘、`subst` 盘、可移动盘、仅当前会话可见的 VHD | 高权限进程找不到/无权访问 helper 或 job 文件 |
| E. 工作目录/PATH 异常 | 从 UNC 路径启动、当前目录不可访问、PATH 中同名 `powershell.exe` | `CreateProcess` 返回 5 或 3 |
| F. helper 被 Defender/AV 隔离或损坏 | 未签名提权程序被拦截，文件变成 0 字节/被锁定 | 可能报拒绝访问；需单独检查 |
| G. 不是 x64 系统 | 32 位 Win10 / ARM64 Win10 | 通常是 193（不是有效的 Win32 程序），不是 5 |

补充：

- 项目当前发布包**未签名**（`.claude/plans/implementation-status.md:107`），Win10 上 SmartScreen/安全软件拦截的概率高于开发机。
- helper 的 `is_elevated()` 也依赖 `whoami.exe` / `net.exe`（`crates/disksage-helper/src/actions.rs:15-32`）；服务清理、回收站清空也依赖 PowerShell（`actions.rs:44-56, 108-143, 395-405`）。如果目标机全局禁 PowerShell，即使 helper 启动成功，这些管理员功能仍会失败。
- **非管理员路径也受影响**：`crates/disksage-clean/src/recycle_bin.rs:5-40` 的 `empty_recycle_bin()` 同样通过 `powershell.exe` 执行 `Clear-RecycleBin`，所以「回收站」规则在该 Win10 环境也会失败；`executor.rs:98` 调用它。

### 1.4 解决方案（P0，建议先做）

#### PowerShell 依赖清单（保留主路径 + 原生回退）

| 位置 | 当前实现 | 影响功能 | 主路径 | 回退路径 |
|---|---|---|---|---|
| `src-tauri/src/lib.rs:1696-1735` | `Start-Process -Verb RunAs` | 所有管理员扫描/清理/MFT | PowerShell（现有行为） | `ShellExecuteExW("runas")` + 等待进程 |
| `crates/disksage-helper/src/actions.rs:44-56, 102-143` | `Get-Service` / `Stop-Service` / `Start-Service` | 传递优化、Windows Update 缓存清理 | PowerShell | SCM API |
| `crates/disksage-helper/src/actions.rs:395-405` | `Clear-RecycleBin` | 管理员计划中的回收站清空 | PowerShell | `SHEmptyRecycleBinW` |
| `crates/disksage-clean/src/recycle_bin.rs:5-40` | `Clear-RecycleBin` | 普通「回收站」规则 | PowerShell | `SHEmptyRecycleBinW` |
| `crates/disksage-helper/src/actions.rs:15-32` | `whoami /groups`、`net session` | helper 提权自检 | 现有实现 | `OpenProcessToken` + `TokenElevation` |

不需要改的：移动文件到回收站使用 `trash` crate（内部 IFileOperation），全盘扫描/MFT、规则匹配、SQLite、LLM 均不依赖 PowerShell。
构建脚本 `scripts/*.ps1`、`package.json` 的 PowerShell 调用只影响开发机，不影响最终用户环境。

#### 方案 1（推荐）：PowerShell 优先，原生 API 自动回退

新增 `src-tauri/src/elevation.rs`，实现一条回退链：

1. **已有管理员令牌**：直接用 `std::process::Command` 启动 helper（不弹 UAC，不经过 PowerShell）。
2. **PowerShell 主路径**：保持现有 `Start-Process -Verb RunAs` 行为，使用完整路径
   `%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe`，工作目录设为本地可访问目录。
3. **原生回退**：如果 PowerShell 不存在、`Command::output()` 返回 `os error 5`/`2`，
   或 PowerShell 在 UAC 之前失败，则使用 `ShellExecuteExW` + `lpVerb="runas"` +
   `SEE_MASK_NOCLOSEPROCESS`；`WaitForSingleObject` 等待，`GetExitCodeProcess` 取退出码。
4. **回退链到此为止**：不重启 UI 提权（项目安全规则要求 UI 始终 asInvoker）；
   两条路径都失败时返回合并错误，并写入诊断日志。

关键细节：

- **不要对用户取消 UAC 做回退**：PowerShell 退出码 1223 / 原生 `ERROR_CANCELLED` 直接返回「用户取消」，
  否则会连续弹两次 UAC。
- 只在**启动级失败**（无法创建 PowerShell、找不到文件、策略拒绝）时回退；PowerShell 正常执行但
  helper 返回业务失败时，不重试。
- **结果文件兜底**：如果 helper 已启动并写出 `<job>.result.json`，即使退出码非 0 也不重试，
  避免重复执行清理/MFT。PowerShell 路径同时用 stdout 标记 `CC_DISKSAGE_HELPER_LAUNCHED` 判断
  helper 是否真的启动过。
- 两条路径都失败时，错误信息同时包含两段原因，例如：
  「PowerShell 不可用（os error 5）；原生提权也失败（Access is denied）。」
- 诊断日志写入 `<数据目录>\logs\helper-launch.jsonl`，记录实际使用路径
  （`mechanism=powershell|native|direct`）、事件（start/success/failure/cancelled/helper_exited）和错误。
- 默认策略为「自动：PowerShell 优先，失败回退原生」；P0 不增加 UI 切换，日志可用于判断实际路径。

同步给 helper 增加回退：

- `is_elevated()`：优先用 `OpenProcessToken` + `GetTokenInformation(TokenElevation)`；
  失败时保留现有 `whoami`/`net` 作为回退（不再把外部进程作为唯一判断）。
- 服务查询/停止/启动：PowerShell 失败（尤其是无法启动 PowerShell）时，用
  `OpenSCManagerW` / `OpenServiceW` / `ControlService` / `StartServiceW` / `QueryServiceStatus`
  重试；只允许现有三个服务名。
- 清空回收站：`Clear-RecycleBin` 失败时，用 `SHEmptyRecycleBinW`（必要时先 `CoInitializeEx`）重试；
  同时替换 `crates/disksage-clean/src/recycle_bin.rs` 的实现，普通「回收站」规则和管理员动作共用。
- `dism.exe` / `powercfg.exe` 保留，但使用 `%SystemRoot%\System32\...` 完整路径。
- `explorer.exe /select` 可后续换成 `SHOpenFolderAndSelectItems`；`schtasks.exe` 可后续换成
  Task Scheduler COM。它们不属于本次 P0，但要确认目标环境是否只禁 PowerShell。

这样正常环境继续走已经验证过的 PowerShell 路径，禁用 PowerShell 的 Win10 环境自动切到原生 API，
不需要用户手动选择。

#### 方案 2：启动前预检 + 可诊断错误

在 `launch_elevated_helper` 之前检查并写入 `logs/helper-launch.jsonl`：

- helper 是否存在、大小是否 > 0、PE 头是否有效、是否有 `Zone.Identifier`、Authenticode 状态。
- 安装目录盘类型：`GetDriveTypeW`（固定盘/网络盘/可移动盘）；`subst`/映射盘检测。
- 数据目录 `jobs` 是否可写；job 文件是否可被高权限进程读取。
- 当前进程完整性级别、是否已提权。
- UAC 注册表：`EnableLUA`、`ConsentPromptBehaviorAdmin`、`ConsentPromptBehaviorUser`、`FilterAdministratorToken`。
- 当前用户是否属于 `BUILTIN\Administrators`（需要处理 UAC 过滤令牌）。
- `seclogon` 服务状态。
- 安全软件产品名（`root\SecurityCenter2`）。
- PowerShell 是否存在/能否启动（用于选择主路径或原生回退，并写入日志）。

UI 错误区分显示，例如：

- 「系统策略拒绝提权：该账户不是管理员，或 UAC 已被关闭。请用管理员账户登录，或右键应用选择“以管理员身份运行”。」
- 「安全软件/组策略阻止了 PowerShell。已改用系统原生提权，请重试；若仍失败，请把诊断日志发给开发者。」
- 「helper 被安全软件隔离或文件损坏，请重新安装并把安装目录加入白名单。」

#### 方案 3：helper 自身加固

- 给 helper 嵌入 `requestedExecutionLevel level="requireAdministrator"` manifest，确保任何启动方式都不会以普通权限运行；非提权 `CreateProcess` 会得到 740，错误更明确。
- 校验 helper 的哈希/版本与主程序一致（防止旧 helper 残留）。
- 保持现有 job nonce、有效期、调用者 SID、规则重新加载、红线校验不变。

#### 方案 4：安装与发布

- 安装时校验 helper 存在、非 0 字节、无 MOTW；必要时 `Unblock-File`。
- 正式发布使用现有签名流水线对 helper、主程序、安装包签名（项目已有 `npm run sign:installers`），降低 Win10 SmartScreen/AV 拦截。
- 检测安装路径：网络盘/`subst`/可移动盘时直接提示「请安装到本地固定磁盘」，不要等到 UAC 失败。
- 安装包增加「诊断」入口或文档，指导用户导出 `logs/helper-launch.jsonl`。

### 1.5 现场快速诊断（在出问题的 Win10 机器上运行）

```powershell
# 1) UAC 与账户
Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System' |
  Select-Object EnableLUA, ConsentPromptBehaviorAdmin, ConsentPromptBehaviorUser, FilterAdministratorToken
whoami /groups | findstr /i "S-1-5-32-544 S-1-16-12288 S-1-16-16384"
Get-Service seclogon | Select-Object Status, StartType

# 2) PowerShell 能否启动（用完整路径，排除 PATH 干扰）
& "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -Command "Write-Output ok"

# 3) 安装盘类型与 helper 状态
Get-CimInstance Win32_LogicalDisk | Select-Object DeviceID, DriveType, ProviderName
Get-Item 'D:\CCDiskSage\disksage-helper.exe' | Select-Object FullName, Length, LastWriteTime
Get-Item 'D:\CCDiskSage\disksage-helper.exe' -Stream Zone.Identifier -ErrorAction SilentlyContinue
Get-AuthenticodeSignature 'D:\CCDiskSage\disksage-helper.exe' | Select-Object Status

# 4) 安全软件
Get-CimInstance -Namespace root/SecurityCenter2 -ClassName AntiVirusProduct |
  Select-Object displayName, productState

# 5) 手动验证 UAC：右键 disksage-helper.exe → 以管理员身份运行
#    正常应弹出 UAC；没有参数时 helper 会以退出码 2 结束，这是预期。
```

如果第 2 步在普通 PowerShell 里成功、但应用里仍报 `os error 5`，基本可以确定是**应用进程的子进程创建被安全软件/策略拦截**或**工作目录/路径问题**，原生提权方案可以直接绕开。

---

## 二、问题 2：扫描和清理不够深

### 2.1 当前实际限制（代码事实）

#### A. 规则清理覆盖面

当前内置规则共 **33 条**：

- `system.toml` 6 条：用户 Temp、缩略图、着色器缓存、Windows Temp、用户 WER、用户崩溃转储
- `system_admin.toml` 7 条：机器 WER、Minidump、传递优化、Windows 更新缓存、DISM 分析/清理、休眠
- `system_maintenance.toml` 4 条：Windows.old、还原点、分页文件、回收站
- `browsers.toml` 3 条：Edge/Chrome/Firefox
- `apps.toml` 4 条：VS Code、Teams 经典版、Discord、Slack
- `dev.toml` 9 条：npm/pnpm/yarn/pip/uv/Go/Cargo/Gradle/NuGet

限制：

- 管理员规则必须走 helper；**问题 1 不解决，管理员层深度清理全部不可用**。
- 所有规则 `verified_on` 都是 `Windows 11 26200`，没有 Windows 10 22H2 的真实核验记录。
- 规则测试门槛未完全落地：`rules/tests/` 为空，`crates/disksage-rules/tests/quick_scan.rs` 只有 8 个测试，并非每条内置规则都有 positive + redline fixture。扩规则前必须先补测试框架。

#### B. 空间分析/treemap 深度

全盘 walker 和 MFT 都会遍历完整目录树，但**返回给 UI 的数据被截断**：

- `src-tauri/src/lib.rs:112-141` `to_treemap()`：只递归 `depth < 2`，即最多 3 层；每个节点最多 50 个子节点；总预算 2500 个节点。
- `src-tauri/src/lib.rs:85-110` `prune_snapshot()`：持久化时只保留前 3 层、每个节点前 100 个子节点、Top 50 顶层目录、200 个大文件。
- `src/pages/ExplorePage.tsx:232` 提示「完整数据保留在本地扫描快照中」，但持久化快照实际上已被裁剪，重启后无法深挖。

所以「扫描不够深」至少有一部分是**展示/持久化深度限制**，不是 walker 没扫到。

#### C. 大文件与重复文件

- 大文件阈值固定 500 MB，Top 200（`crates/disksage-scan/src/walker.rs:16-19`）。
- 重复文件只扫描文档/下载/图片/视频/桌面（`src-tauri/src/lib.rs:1454-1480`），并跳过 AppData、Tencent、node_modules、target、.git 等（`crates/disksage-scan/src/duplicates.rs:17-36`）。这是安全选择，但会漏掉应用目录里的重复内容。

#### D. 安全跳过的目录（不应为了「深」而放开）

- walker/MFT/规则匹配统一跳过：`$Recycle.Bin`、`System Volume Information`、`WindowsApps`、`Recovery`（`walker.rs:21-26`、`mft.rs:104-120`、`matcher.rs:21-26`）。
- 系统红线：`WinSxS`、`Windows\Installer`、`System32`、`SysWOW64`、`Boot`、`Fonts`、`servicing`、`pagefile.sys`、`hiberfil.sys`、`swapfile.sys`（`crates/disksage-core/src/path_safety.rs:136-191`）。
- 不跟随 reparse point / junction / symlink（`walker.rs:166-175`、`matcher.rs:174-203`）。

这些限制必须保留。深度清理应在**规则和受支持 API** 上做文章，而不是放开红线。

### 2.2 解决方案：三层「深度」

#### 第一层：空间分析真正可下钻（只读，风险最低）

目标：让用户能一直点到任意深度的目录，而不是 3 层。

建议实现：

1. 新增 `get_treemap_children(scan_id, path)` 命令，只返回某个目录的**直接子目录**（按大小排序，限制 200 个），前端点击节点时按需加载。
2. 持久化完整目录树：
   - 方案 A（推荐）：每个扫描保存一份 zstd 压缩的完整 `DirNode` 树（BLOB 或独立文件），只保留最近 3~5 次；更早的扫描只保留摘要。
   - 方案 B：SQLite `scan_nodes(scan_id, parent_path, name, allocated_bytes, file_count)` 表，按 `parent_path` 建索引，按需查询。
   - 200k 目录的完整树压缩后通常只有几 MB，成本可控。
3. 初次 treemap 只加载前 3 层；点击节点时再请求下一层，保留 ECharts 的 zoomToNode 交互。
4. 大文件阈值可配置（100 MB / 500 MB / 1 GB），Top N 提高到 2000，并增加「最老文件」「扩展名」「是否签名」筛选。
5. 扫描错误详情页：列出无法访问的目录（权限/占用），提供「用管理员身份 MFT 重扫」。

验收：任意目录可连续下钻 ≥ 10 层；重启后仍可下钻；20 万目录快照加载 < 1 秒；内存峰值可控。

#### 第二层：扩展声明式规则包（清理深度的主体）

原则不变：

- 只通过 TOML 声明式规则删除；动作只引用固定 `ActionSpec`。
- 每条规则必须有 positive + redline fixture、`verified_at`、`verified_on`、官方路径来源。
- 第三方应用规则先在真实 Win10 22H2 + Win11 环境核验，默认风险从 `caution` 起步，默认不勾选。
- 不进入 `WinSxS`、`Windows\Installer`、`WindowsApps`、`System Volume Information`、聊天数据库/媒体、浏览器凭据。
- 管理员规则默认 `default_selected = false`；不可恢复动作必须有二次确认。

建议按批次扩展：

**批次 1：用户级、可重建、Win10/11 都常见（优先）**

| 候选规则 | 路径（待核验） | 风险 | 管理员 | 说明 |
|---|---|---|---|---|
| Chromium 着色器/GPU 缓存 | `%LOCALAPPDATA%\...\User Data\*\ShaderCache`、`GrShaderCache`、`DawnCache`、`DawnGraphiteCache` | safe | 否 | 现有浏览器规则未覆盖 |
| Chromium 崩溃报告 | `...\User Data\Crashpad\reports`、`BrowserMetrics` | safe | 否 | 可重建 |
| Chromium 组件缓存 | `...\User Data\component_crx_cache`、`extensions_crx_cache` | safe | 否 | 扩展/组件会重新下载 |
| Firefox 额外缓存 | `Profiles\*\startupCache`、`shader-cache`、`minidumps` | safe | 否 | 与 cache2 分开 |
| IE/旧式网络缓存 | `%LOCALAPPDATA%\Microsoft\Windows\INetCache` | safe | 否 | 必须排除 Cookie/History |
| Office 文件缓存 | `%LOCALAPPDATA%\Microsoft\Office\16.0\OfficeFileCache` | safe | 否 | 不含文档 |
| OneNote 缓存 | `%LOCALAPPDATA%\Microsoft\OneNote\16.0\cache` | safe | 否 | 需核验版本 |
| Outlook RoamCache | `%LOCALAPPDATA%\Microsoft\Outlook\RoamCache` | caution | 否 | 可能含最近邮件缓存 |
| 新版 Teams 缓存 | `%LOCALAPPDATA%\Packages\MSTeams_8wekyb3d8bbwe\LocalCache` 下已核验子目录 | caution | 否 | 不可整包删除 |
| Squirrel/Electron 更新临时文件 | `%LOCALAPPDATA%\SquirrelTemp` | safe | 否 | 可重建 |
| Edge/Chrome 更新下载缓存 | `%LOCALAPPDATA%\Microsoft\EdgeUpdate\Download`、`%LOCALAPPDATA%\Google\Update\Download` | safe | 否 | 已安装版本不受影响 |
| NVIDIA 额外缓存 | `%LOCALAPPDATA%\NVIDIA\GLCache`、`ComputeCache` | safe | 否 | 与 DXCache 分开 |
| Intel/AMD 额外缓存 | `%LOCALAPPDATA%\Intel\ShaderCache`、AMD `DxcCache`/`GLCache`/`VkCache` | safe | 否 | 需按驱动版本核验 |
| NuGet HTTP 缓存 | `%LOCALAPPDATA%\NuGet\v3-cache`、`plugins-cache` | safe | 否 | 不删 `.nuget\packages` |
| Maven 本地仓库 | `%USERPROFILE%\.m2\repository` | caution | 否 | 会重新下载 |
| Electron 构建缓存 | `%LOCALAPPDATA%\electron\Cache`、`electron-builder\Cache` | safe | 否 | 开发机常见 |
| 游戏着色器缓存 | Steam `steamapps\shadercache`、`depotcache`；Epic `Saved\webcache` | caution | 否 | 需逐游戏/平台核验 |
| Adobe 媒体缓存 | `%APPDATA%\Adobe\Common\Media Cache Files` | caution | 否 | 重新渲染会变慢 |

**批次 2：系统级、需要 helper、默认不勾选**

| 候选规则 | 路径（待核验） | 风险 | 说明 |
|---|---|---|---|
| Prefetch | `%WINDIR%\Prefetch` | caution | 可重建；首次启动略慢 |
| CBS/DISM/更新日志 | `%WINDIR%\Logs\CBS`、`Logs\DISM`、`Logs\WindowsUpdate`、`Logs\MoSetup`、`Logs\SIH` | caution | 仅日志/ cab，逐目录核验 |
| Panther 安装日志 | `%WINDIR%\Panther` | caution | 安装完成后可清 |
| LiveKernelReports | `%WINDIR%\LiveKernelReports` | caution | 内核转储；可能较大 |
| MEMORY.DMP | `%WINDIR%\MEMORY.DMP` | danger | 建议引导用户确认，不自动勾选 |
| Windows SystemTemp | `%WINDIR%\SystemTemp`（若存在） | caution | Win10/11 版本差异，需 detect |
| 传递优化备用路径 | `%PROGRAMDATA%\Microsoft\Windows\DeliveryOptimization\Cache` | caution | 与现有 NetworkService 路径分开 |
| 升级残留 | `%SYSTEMDRIVE%\$GetCurrent`、`$WINDOWS.~BT`、`$Windows.~WS`、`$WinREAgent`、`ESD` | danger | 优先走 cleanmgr/Storage Sense 引导，不直接删 |
| Windows.old | `%SYSTEMDRIVE%\Windows.old` | caution | 现有引导项；补充只读大小与 cleanmgr 执行 |
| Microsoft Store 缓存 | `%LOCALAPPDATA%\Packages\Microsoft.WindowsStore_8wekyb3d8bbwe\LocalCache` | caution | 应用会重建 |
| Windows Installer 缓存 | **禁止** | — | 红线，不做 |
| 组件存储 | 现有 DISM Analyze/StartComponentCleanup | caution | 继续只用 DISM；绝不手删 WinSxS |

**批次 3：只读「深度发现」，不直接删除**

新增独立的 `deep_inspect`（只读）能力，输出 `DiscoveryItem`（不是规则 Finding，不参与勾选/删除）：

- 全盘 ≥100 MB 文件，按年龄/扩展名/目录聚合；可标记「安装包/日志/转储/视频/备份」。
- 用户目录中 90 天以上的旧安装包（`*.msi`、`*.exe`、`*.iso`）。
- 任意位置的大日志/转储（`*.log`、`*.etl`、`*.dmp`、`*.mdmp`、`*.cab`），仅建议。
- 指定项目根目录下的构建产物（`target`、`node_modules`、`.next`、`dist`、`build`、`obj`、`bin`）——只读统计，用户自行决定。
- WSL/Docker 的 `ext4.vhdx` 大小与压缩建议（`Optimize-VHD`/`diskpart` 引导，不删除）。
- 空目录、孤儿用户目录、旧 Windows 安装——仅展示与引导。

这些项目**永远不进入自动勾选/自动清理**；LLM 只能基于元数据给建议，不能新增候选或执行删除。

#### 第三层：产品化「深度清理」模式

- 清理页增加深度档位：
  - **安全**：现有 safe + 用户级可重建缓存（默认勾选）
  - **谨慎**：caution + 部分第三方应用缓存（默认不勾选）
  - **系统维护**：管理员规则、日志、Prefetch、DISM、Windows.old（一次 UAC，逐项确认）
  - **仅建议**：大文件/旧安装包/开发产物（只读）
- 每条规则展示：`verified_at` / `verified_on`（Win10 22H2 / Win11）、风险、是否需要管理员、`size_provenance`、E1–E8 证据。
- 年龄策略：Temp 1 天、WER 7 天、转储 30 天、浏览器缓存 0 天；可配置。
- 管理员动作一次 UAC 批量执行（保持现有 job 机制）。
- 永久删除仍单独开关 + 二次确认；新增规则不得绕过。

### 2.3 规则准入与测试框架（扩规则前必须先做）

当前 `rules/tests/<rule_id>/positive|redline` 目录为空，只有 8 个规则测试。建议先补：

1. 通用 fixture runner：扫描 `rules/tests/<rule_id>/positive`，断言规则命中预期文件；扫描 `redline`，断言**零命中**。
2. 全局红线测试：所有内置规则对 `WinSxS`、`Windows\Installer`、`WindowsApps`、`System Volume Information`、`System32`、浏览器凭据、聊天数据库等路径零命中。
3. 第三方应用规则核验记录模板：应用版本、安装路径、官方文档链接、Win10 22H2 / Win11 构建号、截图/命令输出。
4. CI 门槛：新增规则没有 positive + redline 测试直接失败。
5. Win10 22H2 VM 与 Win11 真机双环境跑规则测试；所有规则 `verified_on` 明确写实际环境，不再统一写「Windows 11 26200」。

### 2.4 不建议做的事

- 不为了「深」而遍历 reparse point/junction，或删除 `WinSxS`/`Windows\Installer`/`WindowsApps`/`System Volume Information`。
- 不引入任意命令/脚本规则，不用通用 `C:\Windows\Logs\**` 大范围通配。
- 不把第三方规则库（Winapp2 等）直接导入安装包。
- 不默认勾选 caution/admin/irreversible 项。
- 不把聊天数据库、聊天媒体、浏览器 Cookie/密码/历史/书签作为清理目标。
- 不用 `/ResetBase`，不手删 WinSxS。

---

## 三、分阶段实施计划

| 阶段 | 内容 | 验收 |
|---|---|---|
| P0（1-2 天） | 提权回退链（PowerShell 优先 → 原生 `ShellExecuteExW`）；helper 服务控制/回收站原生回退；`disksage-clean::recycle_bin` 原生回退；`helper-launch.jsonl` 诊断日志；错误码映射 | 正常 Win10/11 仍走 PowerShell 且行为不变；禁用 PowerShell 的 Win10 自动走原生路径，能启动 helper、清服务缓存、清空回收站；取消 UAC 不重复弹窗；失败时日志可定位 |
| P1（2-3 天） | treemap 按需下钻 + 完整树持久化；大文件阈值/Top N 可配置；扫描错误详情 | 任意深度下钻；重启后仍可下钻；20 万目录加载 < 1 秒 |
| P2（3-5 天） | 规则测试框架 + 批次 1 用户级规则 + Win10/11 核验 + fixtures | 每条规则 positive/redline 通过；红线零命中；真机大小核验 |
| P3（3-5 天） | 批次 2 管理员规则 + 一次 UAC 批量执行 + 系统引导项 | Win10/11 管理员扫描/清理实测；DISM/服务恢复；审计完整 |
| P4（3-5 天） | `deep_inspect` 只读深度发现 + 深度清理 UI 档位 | 只读；不产生可删除候选；大文件/旧安装包可打开目录；LLM 边界不变 |

### 需要用户确认/提供的信息

1. 出问题的 Win10 机器：UAC 设置、账户是否管理员、是否安装 360/火绒/企业 EDR、安装盘是否为本地固定盘、手动右键 helper「以管理员身份运行」是否弹 UAC。
2. 期望优先覆盖的「深度可清理」内容：Windows 更新/组件存储、Windows.old、浏览器缓存、Office/Teams、开发缓存、游戏平台、微信/QQ（仅缓存，不碰聊天数据）等。
3. 是否接受把正式发布包纳入代码签名（当前流水线已完成，但按之前决定暂缓）。

---

## 四、P0 实现记录（2026-10-19）

已实现：

- `src-tauri/src/elevation.rs`（新增）
  - 已提权 → 直接启动 helper。
  - PowerShell 主路径：完整路径 `System32\WindowsPowerShell\v1.0\powershell.exe`，
    `Start-Process -Verb RunAs -PassThru` + `WaitForExit`，输出
    `CC_DISKSAGE_HELPER_LAUNCHED` 标记。
  - 取消 UAC（1223 / `0x800704C7` / 取消文案）不重试。
  - PowerShell 不可用或 UAC 前失败 → `ShellExecuteExW("runas")` 原生回退。
  - helper 已启动（stdout 标记或 `<job>.result.json` 已存在）但退出码非 0 时不再重试。
  - 启动前检查 helper 存在、非空、job 存在；失败写入
    `<数据目录>\logs\helper-launch.jsonl`。
- `crates/disksage-helper/src/actions.rs`
  - `is_elevated()` 改为原生 `OpenProcessToken` + `TokenElevation` 优先，
    `whoami`/`net` 保留为回退。
  - 服务查询/停止/启动：PowerShell 优先，失败时回退 SCM API；
    服务名仍只允许 `wuauserv/bits/dosvc`。
  - 回收站清空改为复用 `disksage-clean::recycle_bin`。
- `crates/disksage-clean/src/recycle_bin.rs`
  - `Clear-RecycleBin` 优先，失败时 `SHEmptyRecycleBinW` 回退。
- `docs/architecture.md`、`.claude/plans/implementation-status.md` 同步更新。

验证结果：

- `cargo test --workspace`：通过（0 失败）。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过。
- `npm run test`：3 项通过。
- `npm run build`：通过。
- `npm run build:helper:release`：通过，sidecar 已更新。
- `npm run tauri build`：通过，生成
  `target\release\bundle\nsis\CC DiskSage_0.2.0_x64-setup.exe`（约 211.8 MiB）。

待真机验证：

- 在禁用 PowerShell 的 Windows 10 上安装该包，验证：
  1. 管理员全盘扫描（MFT）自动走原生提权；
  2. 管理员扫描（quick_scan）成功；
  3. 传递优化 / Windows Update 缓存清理走 SCM 回退；
  4. 回收站清空走 `SHEmptyRecycleBinW` 回退；
  5. `logs/helper-launch.jsonl` 中出现 `mechanism=native`；
  6. 正常 Win11 环境仍出现 `mechanism=powershell`。

## 五、深度清理批次 1（2026-10-19）

已落地 9 条规则：

| 规则 | 风险 | 管理员 | 默认勾选 | 真机实测（Windows 11 26200） |
|---|---|---|---|---|
| `browser.edge_component_cache` | caution | 否 | 否 | 177.3 MB / 24 文件 |
| `browser.chrome_component_cache` | caution | 否 | 否 | 77.7 MB / 24 文件 |
| `browser.edge_gpu_cache` | safe | 否 | 是 | 2.1 MB / 23 文件 |
| `browser.chrome_gpu_cache` | safe | 否 | 是 | 8.2 MB / 33 文件 |
| `system.servicing_logs` | caution | 是 | 否 | 606.9 MB / 114 文件（非提权估算） |
| `system.prefetch` | caution | 是 | 否 | 0 B（本机 Prefetch 为空） |
| `system.panther_logs` | caution | 是 | 否 | 1 个日志文件 |
| `system.live_kernel_reports` | caution | 是 | 否 | 0 B |
| `system.windows_system_temp` | caution | 是 | 否 | 0 B |

配套：

- `crates/disksage-rules/tests/builtin_fixtures.rs`：按规则 ID 跑
  `rules/tests/<rule_id>/{positive,redline}`，fixture 的 mtime 自动设为一年前，
  覆盖 `min_age_days`；红线 fixture 必须零命中。
- `rules/tests/<rule_id>/...`：9 条规则各自的 positive + redline fixture。
- `docs/rule-authoring.md`：补充 fixture 目录约定和新规则检查清单。

边界：

- `system.servicing_logs` 只匹配 `CbsPersist_*.log`、`*.cab`、DISM/WindowsUpdate/MoSetup/SIH/waasmedic
  的日志/ETL；**不匹配 CBS.log**，不触碰 WinSxS，不删除已安装更新。
- `system.prefetch` 只匹配 `*.pf`，不删除 `layout.ini`。
- `system.panther_logs` 只匹配日志/ETL/转储，不删除 XML/TXT/INI。
- 浏览器规则硬排除 Cookie、登录数据、历史、书签、Local Storage、Session Storage、
  IndexedDB、Service Worker、Network、Preferences、Local State。
- 组件/扩展 CRX 缓存默认不勾选（caution）；GPU/着色器缓存为 safe、默认勾选。

待办：

- 在提权 helper 下实测 `system.servicing_logs` 等管理员项的大小与回收站删除。
- 在 Windows 10 22H2 上核验路径与大小，并更新各规则的 `verified_on`。
- 继续批次 2：Office/Teams、游戏平台、开发缓存、升级残留、只读深度发现。

## 六、参考

- 现有实现状态：`.claude/plans/implementation-status.md`
- 规则引擎与安全边界：`.claude/plans/disk-cleaner-implementation.md` 第 8 节
- 规则准入：`.claude/CLAUDE.md`「规则准入」
- 路径红线：`crates/disksage-core/src/path_safety.rs`
- 提权链路：`src-tauri/src/lib.rs:1196, 1343, 1696`
- helper 动作：`crates/disksage-helper/src/actions.rs`
