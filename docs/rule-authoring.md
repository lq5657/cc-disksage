# 规则编写指南

内置规则位于 `rules/builtin/*.toml`，由 `disksage-rules` 在编译时内嵌。
用户规则位于数据目录的 `rules\*.toml`（D 盘安装为 `D:\CCDiskSageData\<用户名>\rules\`，
系统盘安装为 `%LOCALAPPDATA%\cc-disksage\rules\`），只能使用
`trash` / `delete` / `quarantine`，且不得要求管理员权限、不得默认勾选、
不得标记为 `danger`。

## 最小示例

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

detect = { path_exists_any = ["%TEMP%"] }

targets = [
  { root = "%TEMP%", include_globs = ["**/*"], exclude_globs = ["**/*.lock", "**/~$*"], max_depth = 32 }
]

explain = {
  what_zh = "应用程序产生的临时文件。",
  what_en = "Temporary files created by applications.",
  impact_zh = "正在被占用的文件会被跳过。",
  impact_en = "Files in use are skipped.",
  exclude_notes_zh = "不会删除文档或浏览器数据。",
  exclude_notes_en = "Documents and browser data are never touched.",
  source = "Microsoft Windows TEMP convention"
}

action = { type = "trash" }
```

## 字段约束

| 字段 | 说明 |
|---|---|
| `root` | 必须是字面路径（允许 `%TEMP%` 等白名单环境变量），**不允许通配符** |
| `include_globs` / `exclude_globs` | 相对 root 的 `/` 分隔 glob；exclude 优先 |
| `max_depth` | 1–128；root 自身不算删除目标 |
| `follow_reparse_points` | 必须为 false（当前版本拒绝 true） |
| `min_age_days` | 只处理早于该天数的文件；0 表示不限制 |
| `risk` | `safe` / `caution` / `danger` |
| `default_selected` | 只有 `safe` 可以为 true |
| `recoverability` | `regenerable` / `recycle_bin` / `quarantine` / `irreversible` / `user_data` |
| `action` | 固定枚举，不接受任意命令 |

支持的 action：

- `trash`、`delete`、`quarantine`
- `empty_recycle_bin`
- `service_cleanup { services, paths }`（管理员）
- `dism_analyze`、`dism_start_component_cleanup`（管理员）
- `guided { guide }`（打开系统设置）
- `hibernate_off`、`create_restore_point`（管理员/危险）

当前快速扫描与执行器只启用了 `trash` / `delete`；管理员动作在提权 helper 接入后开放。

## 环境变量白名单

`%TEMP%`、`%TMP%`、`%LOCALAPPDATA%`、`%APPDATA%`、`%USERPROFILE%`、
`%PROGRAMDATA%`、`%WINDIR%`、`%SYSTEMDRIVE%`

## 安全测试（强制）

每条规则必须同时有：

- `rules/tests/<rule_id>/positive/`：应当命中的文件
- `rules/tests/<rule_id>/redline/`：绝不能命中的文件（聊天数据库、Cookie、密码、文档等）

当前测试骨架：

- `crates/disksage-rules/tests/quick_scan.rs`：schema、glob、红线等通用行为。
- `crates/disksage-rules/tests/builtin_fixtures.rs`：按规则 ID 跑
  `rules/tests/<rule_id>/{positive,redline}`。新规则必须把 ID 加入该文件的
  `NEW_RULES` 列表，并提交两类 fixture。

fixture 目录约定：`positive/` 与 `redline/` 下直接用环境变量名作为顶层目录，例如：

```text
rules/tests/browser.edge_gpu_cache/positive/LOCALAPPDATA/Microsoft/Edge/User Data/Default/DawnCache/sample.bin
rules/tests/browser.edge_gpu_cache/redline/LOCALAPPDATA/Microsoft/Edge/User Data/Default/Cookies
```

测试会把每个环境变量指向临时目录，把这些子树复制进去，再执行规则匹配；
所有 fixture 文件的 mtime 会被设为一年前，避免被 `min_age_days` 过滤。

新增规则时需要确认：

1. 解析与 schema 校验通过
2. 正向 fixture 至少命中一个文件
3. 红线 fixture 零命中
4. 规则不允许把 root 本身作为删除目标
5. 真实机器上记录 `verified_at` / `verified_on` 与实测大小

## 管理员规则

`requires_admin = true` 的规则由 `disksage-helper.exe` 在 UAC 授权后执行。快速扫描会把这些规则
作为“管理员项”展示，大小标记为估算（可能为 0）；helper 执行时会重新扫描。

管理员规则可以使用：

- `trash` / `delete`：删除规则匹配的文件
- `service_cleanup`：只允许 `wuauserv`、`bits`、`dosvc`，先停服务、清理固定路径、再恢复原本运行的服务
- `dism_analyze`：只读 `DISM /Online /Cleanup-Image /AnalyzeComponentStore`
- `dism_start_component_cleanup`：`StartComponentCleanup`，**不使用 `/ResetBase`**
- `hibernate_off`：`powercfg /hibernate off`

对于没有文件目标的动作规则（DISM/休眠/引导/回收站），`targets` 使用一个存在但永不匹配的目录
（如 `%WINDIR%\Logs` + `__disksage_never_match__`），以便规则能进入发现列表。

`Guided` 动作只打开系统界面（磁盘清理、系统保护、系统属性），本工具不直接删除。
`empty_recycle_bin` 通过系统 API 清空回收站，清空后不可恢复。

## 自动清理规则准入

自动清理只加载编译进二进制的内置规则，不加载用户规则，并且只选择同时满足以下条件的项：

- `risk = "safe"`
- `default_selected = true`
- `recoverability = "regenerable"`
- `requires_admin = false`
- 动作为 `trash` 或 `delete`

因此把规则改为 `caution`、取消默认勾选或标记管理员，都会自动将其排除在定时清理之外。
自动模式默认仍使用回收站；只有用户显式开启永久删除才会直接释放空间。

## 第三方应用规则

- 必须记录 `verified_at`、`verified_on` 与路径来源。
- 未在真实安装环境核验的应用规则不得进入安装包。
- 应用版本路径不一致时隐藏规则，**不要用更宽的通配符兜底**。
- 浏览器规则硬排除 Cookie、Login Data、History、Bookmarks、Service Worker、Local Storage、IndexedDB。
- 微信/QQ/钉钉规则硬排除聊天数据库、媒体、已接收文件；WPS 硬排除文档/备份。
