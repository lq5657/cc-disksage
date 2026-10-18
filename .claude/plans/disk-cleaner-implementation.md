# CC-DiskSage：Windows 智能磁盘清理工具 — 实现方案

> 说明：Claude Code 的 Plan Mode 只允许写入系统指定的计划文件。批准后第 0 步会把本方案复制到项目内 `D:\work\claude-projects\cc-disksage\.claude\plans\disk-cleaner-implementation.md`，后续所有计划、规则文档、Agent 说明均放在项目 `.claude` 下，不再写入用户级 `.claude`。

## 1. Context（为什么做）

目标机器（Windows 11 Home China，C: 200GB 仅剩 ~71.6GB）磁盘占用高，尤其 C 盘。市面工具存在三类问题：

1. **不透明**：CCleaner / 360 / 各种"一键优化"直接给出"可清理 XX GB"，用户无法判断删了什么、为什么能删、删了会怎样。
2. **不安全**：注册表清理、手删 WinSxS、激进通配符删除，容易破坏系统或应用数据。
3. **不智能 / 不离线**：多数 AI 清理项目要么依赖在线服务，要么只是聊天壳；企业/离线环境无法使用。

本产品要做的是：**离线可安装、规则驱动、证据优先的 Windows 清理工具**，可选接入 OpenAI 兼容 LLM，对每个候选清理项生成「事实证据链 + 影响说明 + 可恢复性 + AI 判断」，最终由用户决定是否清理。

### 已确认的产品决策

| 决策 | 结论 |
|---|---|
| 技术栈 | Rust + Tauri 2 + React/TypeScript，前端资源全部打包进安装包 |
| 离线 | 不依赖 CDN；SQLite 内嵌；Win10 离线安装包内置 WebView2 离线安装器；Win11 系统自带 WebView2 |
| Rust 工具链 | **终端客户无需安装 Rust**。Rust 编译产物为原生二进制，安装包只包含 exe/前端资源/规则库。仅开发与构建机需要 Rust |
| LLM | 首版支持 OpenAI 兼容云 API（BYOK：DeepSeek / 通义千问 / 智谱 / Moonshot / OpenAI 等） |
| LLM 形态 | 不做完整聊天产品；以「逐项证据面板 + 可清理摘要报告」为主，详情内提供轻量「继续追问」框 |
| 首版范围 | 空间仪表盘、规则化安全清理、大文件查找、LLM 证据链、审计日志、Windows 安装包 |
| 二期 | NTFS MFT 秒扫、重复文件、定时/阈值自动清理、自动更新、代码签名 |
| 明确不做 | 注册表清理、手删 WinSxS、强制删除占用文件、静默自动删除用户数据 |
| 产品命名 | 内部目录名 `cc-disksage`；公开名称需避开已存在的 DiskSage 项目，发布前确认（见风险表） |

## 2. 参考产品与取舍

| 产品 | 值得借鉴 | 明确不学 |
|---|---|---|
| CCleaner | 分类清理、健康检查、定时任务 | 注册表清理、捆绑推广、恐吓式营销 |
| BleachBit | 声明式规则（CleanerML/winapp2.ini）、预览、CLI | 过宽的通配符规则 |
| WizTree | 直读 NTFS MFT，2–5 秒全盘扫描 | 仅 NTFS/需管理员（作为二期增强，非唯一路径） |
| TreeSize / SpaceSniffer | 文件夹树 + treemap 空间可视化 | 纯分析、不提供安全清理闭环 |
| Microsoft PC Manager / Storage Sense | 保守的一线系统清理、自动化 | 与系统设置深度耦合、功能受限 |
| Dism++ | DISM 组件清理、更新缓存处理 | 暴露过多危险选项给普通用户 |
| Pinkbin | Tauri+Rust、MFT、只发送目录元数据给 LLM、回收站+撤销、规则红线测试 | 规则覆盖面仍较窄 |
| DiskSage | 只读扫描、逐项确认、开发/AI 缓存识别、Ollama 本地推理 | Python/Flask 桌面体验与安装形态 |

**核心借鉴结论**：规则必须声明式、可测试、带「红线」断言；AI 只做解释与排序，不拥有删除权；所有删除可预览、可恢复、可审计。

## 3. 产品原则（不可妥协）

1. **安全优先于回收量**：宁可少清，不可误删；所有规则有红线测试。
2. **证据优先于结论**：每个可清理项都必须能回答"这是什么、为什么能删、删了会怎样、能否恢复"。
3. **本地优先**：无网络也能完成扫描、规则清理、证据展示；LLM 只是可选增强。
4. **用户拥有最终决定权**：不静默删除、不默认勾选危险项、不诱导付费。
5. **最小数据外发**：LLM 只接收聚合元数据，默认脱敏，发送前可见。
6. **不做注册表清理**、不做"一键优化"、不碰系统红线目录。

## 4. 总体架构

```
┌─────────────────────────────────────────────────────────────┐
│ React + TypeScript + Vite + ECharts（打包进安装包，无 CDN） │
│  仪表盘 / 空间分析 / 清理建议 / 证据抽屉 / 设置 / 历史       │
└──────────────────────────┬──────────────────────────────────┘
                           │ Tauri IPC（typed commands + events）
┌──────────────────────────▼──────────────────────────────────┐
│ Rust Core（src-tauri + crates）                              │
│  ├─ scanner     卷信息、快速扫描、全盘扫描、treemap 聚合      │
│  ├─ rules       TOML 规则加载、路径展开、匹配、红线校验       │
│  ├─ clean       计划、预览、执行、回收站/隔离区、审计         │
│  ├─ llm         OpenAI 兼容客户端、脱敏、结构化输出、校验     │
│  ├─ storage     SQLite（快照/发现/审计/LLM 缓存/设置）        │
│  └─ elevated    独立提权 helper（仅执行白名单规则动作）       │
└──────────────────────────┬──────────────────────────────────┘
                           │
              Windows APIs / 文件系统 / DISM / 服务控制
```

**安全边界**：UI 永远不以管理员运行；需要管理员的动作交给窄接口的 `disksage-helper.exe`，通过 UAC 单次授权执行规则白名单内的任务。

## 5. 仓库结构

```
cc-disksage/
├─ Cargo.toml                     # workspace
├─ crates/
│  ├─ disksage-core/              # 领域类型、错误、路径安全、证据模型
│  ├─ disksage-scan/              # 卷信息、jwalk 扫描、聚合、MFT(二期)
│  ├─ disksage-rules/             # TOML 规则 schema/loader/matcher
│  ├─ disksage-clean/             # 计划、执行、回收站、隔离、审计
│  ├─ disksage-llm/               # OpenAI 兼容客户端、脱敏、schema 校验
│  └─ disksage-helper/            # 提权 helper bin（仅 scan/clean 固定动作）
├─ src-tauri/                     # Tauri 应用、commands、state、tray
│  ├─ src/main.rs / lib.rs / commands/*.rs
│  ├─ tauri.conf.json
│  ├─ capabilities/default.json
│  └─ icons/
├─ src/                           # React 渲染层
│  ├─ pages/{Dashboard,Scan,Cleanup,History,Settings}.tsx
│  ├─ components/{DriveRing,Treemap,FolderTree,FindingList,EvidenceDrawer,AiSummary,Progress}.tsx
│  ├─ lib/{ipc.ts,i18n.ts,format.ts}
│  └─ stores/
├─ rules/
│  ├─ builtin/*.toml              # 内置规则（编译进二进制）
│  └─ tests/<rule_id>/{positive,redline}/   # 每条规则必须有
├─ tests/fixtures/
├─ scripts/{vendor-offline.ps1,build-installer.ps1}
├─ docs/{architecture,rule-authoring,privacy}.md
├─ .claude/plans/                 # 项目内计划文档（批准后创建）
├─ package.json / vite.config.ts / tsconfig.json
└─ README.md
```

### 5.1 核心依赖（用 lockfile 固定版本，构建时校验许可证）

| 领域 | 选型 | 用途 |
|---|---|---|
| 桌面框架 | `tauri 2` + `@tauri-apps/cli` | 窗口、IPC、打包 |
| 前端 | React 19、TypeScript、Vite、Tailwind、zustand、i18next | UI |
| 可视化 | ECharts（treemap/line，tree-shaking） | 空间可视化 |
| 图标 | lucide-react（打包进 bundle） | 图标 |
| 目录遍历 | `jwalk`、`walkdir`、`globset`、`rayon` | 并行扫描/匹配 |
| Windows API | `windows` / `windows-sys` | 卷信息、服务控制、reparse point、ShellExecute |
| 回收站 | `windows` crate 直接实现 `IFileOperation`（逐项错误 + 回收站映射，不用 `trash` crate） | 可恢复删除 |
| 数据库 | `rusqlite`（bundled） | 快照/审计/设置 |
| 序列化 | `serde`、`toml`、`serde_json`、`postcard` + `zstd` | 规则/DTO/压缩快照 |
| 规则内嵌 | `rust-embed` | 内置规则编译进 exe |
| LLM | `reqwest`（rustls）、`tokio`、`jsonschema`、`schemars` | HTTP/校验 |
| 密钥 | Windows Credential Manager（`CredWriteW`/`CredReadW` 薄封装；`keyring` 仅作备选） | API Key |
| 错误/日志 | `thiserror`、`anyhow`、`tracing` + rolling appender | 可诊断 |
| 类型生成 | `ts-rs` | Rust → TS DTO |
| 测试 | `cargo test`/`nextest`、Vitest、React Testing Library、mock HTTP server | 单元/集成 |

## 6. 领域模型（核心类型）

```rust
pub struct VolumeInfo { pub letter: char, pub label: String, pub fs: String,
                        pub total_bytes: u64, pub free_bytes: u64 }

pub struct ScanSnapshot {
    pub id: String, pub volume: char, pub started_at: i64, pub duration_ms: u64,
    pub total_bytes: u64, pub allocated_bytes: u64, pub file_count: u64, pub dir_count: u64,
    pub error_count: u64, pub node_limit_hit: bool,
    pub top_dirs: Vec<DirNode>, pub large_files: Vec<LargeFile>,
}

pub struct Finding {
    pub id: String, pub rule_id: String, pub rule_version: u32,
    pub category: String, pub title_zh: String, pub title_en: String,
    pub risk: RiskLevel, pub recoverability: Recoverability,
    pub requires_admin: bool, pub default_selected: bool,
    pub targets: Vec<MatchedPath>,              // 规则模板 + 实际路径（前端不直接使用）
    pub allocated_bytes: u64, pub logical_bytes: u64, pub file_count: u64,
    pub size_provenance: SizeProvenance,        // Scanner | Helper | Estimated
    pub oldest_mtime: Option<i64>, pub newest_mtime: Option<i64>,
    pub ext_histogram: Vec<(String, u64)>, pub sample_files: Vec<SampleFile>,
    pub evidence: Vec<Evidence>, pub consequence: String,
    pub action: ActionSpec, pub llm: Option<LlmAssessment>,
}

pub enum RiskLevel { Safe, Caution, Danger }
pub enum Recoverability { Regenerable, RecycleBin, Quarantine, Irreversible, UserData }

pub struct Evidence {
    pub id: String,                             // E1、E2…，单个 Finding 内稳定
    pub source: EvidenceSource, pub kind: EvidenceKind,
    pub claim: String,                          // 由本地数据生成，绝不由 LLM 生成
    pub path: Option<PathBuf>, pub bytes: Option<u64>,
    pub timestamp: Option<i64>, pub rule_id: Option<String>, pub confidence: f32,
}
pub enum EvidenceSource { Rule, Scanner, AppDetection, Llm }
pub enum EvidenceKind { RuleProvenance, Path, Size, Age, AppDetection, LlmInference }

pub enum ActionSpec {
    Trash, Delete, Quarantine, EmptyRecycleBin,
    ServiceCleanup { services: Vec<String>, paths: Vec<PathBuf> },
    DismAnalyze, DismStartComponentCleanup, Guided(GuideId),
    HibernateOff, CreateRestorePoint,
}

pub struct LlmAssessment {
    pub schema_version: u32, pub finding_id: String,
    pub verdict: Verdict, pub confidence: Confidence,
    pub summary: String, pub reasoning: Vec<ReasoningStep>,
    pub cited_evidence_ids: Vec<String>, pub consequence: String, pub caveats: Vec<String>,
    pub model: String, pub generated_at: i64, pub payload_hash: String,
}
pub enum Verdict { Cleanable, Review, DoNotClean }
pub enum Confidence { Low, Medium, High }
pub struct ReasoningStep { pub text: String, pub evidence_ids: Vec<String> }

pub struct CleanupPlan { pub id: String, pub snapshot_id: String, pub created_at: i64,
                         pub items: Vec<PlanItem>, pub total_allocated_bytes: u64,
                         pub requires_admin: bool, pub warnings: Vec<PlanWarning>, pub status: PlanStatus }
pub struct PlanItem { pub finding_id: String, pub rule_id: String, pub action: ActionSpec,
                      pub targets: Vec<PathBuf>, pub estimated_bytes: u64, pub file_count: u64,
                      pub recoverability: Recoverability, pub requires_admin: bool }
pub struct CleanupResult { pub job_id: String, pub freed_bytes: u64, pub deleted: u64,
                           pub skipped_locked: Vec<PathBuf>, pub failed: Vec<(PathBuf, String)>,
                           pub cancelled: bool }
```

**证据链原则**：所有数字来自本地扫描器或提权 helper，并带 `size_provenance`；LLM 只能引用 `evidence[].id`，不得产生新数字、路径或动作。UI 校验引用 id 存在，数字不一致时丢弃 LLM 文案，回退到本地模板。TS 边界上 64 位 ID 用字符串，字节数超过 `2^53` 时也用字符串。

## 7. 扫描器设计

### 7.1 快速扫描（清理预览，目标 <5 秒）
- 只遍历规则目标路径（`%TEMP%`、浏览器缓存目录、开发缓存等），按规则过滤。
- 按根目录分组，多个规则共享一次遍历；globset 预编译；避免对同一个 `%LOCALAPPDATA%` 反复扫描。
- 先执行 detect guard；管理员规则在提权 helper 计算前标记为 `Estimated`，不把访问受限的部分遍历结果当成权威大小。
- 每条规则输出 `Finding` + `Evidence` + 可回收字节数（带 `size_provenance`）。
- 不扫描整个 C 盘，不阻塞 UI。

### 7.2 全盘扫描（空间分析，目标 1M 文件 <60s 热缓存 / <90s 冷缓存）
- v1 使用自研 `rayon` 并行目录遍历（快速扫描可用 `jwalk`）；线程数 4–8；跳过 reparse point / junction，避免循环。
- 目录 arena：每个目录只做一次短锁插入；子节点 ID > 父节点 ID，便于反向自底向上聚合；节点上限 200 万，超过后停止为 <64KB 文件建节点并置 `node_limit_hit`。
- 跳过 `$Recycle.Bin`、`System Volume Information`、`WindowsApps` 内部等；权限不足目录计入 `error_count`，记录最多 100 个样本路径，不中断。
- 大小：目录枚举得到逻辑大小；>1MiB 或 compressed/sparse/reparse 文件用 `GetCompressedFileSizeW` 取分配大小，避免逐文件句柄开销。
- 硬链接：>1MiB 文件用 `GetFileInformationByHandle` 取 `nNumberOfLinks` 与 file ID；全局可回收估算按 `(volume_serial, file_index)` 去重；WinSxS 不计入可回收。
- 聚合：目录大小树、Top-N 大文件（最小堆）、扩展名/年龄直方图。
- 内存边界：不保留全部文件路径；node cap；快照用 `postcard + zstd` 压缩；SQLite 保留最近 5 次全盘 / 20 次快速扫描。
- 取消：`AtomicBool` 每目录/每 256 文件检查；进度 100ms 节流，通过 Tauri `Channel<ScanProgress>` 推送。
- IPC 分页：不把整棵树发给前端；`get_treemap(snapshot_id, node_id, max_tiles)`（≤10,000 tiles）、`get_children(...offset, limit)`、`get_large_files(...)`。
- 快照写入 SQLite，支持趋势对比与重启后秒开。

### 7.3 二期 MFT 快扫
- 独立 `MftScanner` 实现同一 `ScanEngine` trait；使用 `mft` / `ntfs` crate 直读 NTFS MFT（需管理员、仅 NTFS），目标 2–5 秒。
- 非 NTFS / 无管理员自动回退到 `WalkerScanner`。
- 与遍历结果做基准对比测试；硬链接按 file ID 去重。

## 8. 规则引擎

### 8.1 规则格式（TOML，示例）

```toml
schema_version = 1

[[rules]]
id = "system.user_temp"
version = 1
verified_at = "2026-10-18"
verified_on = "Windows 11 26200"
category = "system_temp"
title_zh = "用户临时文件"
title_en = "User temp files"
risk = "safe"
requires_admin = false
default_selected = true
recoverability = "regenerable"
min_age_days = 1

[detect]
path_exists_any = ["%TEMP%"]

[[targets]]
root = "%TEMP%"
include_globs = ["**/*"]
exclude_globs = ["**/*.lock", "**/*.tmp.lock", "**/~$*"]
max_depth = 64
follow_reparse_points = false

[explain]
what_zh = "应用程序产生的临时文件，正常关闭后通常不再需要。"
impact_zh = "正在被占用的文件会跳过；部分程序可能需要重新生成临时数据。"
exclude_notes_zh = "不会删除文档、桌面、下载目录或浏览器数据。"
source = "Microsoft Windows TEMP 约定"

action = { type = "trash" }

# 管理员/服务类动作也只能引用固定枚举，不能在 TOML 里写任意命令：
[[rules]]
id = "system.wu_download_cache"
version = 1
verified_at = "2026-10-18"
category = "windows_update"
title_zh = "Windows 更新下载缓存"
risk = "caution"
requires_admin = true
default_selected = false
recoverability = "regenerable"
detect = { path_exists_all = ["%WINDIR%\\SoftwareDistribution\\Download"] }
action = { type = "service_cleanup",
           services = ["wuauserv", "bits", "dosvc"],
           paths = ["%WINDIR%\\SoftwareDistribution\\Download"] }
```

### 8.2 引擎与路径安全要求
- `serde` + TOML 反序列化，启动时校验 schema；内置规则用 `rust-embed` 编译进二进制；未知字段一律拒绝。
- 用户可在 `%LOCALAPPDATA%\cc-disksage\rules\` 添加规则；用户规则**只能是附加**：只能使用 `trash`/`delete`/`quarantine`，只能作用于用户选择的根目录，不能定义管理员/命令/DISM/引导动作，不能移除红线，不能降低内置规则的风险等级。规则总数上限 200。
- **action 是固定 Rust 枚举**：`trash`、`delete`、`quarantine`、`empty_recycle_bin`、`service_cleanup`、`dism_analyze`、`dism_component_cleanup`、`guided`、`hibernate_off`、`create_restore_point`。服务名、DISM 参数、引导工具 ID 全部硬编码在 `disksage-clean::actions`，规则只能按 ID 引用；TOML 中不存在任意可执行命令/参数。
- **路径安全**（UI 进程与 helper 共用同一实现）：
  1. 只展开白名单环境变量（`%TEMP%`、`%TMP%`、`%LOCALAPPDATA%`、`%APPDATA%`、`%USERPROFILE%`、`%PROGRAMDATA%`、`%WINDIR%`、`%SYSTEMDRIVE%`）。
  2. 拒绝 UNC、设备路径、`\\?\GLOBALROOT`、相对路径、规范化后仍含 `..` 的路径。
  3. 统一分隔符/大小写比较；保留原始路径用于展示。
  4. 拒绝将卷根、Windows 根、用户目录根、桌面/文档/下载根、AppData 根本身作为删除目标；只能删除其下匹配的后代。
  5. 全局红线目录大小写不敏感地强制拦截，任何规则/用户规则都不能覆盖。
  6. 逐级检查 reparse point；绝不跟随 junction/symlink 遍历或删除。
  7. 删除前用 `FILE_FLAG_OPEN_REPARSE_POINT` + `FILE_FLAG_BACKUP_SEMANTICS` 打开目标，取最终路径再次校验；路径变化则跳过，降低 TOCTOU 风险。
  8. 已知 file ID 位于受保护根下的硬链接文件跳过。
- 规则测试（强制门槛）：每条规则必须有 `positive`（应命中）与 `redline`（绝不能命中）fixture；`red_lines.rs` 遍历所有内置规则对受保护路径断言零命中；`proptest` 覆盖路径规范化边界。新增规则没有两类测试则 CI 失败。
- 测试时通过可注入的 `RuleContext { env, fixture_root }` 解析 `%APPDATA%` 等变量，绝不在真实用户目录跑测试。

### 8.3 v1 内置规则包（按风险）

**规则准入制度**：每条规则必须记录 `verified_at`、验证用的 Windows/应用版本、官方路径来源；未在真实安装环境核验的第三方应用规则不得进入安装包（宁缺毋滥）。每条规则必须有 positive + redline fixture。

**安全类（默认勾选；只包含可自动重建、无用户数据的缓存）**

| ID | 名称 | 目标与条件 | 管理员 | 恢复性 |
|---|---|---|---|---|
| sys.user-temp | 用户临时文件 | `%TEMP%`、`%LOCALAPPDATA%\Temp`，>1 天 | 否 | 自动重建 |
| sys.thumbnails | 缩略图/图标缓存 | `thumbcache_*.db`、`iconcache_*.db` | 否 | 自动重建 |
| sys.shader-cache | 着色器缓存 | D3DSCache、NVIDIA DXCache、AMD DxCache | 否 | 自动重建 |
| browser.edge-cache | Edge 缓存 | 仅 Cache/Code Cache/GPUCache；硬排除登录态/历史/书签/Service Worker | 否 | 自动重建 |
| browser.chrome-cache | Chrome 缓存 | 同上 | 否 | 自动重建 |
| browser.firefox-cache | Firefox 缓存 | 仅 `cache2` | 否 | 自动重建 |
| dev.npm-cache | npm 缓存 | `_cacache`，>7 天 | 否 | 重新下载 |
| dev.pnpm-store | pnpm store | store 目录，>7 天 | 否 | 重新下载 |
| dev.yarn-cache | Yarn 缓存 | cache 目录 | 否 | 重新下载 |
| dev.pip-cache | pip 缓存 | `%LOCALAPPDATA%\pip\Cache` | 否 | 重新下载 |
| dev.uv-cache | uv 缓存 | `%LOCALAPPDATA%\uv\cache` | 否 | 重新下载 |
| dev.go-cache | Go 构建缓存 | `%LOCALAPPDATA%\go-build` | 否 | 自动重建 |
| apps.teams-cache | Teams 缓存 | `Cache`/`Code Cache`/`GPUCache` | 否 | 自动重建 |
| apps.discord-cache | Discord 缓存 | 同上 | 否 | 自动重建 |
| apps.slack-cache | Slack 缓存 | 同上 | 否 | 自动重建 |
| apps.vscode-cache | VS Code 缓存 | `Cache`/`CachedData`/`GPUCache`/`logs` | 否 | 自动重建 |

**谨慎 / 危险类（默认不勾选，需明确确认；管理员动作走 helper）**

| ID | 名称 | 动作与条件 | 风险 | 管理员 | 恢复性 |
|---|---|---|---|---|---|
| sys.windows-temp | Windows 临时文件 | `%WINDIR%\Temp`，>1 天；部分安装程序会占用 | 谨慎 | 是 | 自动重建 |
| sys.wer | 错误报告 | 用户 + ProgramData WER 队列/归档，>7 天 | 谨慎 | 部分 | 自动重建 |
| sys.crash-dumps | 崩溃转储 | `%LOCALAPPDATA%\CrashDumps`、`%WINDIR%\Minidump`，>30 天 | 谨慎 | 部分 | 自动重建 |
| sys.icon-cache | 图标缓存 | Explorer 锁定时跳过 | 谨慎 | 否 | 自动重建 |
| sys.cbs-wu-logs | CBS/WU 旧日志 | 只删历史 `CbsPersist_*.cab`/旧 `.etl`，绝不删当前日志 | 谨慎 | 是 | 自动重建 |
| sys.delivery-opt | 传递优化缓存 | 固定 `Delete-DeliveryOptimizationCache -Force` | 谨慎 | 是 | 自动重建 |
| dev.cargo-cache | Cargo 缓存 | registry cache/src，>30 天 | 谨慎 | 否 | 重新下载 |
| dev.gradle-cache | Gradle 缓存 | `.gradle\caches`，>30 天 | 谨慎 | 否 | 重新下载 |
| dev.nuget-cache | NuGet 缓存 | `.nuget\packages`，>30 天 | 谨慎 | 否 | 重新下载 |
| apps.wechat-cache | 微信缓存 | 仅 3.x/4.x 已核验 Cache/temp；硬排除 Msg/File/Image/Video/DB | 谨慎 | 否 | 自动重建 |
| apps.qq-cache | QQ 缓存 | 仅已核验缓存目录；硬排除聊天/文件/图片 | 谨慎 | 否 | 自动重建 |
| apps.dingtalk-cache | 钉钉缓存 | 仅已核验缓存目录；硬排除媒体/接收文件/DB | 谨慎 | 否 | 自动重建 |
| apps.wps-cache | WPS 缓存 | 仅已核验缓存目录；硬排除文档/备份/userdata | 谨慎 | 否 | 自动重建 |
| sys.recycle-bin | 回收站 | 清空回收站 | 谨慎 | 否 | 不可恢复（提示） |
| downloads.old-installers | 旧安装包 | Downloads 顶层 exe/msi/zip/iso/7z/rar，>90 天且 >100MB | 谨慎 | 否 | 回收站 |
| sys.wu-download | Windows 更新缓存 | 停 wuauserv/bits/dosvc → `SoftwareDistribution\Download` → 仅恢复原本运行的服务 | 谨慎 | 是 | 重新下载 |
| sys.dism-analyze | 组件存储分析 | `DISM AnalyzeComponentStore`（只读） | 安全 | 是 | — |
| sys.dism-cleanup | 组件存储清理 | `StartComponentCleanup`，**不用 /ResetBase** | 谨慎 | 是 | 不可逆（系统受支持） |
| sys.windows-old | 旧系统文件 | 仅引导至系统存储设置/cleanmgr，绝不直接删 | 谨慎 | 引导 | 不可恢复（失去回滚） |
| sys.hibernation | 休眠文件 | `powercfg /hibernate off` | 危险 | 是 | 可重新开启 |
| sys.restore-points | 还原点限额 | 仅引导至系统保护 UI，v1 不自动执行 | 危险 | 引导 | 不可恢复 |

**浏览器硬排除（任何浏览器规则都不得匹配）**：`Cookies`、`Login Data`、`Web Data`、`History`、`Bookmarks`、`Local Storage`、`Session Storage`、`IndexedDB`、`Service Worker\Database`、`Preferences`、`Local State`；Firefox `cookies.sqlite`、`places.sqlite`、`logins.json`、`key4.db`、`cert9.db`。

**国产应用硬排除**：微信/QQ/钉钉的聊天数据库、图片、视频、语音、已接收文件、备份；WPS 文档/备份/用户数据。规则只在检测到对应数据目录时展示；应用版本路径不一致时隐藏规则，绝不放宽为通配删除。

**大文件查找**（独立功能，不是规则）：全盘 Top-N（默认 >500MB），展示路径/大小/年龄/扩展名，支持打开目录与回收站删除，默认不批量勾选。

**永不触碰（全局红线，编译进二进制，用户不可编辑）**
- `%WINDIR%\WinSxS`（手删）、`%WINDIR%\Installer`、`%WINDIR%\WindowsApps`、`System Volume Information`、`C:\Recovery`、`C:\Boot`、EFI 分区
- `pagefile.sys`、`hiberfil.sys`、`swapfile.sys`（休眠文件只允许通过固定 `powercfg` 动作处理）
- 注册表 hive、凭据库、SSH 密钥、浏览器密码/Cookie/历史/书签
- 用户文档/桌面/图片/下载根目录本身；仅允许规则明确匹配的后代
- 微信/QQ/钉钉聊天数据库、媒体、已接收文件
- OneDrive / 坚果云等同步目录

**二期候选**：Ollama / HuggingFace / LM Studio 模型缓存（需识别“未使用模型”，默认不勾选）、重复文件、WinSxS 硬链接感知分析。

## 9. 清理执行与安全

### 9.1 流程
`扫描 → Finding 列表 → 用户勾选 → CleanupPlan 预览（字节数/文件数/风险）→ 二次确认 → 执行 → 结果报告 → 可恢复/审计`

- **Dry-run 永远可用**；默认先预览。
- **前端不能直接传路径执行删除**：IPC 只接受 `finding_id` / `rule_id` + 选中索引，Rust 端从已扫描结果重新解析并做路径/红线校验；杜绝被 XSS 或恶意 IPC 调用删除任意文件。
- 默认文件动作是**回收站**（`IFileOperation` + `FOF_ALLOWUNDO`），可撤销；永久删除只允许用于可重建缓存，且需要单独的“永久删除缓存文件”开关 + 二次确认；清空回收站始终明确不可逆。
- 锁定文件：逐项收集 `ERROR_SHARING_VIOLATION`/拒绝访问，500ms 后重试一次；不强制、不结束进程、不使用重启删除；提示用户关闭占用程序。
- 隔离区：同卷移动到 `%LOCALAPPDATA%\cc-disksage\quarantine\<plan-id>\`，写 manifest；可恢复但**不释放空间**（直到清理），UI 必须说明；默认关闭。
- 回收站撤销：捕获操作前后回收站状态，映射 `$I`/`$R` 到原路径，优先调用 Shell Restore；映射失败则提供“打开回收站”。中英文 Windows 都要测试；自动恢复是 best-effort。
- 审计：SQLite + append-only JSONL；字段含 plan/rule/action/paths/bytes/result/elevation/restore metadata/app version/timestamp；用 blake3 哈希链防篡改；保留 180 天，日志 10MB × 5 轮转。
- 撤销：回收站走系统还原；隔离区应用内恢复；永久删除项在报告中明确标注不可恢复。

### 9.2 提权 helper（清理 + 管理员扫描）
- 主程序 `asInvoker`；`disksage-helper.exe` 随安装包分发，提供两个受限任务：`scan` 与 `clean`。
- Job 文件：`%LOCALAPPDATA%\cc-disksage\jobs\<uuid>.json`；helper 写 `<uuid>.result.json` + `<uuid>.progress.jsonl`；UI 每 500ms 轮询；处理 UAC 取消（`ERROR_CANCELLED`）。
- helper **绝不信任 job**：重新加载编译进二进制的规则库，校验规则 ID/版本，重新执行路径安全与红线校验，确认目标在规则声明的根目录内，拒绝 reparse point，校验 nonce + 10 分钟有效期 + 调用者 SID，并确认自身确实已提权；未知动作、用户规则、错误 SID 一律拒绝。
- helper 只执行固定 `ActionSpec`，不接受任意可执行文件、参数或路径。
- **管理员全盘扫描**：helper 扫描 `C:\Windows`、`Program Files`、其他用户目录等无权限区域，将快照写入 `%LOCALAPPDATA%\cc-disksage\snapshots\`（SQLite 同步索引），未提权的 UI 只读展示；避免整个 UI 长期以管理员运行。
- helper 逐项写结果；UI 写规范审计，helper 另写自己的日志；job 文件 24 小时后清理。
- 用户拒绝 UAC：管理员规则标记为「需要管理员」，其余规则正常执行；普通用户级扫描不受影响。

### 9.3 系统动作实现
- Windows Update 缓存：SCM API 查询/停止/恢复服务；记录每个服务原本是否运行，**只恢复原本运行的服务**；失败时 `finally` 恢复；恢复日志用于崩溃后下次启动检测。
- DISM：先 `AnalyzeComponentStore` 展示可回收量，再 `StartComponentCleanup`；退出码 3010 提示需要重启，绝不强制重启；不使用 `/ResetBase`。
- 传递优化：固定 `Delete-DeliveryOptimizationCache -Force`（或等价 API），不手删未知路径。
- 回收站：直接用 `windows` crate 实现 `IFileOperation`（逐项错误 + 回收站映射），不依赖 `trash` crate。
- Windows.old：只打开系统存储设置/cleanmgr，不直接删除。
- 恢复点：危险/管理员动作前可选 `Checkpoint-Computer`；失败则只允许安全项。
- 大文件：只做查找与展示，删除走回收站，默认不批量勾选。
- 取消语义：DISM/服务操作不可强杀；开始前 UI 警告，取消只对文件遍历/删除生效，服务状态始终恢复。

### 9.4 安全不变量（CI 与 code review 强制）
1. 删除绝不超出规则验证过的根目录。
2. 红线永远优先于 include；计划阶段与 helper 执行前各校验一次。
3. 永不跟随或穿过 reparse point/junction 删除。
4. 卷根、Windows 根、用户目录根、桌面/文档/下载根永远不是删除目标。
5. 规则与用户输入都不能提供原始命令；只允许固定 `ActionSpec`。
6. 服务停止/启动有状态保护；失败必恢复。
7. DISM 先分析、无 `/ResetBase`、不强制重启。
8. 永久删除与清空回收站需要独立二次确认。
9. LLM 不能新增 Finding、动作或勾选，只能标注。
10. 锁定文件跳过，绝不强制。
11. v1 没有“一键自动清理”；每次执行都有预览。
12. 所有展示给用户的数字都来自扫描器/helper，并标注来源。

## 10. LLM 证据链与摘要

### 10.1 接入
- OpenAI 兼容：`POST {base_url}/chat/completions`。预置：DeepSeek `https://api.deepseek.com/v1`、通义千问 `https://dashscope.aliyuncs.com/compatible-mode/v1`、智谱 `https://open.bigmodel.cn/api/paas/v4`、Moonshot `https://api.moonshot.cn/v1`、OpenAI `https://api.openai.com/v1`，并支持自定义 base URL/model。除 loopback 外强制 HTTPS。
- Ollama 也提供 `http://127.0.0.1:11434/v1` 兼容端点，因此无需单独适配器即可接本地离线模型（首版不内置模型管理）。
- API Key 存 Windows Credential Manager（`CredWriteW`/`CredReadW` 薄封装，`CRED_TYPE_GENERIC`），用 `SecretString` 持有并在 drop 时清零；永不返回前端、永不写 SQLite/日志/崩溃报告。`llm_get_config` 只返回 `has_key: bool`。
- 连接 10s / 总 60s 超时；最多 2 并发、1 req/s；输出上限 2048 tokens；仅 429/5xx 指数退避重试；失败自动回退本地规则说明。LLM 完全可选，默认关闭。

### 10.2 发送内容（严格最小化）
- 只发聚合元数据：规则 id/版本、类别、路径模板、分配/逻辑大小、文件数、时间范围、扩展名分布、应用检测结果、证据 ID 列表、最多 10 个样本文件名（每个截断 120 字符，默认 0 个）。
- 脱敏：`C:\Users\<name>` 等用户目录 → `%USERPROFILE%`；清除 SID、邮箱样式字符串、控制字符。
- **绝不发送文件内容、注册表值、Cookie、聊天数据库、完整用户路径**（默认脱敏）。
- Payload 示例（可在发送前完整预览/复制）：
```json
{
  "task": "assess_finding",
  "finding": {
    "rule_id": "apps.wechat-cache",
    "rule_version": 1,
    "risk_hint": "caution",
    "size_provenance": "scanner",
    "path_template": "%APPDATA%\\Tencent\\WeChat\\<id>\\FileStorage\\Cache",
    "allocated_bytes": 1331439862,
    "logical_bytes": 1400000000,
    "file_count": 8321,
    "oldest_mtime": "2026-08-01",
    "newest_mtime": "2026-10-17",
    "extensions": {".dat": 78, ".jpg": 12, ".png": 8, "other": 2},
    "evidence": ["E1", "E2", "E3", "E4", "E5"],
    "app_detected": { "name": "WeChat", "version": "4.x" }
  },
  "output_schema": "assessment-v1"
}
```
- 首次发送前展示完整 payload 预览，用户确认；可设置“对该 provider 记住同意”。

### 10.3 输出结构（强约束）
```json
{
  "schema_version": 1,
  "finding_id": "f_...",
  "verdict": "cleanable|review|do_not_clean",
  "confidence": "low|medium|high",
  "summary": "一句话结论（不含数字）",
  "reasoning": [
    { "evidence_ids": ["E1", "E2"], "text": "这是微信缓存目录" }
  ],
  "cited_evidence_ids": ["E1", "E2"],
  "consequence": "不会删除聊天记录",
  "caveats": ["删除后首次打开微信会稍慢"]
}
```
- 用 JSON Schema 校验，未知字段拒绝；**schema 中没有任何大小/数量/百分比字段**。
- `cited_evidence_ids` 与每步 `evidence_ids` 必须存在于本地证据白名单；未知 ID 直接拒绝。
- 正文中提取所有数字 token，必须出现在被引用的证据值或很窄的白名单（版本号）中；发现编造数字即拒绝。
- 失败重试一次并附校验错误；仍失败则回退本地模板，不做无限重试。
- LLM 不能输出可执行命令、路径或删除指令；清理动作永远来自规则引擎。
- **提示注入防护**：文件名/路径是 untrusted 数据，系统提示明确要求忽略数据中的任何指令；JSON 数据分区放置；剥离控制字符；限制样本文件名长度与数量；LLM 输出按纯文本渲染（React 默认转义，禁用 `dangerouslySetInnerHTML`）。

### 10.4 证据面板（UI 结构）
1. **头部**：标题、类别、风险等级、可回收大小、`size_provenance`（扫描器实测/提权 helper/估算）
2. **为什么可以清理**：规则来源 + 扫描事实 + 应用检测 + LLM 推断（引用 chip 可点击跳转）
3. **清理后会发生什么**：自动重建 / 重新下载 / 应用影响 / 是否需重新登录
4. **不会动什么**：明确列出排除项（Cookie、密码、历史、聊天媒体、文档等）
5. **证据列表**：`E1..En`，每项带来源徽章（规则/扫描器/应用检测/AI）
6. **AI 分析**（可选）：verdict、confidence、逐步推理、caveats、“AI 生成，仅供参考”声明
7. **继续追问**：单问题输入框；离线或未配置 LLM 时禁用
8. **操作**：清理、排除、打开所在目录

### 10.5 可清理摘要与追问
- 本地确定性模板始终可用：按风险分组的可回收总量、Top 10 项目、建议方案；数字由 Rust 生成并注入。
- LLM 仅润色非数字叙述；数字校验失败自动回退模板；摘要中明确区分「规则判定」与「AI 推断」。
- 追问：一次一个问题，范围仅限当前 Finding 的证据与已有评估；最多 512 tokens；不保留完整聊天历史、不启用工具调用；输出按纯文本渲染。
- LLM 调用审计：provider/host/model/payload hash/证据 ID/token/延迟/HTTP 状态/重试次数/结果；默认不存原始 payload 与 Key。

**摘要示例（数字全部来自扫描器）**：
> 本次扫描 C: 发现可回收 **12.4 GB**：
> - **安全项 3.1 GB**：浏览器缓存 1.2 GB、开发缓存 1.1 GB、临时文件 0.8 GB
> - **需确认 9.3 GB**：Windows 更新缓存 5.2 GB、旧安装包 4.1 GB
> 建议先清理安全项。Windows 更新缓存需要管理员权限，清理后系统更新会重新下载；旧安装包可在回收站恢复。
> AI 说明：……（仅基于以上元数据，不包含文件内容）

**单项证据示例（微信缓存）**：
| 证据 | 内容 | 来源 |
|---|---|---|
| 规则命中 | apps.wechat-cache v1（2026-10 核验） | 内置规则 |
| 应用检测 | 检测到微信 4.x 数据目录 | 文件系统 + 注册表 |
| 路径 | `%APPDATA%\Tencent\WeChat\<id>\FileStorage\Cache` | 规则模板 |
| 占用 | 1.24 GB / 8,321 个文件 | 扫描器 |
| 时间 | 最近修改 2026-10-17 | 扫描器 |
| 类型 | .dat 78%, .jpg 12%, .png 8% | 扫描器 |
| 红线校验 | 未命中 Msg/File/Image/Video/DB | 规则测试 |
| 影响 | 删除后自动重建，不影响聊天记录 | 规则说明 |
| AI 判断 | cleanable（confidence: high），引用上述证据 | LLM（可选） |

## 11. 前端与交互

- 技术：React 19 + TypeScript + Vite + Tailwind + zustand + ECharts + lucide-react + i18next（zh-CN 默认）。
- 默认聚焦系统盘 C:，支持切换其他 NTFS 卷；空间分析可对任意卷运行，主界面显示所有卷的容量/剩余空间。
- 字体使用系统字体栈（Microsoft YaHei UI / Segoe UI），不下载 CDN 字体；图标打包进 bundle。
- 页面：
  - **仪表盘**：C 盘环形图、剩余空间、快速扫描、可安全清理量、最近清理、趋势
  - **扫描/空间分析**：treemap + 文件夹树 + 大文件表，点击下钻
  - **清理建议**：按类别分组，风险徽章，勾选，详情抽屉
  - **证据抽屉**：见 9.4
  - **历史**：扫描快照、清理记录、导出 CSV/JSON
  - **设置**：LLM（base URL/model/key/测试连接）、隐私（脱敏/样本文件名/payload 预览）、排除项、删除方式、语言、关于
- 可视化：ECharts `echarts/core` 仅引入 Treemap/Pie/Bar/Gauge + CanvasRenderer；treemap ≤10,000 tiles，支持面包屑/下钻；文件夹树与大文件表虚拟化（`@tanstack/react-virtual`）。
- 所有大小/数量带来源徽章：扫描器实测 / helper / 估算；WinSxS 等硬链接目录标注“逻辑大小不可直接相加”。
- CSP：`default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src ipc: http://ipc.localhost; object-src 'none'; frame-ancestors 'none'`。LLM 请求只在 Rust 侧；开发模式额外放行 Vite websocket。
- IPC 类型：Rust 侧 `#[derive(TS)]`（ts-rs）生成 TS DTO，`npm run gen-types`；命令只接收 ID（finding/rule/snapshot），不接收任意删除路径；`open_path` 校验路径属于已知快照或应用数据目录。
- 核心命令：`get_volumes`、`start_scan`、`cancel_scan`、`get_scan_summary`、`get_treemap`、`get_children`、`get_large_files`、`get_findings`、`get_finding`、`create_plan`、`preview_plan`、`execute_plan`、`cancel_plan`、`get_audit`、`llm_*`、`open_recycle_bin`。
- 进度用 Tauri `Channel<T>` 推送；生命周期事件 `scan://done`、`cleanup://done`。
- 无障碍：Radix primitives、键盘导航、焦点环、ARIA 标签、文字 + 图标风险徽章（不只靠颜色）、≥4.5:1 对比度、`prefers-reduced-motion`、图表提供表格替代。
- 首次启动引导：说明安全模型（不确认不删除、红线目录不碰）、LLM 完全可选、检测管理员权限/WebView2；引导用户先做一次只读扫描。
- 无遥测；更新检查默认关闭（离线优先），二期提供可选的手动检查/离线更新包。

## 12. 存储

SQLite（`rusqlite` bundled，无需外部依赖）：
```sql
scans(id, volume, started_at, finished_at, total_bytes, allocated_bytes, free_bytes,
      file_count, dir_count, error_count, node_limit_hit, tree_blob, meta_json)
findings(scan_id, finding_id, rule_id, rule_version, path, allocated_bytes, logical_bytes,
         files, risk, size_provenance, evidence_json)
cleanups(id, scan_id, created_at, plan_json, result_json)
audit(id, plan_id, ts, event, payload_json, elevation, restore_json, prev_hash, hash)
llm_cache(key, model, prompt_version, response_json, created_at)
settings(key, value_json)
```
- WAL 模式；全盘快照保留最近 5 次、快速扫描保留最近 20 次；`tree_blob` 用 `postcard + zstd` 压缩；审计用 blake3 哈希链。
- **设置项**：LLM（base_url / model / temperature / max_tokens / timeout / 是否脱敏 / 是否发送样本文件名 / consent 版本）；清理（回收站或永久、隔离天数、危险项二次确认）；扫描（排除路径、是否跟随 reparse point 固定 false、大文件阈值）；UI（主题、语言）。API Key 只存 Credential Manager，SQLite 仅存引用。
- 设置也可用 `tauri-plugin-store`，但审计/快照统一放 SQLite。

## 13. 安装包与离线策略

- Tauri 2 bundle：NSIS x64；默认 `installMode = currentUser`（安装不需要管理员，装到 `%LOCALAPPDATA%`）；另提供 `perMachine` 企业构建（装到 Program Files，需管理员）。不用 `both`，避免普通用户安装时也被要求 UAC。
- **两种安装包**：
  - `cc-disksage-offline-setup.exe`：`webviewInstallMode: offlineInstaller`，内置 WebView2 Evergreen 离线安装器（约 +127MB），可在无网 Win10/11 安装；设置 `minimumWebview2Version`。
  - `cc-disksage-lite-setup.exe`：`webviewInstallMode: skip`，适用于已装 WebView2 的 Win11/企业环境；启动时检测缺失并给出提示；体积约 15–25MB。
  - **不推荐 `fixedRuntime`**：约 +180MB，且把 WebView2 安全更新绑定到应用发版；仅在严格无管理员/固定运行时环境评估。
- 注意：offline 包在缺少 WebView2 的机器上，Evergreen 离线安装器可能需要一次管理员授权；若目标环境严格禁止管理员，再评估企业变体。
- 侧车二进制命名遵循 Tauri 规则：`binaries/disksage-helper-x86_64-pc-windows-msvc.exe`，通过 `bundle.externalBin` 打包。
- 安装包内包含：编译后的 `cc-disksage.exe`、`disksage-helper.exe`、前端静态资源、内置规则、SQLite（静态链接）、图标；NSIS 语言包含简体中文与英文。
- **静态 CRT**：使用 `STATIC_VCRUNTIME=true` / `-C target-feature=+crt-static`，用 `dumpbin /dependents` 确认只依赖 Windows 系统 DLL；若仍出现 `WebView2Loader.dll`，随包携带；不要用 GNU target。
- **不包含 Rust 工具链，也不需要**；Rust std 已静态链接，安装包携带的是编译后的原生二进制。
- 安装包体积预算：offline 约 180–280MB（WebView2 安装器版本 + NSIS LZMA 压缩影响），出包后实测并写入发布说明。
- **构建机离线**：`scripts/vendor-offline.ps1` 执行 `cargo vendor` + npm 缓存打包，并预下载 NSIS 与 WebView2 离线安装器；这样内部构建机在无外网时也能出包。目标用户机器完全不需要 Rust、Node 或任何构建工具。
- 代码签名：首版未签名会有 SmartScreen 提示，文档说明；二期接入 EV/OV 证书与离线更新包签名。
- 卸载默认保留用户数据，提供“同时删除设置/历史”选项。
- **支持矩阵**：主支持 Windows 11 x64；Windows 10 22H2 尽力支持（在发布说明中写明 EOL/ESU 状态）；NTFS（MFT 二期仅 NTFS）；WebView2 缺失时由 offline 包安装或提示。
- **文档与合规**：随包提供隐私政策（无遥测、LLM 可选、payload 预览）、EULA、开源许可清单（`cargo-about` + npm license checker）；代码建议 MIT/Apache-2.0；第三方应用规则必须独立核验撰写，不复制 CCleaner 专有规则；如需引入 Winapp2.ini 需遵守 CC-BY-SA-4.0 并署名（v1 不引入）。

## 14. 测试与验收

### 自动化
- `cargo test --workspace` + `cargo clippy -D warnings` + `cargo fmt --check` + `cargo deny` + `cargo audit`
- `npm run test`（Vitest + React Testing Library）、`npm run lint`、`tsc --noEmit`、Playwright（mock Tauri IPC）
- 规则红线测试：每条内置规则必须有 positive/redline fixture，CI 强制；`proptest` 覆盖路径规范化。
- 路径安全测试：`..`、UNC、设备路径、junction/symlink、大小写、长路径、中文路径、硬链接。
- 扫描器：fixture 树聚合正确性、sparse/compressed、junction 循环、权限拒绝、node cap、取消、10 万文件性能。
- 清理：dry-run 零变更、计划校验、回收站/隔离区行为（临时目录）、helper job 拒绝用例、服务状态守护、DISM 输出解析。
- LLM：`wiremock` 返回畸形 JSON/未知证据 ID/编造数字/超时/429 → 重试或回退；脱敏；Key 永不入日志；payload 预览。

### 手工 E2E（Windows 10/11 VM + 真机）
1. 离线安装（断网）→ 启动 → 扫描 C: → 与 Explorer / WizTree 对比（容差 ±2%，硬链接/稀疏文件需解释）
2. 清理安全项 → 验证释放空间、浏览器仍保持登录、微信聊天记录/图片/文件完好
3. 回收站项恢复成功；隔离区恢复成功；永久删除项标注不可恢复
4. 管理员项触发 UAC；UAC 取消后降级执行；服务恢复；DISM 输出正确；3010 重启提示
5. LLM：OpenAI 兼容 mock/真实 Key；查看 payload 预览；断网回退；Ollama `/v1` 本地端点可选验证
6. 锁定文件跳过；中文用户名/含空格路径/超 260 字符路径
7. 干净 Win10（无 WebView2、断网）安装；Win11 安装；升级/卸载；审计与历史正确
8. 性能目标：快速扫描 <5s；全盘 100 万文件 <60s 热 / <90s 冷；内存 <500MB；treemap ≤10k tiles <1s

## 15. 里程碑（单人估算）

| 阶段 | 内容 | 工期 |
|---|---|---|
| M0 Bootstrap | 安装 Rust(MSVC) + VS C++ Build Tools + Windows SDK + Tauri CLI；workspace/Tauri/React 骨架；CI、图标、i18n、项目 `.claude` 文档 | 2–3 天 |
| M1 Core/Storage | 领域类型、错误、ID、SQLite 迁移、设置、日志、卷枚举、Credential Manager 封装 | 3–5 天 |
| M2 Scanner | 快速扫描、并行全盘 walker、arena/node cap、进度/取消、大文件、直方图、压缩快照、基准测试 | 7–9 天 |
| M3 Rules | TOML schema/loader/matcher、path safety、固定 ActionSpec、v1 规则包、证据生成、红线/positive 测试、用户规则 | 7–9 天 |
| M4 Cleanup | 计划/预览、IFileOperation 回收站、隔离、审计/撤销、提权 helper、服务/DISM/引导动作、安全测试 | 7–9 天 |
| M5 Frontend | 仪表盘、treemap/文件夹树、清理页、证据抽屉、计划弹窗、历史、设置、主题/i18n/无障碍 | 9–11 天 |
| M6 LLM | OpenAI 兼容客户端、Key、预置、同意/payload 预览、prompt/schema/校验、摘要、追问、缓存与回退 | 6–8 天 |
| M7 Hardening | Win10/11 VM E2E、offline/lite 安装包、portable zip、卸载、依赖检查、性能调优、安全审查、发布清单 | 6–8 天 |

总计约 **47–62 人日，单人全职 8–10 周**。AI 辅助可压缩编码时间，但规则真实环境核验、VM 测试、离线安装与安全审查是硬成本，不宜低估。二期：MFT 秒扫 1–2 周、重复文件 1 周、定时任务 1 周、签名/自动更新 1 周。

## 16. 风险与对策

| 风险 | 对策 |
|---|---|
| 杀软误报 / SmartScreen | 保守规则、用户确认、二期代码签名、提交厂商白名单 |
| 锁定文件删除失败 | 跳过并报告；提示关闭应用；二期 Restart Manager |
| 中文路径 / 长路径 | 全程 `PathBuf`/`OsString`，显示层再转 UTF-8；`\\?\` 前缀 |
| junction / symlink 循环 | 默认不跟随 reparse point；测试覆盖 |
| 硬链接 / 稀疏文件导致大小误判 | 回收量按分配大小；>1MiB 文件按 file ID 去重；WinSxS 不参与清理并单独说明 |
| UAC 摩擦 | 只对管理员规则批量提权一次；用户拒绝后降级执行 |
| WebView2 离线体积 | 提供 offline 与 lite 两种安装包 |
| 国内构建网络（crates.io/GitHub/NSIS） | 预置镜像配置（rsproxy/国内 npm 镜像）、预下载 NSIS 与 WebView2 安装器、提供完全离线构建套件 |
| LLM 幻觉 | schema 无数字字段 + evidence ID 白名单 + 正文数字 token 校验 + 本地模板回退 |
| API Key 泄露 | Credential Manager；日志/前端永不出现 |
| 服务停止/恢复失败 | SCM API + finally 恢复 + 明确错误 |
| DISM 需要重启 | 只提示，不自动重启；处理退出码 3010 |
| 产品名冲突 | `DiskSage` 已被其他开源项目使用；公开发布前改名或确认商标 |
| Windows 10 EOL | 主支持 Win11；Win10 22H2 尽力支持，发布说明写明 EOL/ESU 状态 |
| VC++ 运行库缺失 | 静态 CRT 构建 + `dumpbin /dependents` 检查；必要时随包 VC++ Redist |
| 测试误删真实文件 | 全量 fixture + 临时目录；危险规则仅手工 VM 验证 |

## 17. 验证方式（批准后执行）

1. `cargo build --workspace` 与 `npm run build` 通过；`cargo test`、`npm run test` 全绿。
2. 用 `scripts/build-installer.ps1` 生成 offline/lite 两个 NSIS 安装包。
3. 在 Windows 11 真机（当前机器）进行只读扫描与 dry-run，核对 top 目录/大文件。
4. 在临时 fixture 目录执行清理 E2E，断言命中/未命中与红线清单。
5. 断网安装 offline 包，验证启动、扫描、规则清理、审计、LLM 关闭时完整可用。
6. 用 mock OpenAI 兼容服务验证 payload、schema 校验与失败回退；再用真实 Key 做一次人工验证。

### 首版 Definition of Done
- [ ] 断网环境用 offline 安装包在干净的 Windows 10/11 上安装并启动成功
- [ ] 快速扫描 <5s；全盘 100 万文件 <90s；内存 <500MB；可取消
- [ ] 所有内置规则通过 positive + redline fixture；红线目录零命中
- [ ] 清理后浏览器仍保持登录、微信聊天记录/图片/文件完好、系统可正常更新
- [ ] 回收站/隔离区可恢复；审计日志与历史记录完整可导出
- [ ] 管理员规则触发一次 UAC，服务恢复，DISM 需重启时有提示
- [ ] LLM payload 可预览、默认脱敏、无文件内容；schema/引用/数字校验失败可回退
- [ ] 无遥测；API Key 仅存 Credential Manager；日志不含 Key
- [ ] 卸载/重装/升级不破坏用户设置与历史（或明确提示）

## 18. 批准后的第 0 步

- 创建 `D:\work\claude-projects\cc-disksage\.claude\plans\disk-cleaner-implementation.md`（本方案副本）与 `.claude/CLAUDE.md`（项目约定、规则规范、禁止事项）。
- 初始化 git 仓库与 `.gitignore`。
- 安装构建工具链（需一次 UAC/管理员）：
  - `winget install --id Rustlang.Rustup -e`
  - `winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--wait --quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"`
  - `rustup default stable-x86_64-pc-windows-msvc`
  - `npm install` 安装 `@tauri-apps/cli`，验证 `cargo --version`、`npm run tauri -- --version`。

## 19. 参考来源

- Tauri 2 Windows 安装包与 WebView2 离线模式：https://v2.tauri.app/distribute/windows-installer/
- Tauri 2 配置参考（webviewInstallMode / nsis / externalBin）：https://v2.tauri.app/reference/config/
- Tauri `WebviewInstallMode` API：https://docs.rs/tauri-utils/latest/tauri_utils/config/enum.WebviewInstallMode.html
- Tauri 静态 CRT / 独立二进制讨论：https://github.com/orgs/tauri-apps/discussions/3048
- Tauri 2.12.3 release：https://v2.tauri.app/release/tauri/v2.12.3/
- Rust 1.99.0 release（2026-10）：https://blog.rust-lang.org/2026/10/01/Rust-1.99.0/
- BleachBit CleanerML 声明式规则：https://docs.bleachbit.org/cml/cleanerml.html
- Winapp2.ini 社区规则库：https://github.com/MoscaDotTo/Winapp2
- Pinkbin（Tauri + Rust + MFT + 元数据外发 AI + 回收站撤销）：https://github.com/cccyd2003-qwq/pinkbin
- Pinkbin 架构说明：https://github.com/cccyd2003-qwq/pinkbin/blob/main/docs/ARCHITECTURE.md
- DiskSage（Ollama 本地 AI 清理，只读扫描/逐项确认）：https://dev.to/simp_bonk/my-friends-laptop-is-full-of-junk-nobody-dares-delete-so-i-built-him-a-local-ai-that-explains-9bh
- WizTree / TreeSize / SpaceSniffer 对比：https://univik.com/blog/best-disk-space-analyzer-windows.html
- Microsoft DISM 组件存储清理：https://learn.microsoft.com/en-us/windows-hardware/manufacturing/dism
- Microsoft PC Manager：https://pcmanager.microsoft.com/
- TreeSize Free：https://www.jam-software.com/treesize_free
- SpaceSniffer：https://www.uderzo.it/main_products/space_sniffer/
- Dism++：https://github.com/Chuyu-Team/Dism-Multi-language
