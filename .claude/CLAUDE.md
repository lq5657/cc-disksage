# CC-DiskSage — 项目约定

Windows 智能磁盘清理工具：Rust + Tauri 2 + React/TypeScript。详细方案见
`.claude/plans/disk-cleaner-implementation.md`。所有计划文档、规则说明、发布清单都放在项目
`.claude/` 下，不写入用户级 `~/.claude/`。

## 不可妥协的安全规则

1. 安全优先于回收量：宁可少清，不可误删。
2. 删除只能来自声明式规则；规则必须有 positive + redline 测试。
3. 永不手删/规则删除：`WinSxS`、`Windows\Installer`、`WindowsApps`、`System Volume Information`、
   `pagefile.sys`、`hiberfil.sys`、`swapfile.sys`、注册表 hive、浏览器凭据、聊天数据库/媒体。
4. 永不跟随 reparse point / junction / symlink 进行遍历或删除。
5. 前端 IPC 只传 finding/rule/snapshot ID，绝不传任意删除路径。
6. 规则只引用固定 `ActionSpec` 枚举；TOML 中不得出现任意命令/参数。
7. 管理员动作只走窄接口 `disksage-helper.exe`；UI 始终 `asInvoker`。
8. 默认删除到回收站；永久删除必须单独开关 + 二次确认。
9. LLM 只做标注，不能新增 Finding/动作/勾选；不接收文件内容；默认脱敏。
10. 所有展示数字来自扫描器/helper，并带 `size_provenance`。
11. 大文件删除必须由用户逐项确认，只进回收站，拒绝 Windows/Program Files/ProgramData；
    AI 只根据元数据给建议，不能新增候选、勾选或删除文件。

## 架构

- `crates/disksage-core`：领域类型、错误、路径安全、证据模型
- `crates/disksage-scan`：卷信息、快速扫描、并行全盘 walker、快照（MFT 二期）
- `crates/disksage-rules`：TOML 规则 schema/loader/matcher、红线
- `crates/disksage-clean`：计划、执行、回收站、隔离、审计
- `crates/disksage-llm`：OpenAI 兼容客户端、脱敏、schema 校验
- `crates/disksage-helper`：提权 helper（scan/clean 固定动作）
- `src-tauri`：Tauri 命令、状态、capabilities
- `src`：React UI；`rules/builtin`：内置规则与 fixture

## 构建与测试

```powershell
npm install
npm run tauri dev          # 开发
cargo test --workspace     # Rust 测试
cargo clippy -- -D warnings
npm run test               # 前端测试
npm run build              # 前端构建
npm run build:helper:release  # 构建并复制提权 helper
npm run sign:installers    # 签名 dist-installers（需证书环境变量）
npm run tauri build        # 安装包（目标）
```

## 规则准入

- 每条规则记录 `verified_at`、`verified_on`、路径来源。
- 必须有 `rules/tests/<rule_id>/positive` 与 `redline` fixture；红线零命中。
- 未在真实 Windows/应用版本核验的第三方应用规则不得进入安装包。
- 新的第三方应用规则先做只读探测与 fixture，再考虑默认勾选；默认风险从 `caution` 起步。

## 当前状态（2026-10-18）

v0.2.0 已发布：https://github.com/lq5657/cc-disksage/releases/tag/v0.2.0

已实现：

- M0 工具链、workspace、Tauri/React 骨架
- M1 卷枚举、全盘并行扫描、Dashboard、treemap 空间分析
- M3 TOML 规则引擎、红线/路径安全、快速扫描、E1–E8 证据链
- M4 计划预览、回收站执行、审计日志、历史页
- M6 OpenAI 兼容 LLM 设置、payload 同意、逐项分析、追问
- 管理员清理：`disksage-helper.exe` + UAC，Windows Temp/WER/Minidump/Delivery Optimization/
  Windows Update 缓存/DISM Analyze+StartComponentCleanup/休眠文件
- 系统引导（Windows.old/还原点/分页文件）与回收站清空
- 定时/低空间自动清理：`--auto-clean` 无窗口模式、Task Scheduler、安全规则过滤、设置页、独立日志
- NTFS MFT 秒扫：UAC helper + `ntfs-reader` 分块读取 `$MFT`、路径重建、目录聚合
- 重复文件：大小 → 快速哈希 → 完整 BLAKE3、硬链接去重、回收站删除（至少保留一个）
- 快速扫描优化：release 冷缓存约 5s、热缓存约 2.7s（此前约 30s）
- 清理执行优化：回收站批量提交、实时进度（文件/字节/失败/当前路径）、永久删除可重建缓存（二次确认）
- 管理员扫描入口：清理建议页「管理员扫描」（UAC helper 只读实测系统项）、仪表盘「管理员全盘扫描」（MFT）
- SQLite 持久化：扫描摘要、重复文件结果、扫描历史，重启恢复，保留最近 20 次
- 数据目录跟随安装盘：D 盘安装使用 `D:\CCDiskSageData\<用户名>`，自动迁移旧 C 盘数据与 WebView2 数据
- 系统托盘：关闭隐藏到托盘、显示/退出菜单；扫描/清理/重复文件/手动自动清理发送 Windows 通知
- 仪表盘可清理摘要与「去清理建议」入口；大文件支持 AI 元数据分析、打开目录、确认后移到回收站
- 代码签名流水线：signtool + PFX/证书指纹 + 时间戳，Tauri `signCommand` 签 helper/主程序/安装包
- NSIS 安装包配置（offline WebView2 / lite / helper 侧车）；卸载钩子删除计划任务

未实现：

- 自动更新

验证基线：

- `cargo test --workspace` 通过（69 项，7 项 ignored）
- `npm run test` / `npm run build` 通过
- 真机 C 盘扫描：71 万文件 / 19.9 万目录 / 131GB / 63 秒
- 回收站删除 API、Windows 凭据管理器 Key 读写已在真机验证
- 计划任务非管理员创建/查询/删除（含空格路径）、`--auto-clean` 未启用/阈值跳过路径已在真机验证
- MFT 聚合/路径/跳过规则单测通过；非管理员 helper 拒绝 MFT 任务
- 真实 MFT 提权扫描（UAC）：652,696 文件 / 204,702 目录 / 122.57 GiB / 5.76s；walker 对比 59.6s
- 重复文件真实内容目录只读扫描通过；硬链接、阈值、跳过 AppData/腾讯数据、取消测试通过
- 重复文件真实删除 fixture：保留一个、另一个进回收站、审计写入
- 管理员 helper 清理（UAC）：Windows Temp/WER/Minidump 删除 233 文件 / 约 160 MiB，3 个占用文件失败；审计 elevation=true
- DISM Analyze（UAC）：3010 需重启；已修复 DISM 隐藏重启提示卡死（/NoRestart + 15 分钟超时）
- 快速扫描 release 冷缓存 4.97s / 热缓存 2.72s，结果一致
- 清理：400 文件回收站批量清理约 1.5s 并有实时进度；永久删除 400 文件即时完成
- 管理员扫描（UAC）：传递优化 5.1GB/55 文件、Windows 更新缓存 799MB/10,956 文件、Windows Temp 505KB/7 文件实测
- 大文件：D 盘 600MB fixture 出现在快照；AI payload 默认掩码文件名为 `<file>` 且不含文件名；
  确认后移到回收站成功，快照持久化移除该条目；仪表盘快速扫描摘要与跳转入口正常
- 0.2.0 安装包已覆盖安装到 `D:\CCDiskSage`：版本 0.2.0、数据仍在 D 盘、仪表盘摘要与大文件操作入口正常
- SQLite 文件数据库跨重启 roundtrip、应用启动建库与表结构验证通过
- 数据迁移：C 盘 `%LOCALAPPDATA%\cc-disksage`、`%APPDATA%\cc-disksage` 和 WebView2 数据已迁移到 `D:\CCDiskSageData\youda`，扫描/重复结果/审计保留
- 最终 D 盘安装包覆盖安装后：主程序/helper 含新数据目录逻辑；C 盘旧数据目录未重建
- 托盘：关闭窗口后进程继续运行；窗口隐藏时快速扫描完成通知正常弹出（含「已隐藏到托盘」提示）
- 代码签名流水线验证通过，但按当前决定暂不用于发布
- NSIS 安装包已完成静默安装 → 启动 → 卸载验证；卸载钩子会移除自动清理计划任务
- 安装包：`dist-installers/`（offline 约 212MB / lite 约 4.9MB，SHA-256 见 `SHA256SUMS.txt`）

待人工验证：

- 自动清理实际到点运行（会清理真实缓存）
- 重启后恢复最近扫描/重复结果、历史页扫描列表的 UI 点击验证
- 系统引导项与重复文件删除 UI 点击流程
- 干净 Windows 10 断网安装（VM）
