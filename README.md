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
- 仪表盘：容量环形图、Top 目录、大文件列表
- 空间分析：ECharts treemap，可下钻
- 规则引擎：TOML 规则、路径安全、红线校验、positive/redline 测试
- 快速扫描：用户临时文件、缩略图、着色器缓存、浏览器缓存、开发缓存、常见应用缓存
- 清理计划预览 → 确认 → 回收站删除 → 本地审计日志
- 管理员级系统清理：Windows Temp、机器级 WER、Minidump、传递优化、Windows Update 下载缓存、DISM 分析/清理、休眠文件；通过 UAC 提权 `disksage-helper.exe` 执行
- 系统引导动作：Windows.old（打开磁盘清理）、系统还原点（系统保护）、分页文件（系统属性 → 虚拟内存）
- 回收站清空：通过系统 API 执行，明确不可恢复
- LLM 设置（OpenAI 兼容 / Ollama `/v1`）、payload 预览与同意、逐项 AI 分析、继续追问
- 历史记录页（读取本地审计日志）
- NSIS 安装包（offline WebView2 / lite 两种配置）

尚未实现（后续里程碑）：

- NTFS MFT 秒扫
- 重复文件查找
- 定时 / 低空间自动清理
- 自动更新与代码签名

## 安全模型

- 默认删除到回收站；永久删除只允许可重建缓存并需要二次确认。
- 管理员动作只通过 UAC 提权 helper 执行：helper 重新加载编译进二进制的规则、重新扫描目标、再次校验红线，只执行固定动作；服务名有白名单，DISM 不使用 `/ResetBase`。
- 系统引导动作（Windows.old/还原点/分页文件）只打开 Windows 系统界面，不直接删除；回收站清空需要单独确认，清空后不可恢复。
- 规则只能引用固定动作枚举，TOML 中不存在任意命令或参数。
- 全局红线：WinSxS、Windows Installer、WindowsApps、System Volume Information、
  pagefile/hiberfil/swapfile、注册表 hive、浏览器凭据、聊天数据库/媒体。
- 永不跟随 reparse point / junction / symlink。
- 前端只能传 finding / plan ID，不能传任意删除路径。
- 每条规则必须有 positive 与 redline fixture。
- 所有展示数字来自扫描器/helper，并标注 `size_provenance`。

## 安装包

| 文件 | 大小 | 适用场景 |
|---|---|---|
| `dist-installers/cc-disksage-0.1.0-offline-setup.exe` | 约 211 MB | 断网 Win10/11；内置 WebView2 离线安装器 |
| `dist-installers/cc-disksage-0.1.0-lite-setup.exe` | 约 3.7 MB | 已装 WebView2 的 Win11/企业环境；缺失时 bootstrapper 需联网 |

安装包同时包含 `disksage-helper.exe`（提权清理侧车）。管理员项执行时会弹出 UAC。
SHA-256 见 `dist-installers/SHA256SUMS.txt`。

安装为当前用户（`%LOCALAPPDATA%\CC DiskSage`），不需要管理员。当前未签名，
SmartScreen 可能提示。

### 已验证

- 静默安装 → 启动（窗口正常）→ 静默卸载
- Windows Credential Manager API Key 写入/读取/删除
- 回收站删除 API（临时文件）
- C 盘真实只读扫描与快速扫描（含管理员规则）
- helper 非管理员环境拒绝执行并写出结果；服务/目录/DISM 动作单元测试通过
- 快速扫描出现 Windows.old / 还原点 / 分页文件 / 回收站维护项；引导程序路径已核验
- Rust workspace 全部测试 + 前端 3 项测试通过
- 待验证：offline 包在**干净 Windows 10（无 WebView2、断网）**上的安装（需要 VM）
- 待人工验证：带管理员项的真实 UAC 清理（建议在 VM 或确认后操作）

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
- `.claude/plans/disk-cleaner-implementation.md`（完整方案）

## 说明

产品公开名称 `DiskSage` 已被其他开源项目使用；对外发布前需要确认改名或商标。
当前安装包未签名，Windows SmartScreen 可能提示；代码签名在后续里程碑接入。
