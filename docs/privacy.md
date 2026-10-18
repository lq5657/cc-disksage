# 隐私与数据边界

CC DiskSage 默认完全本地运行，无遥测。LLM 是可选功能，默认关闭。

## 本地数据

| 数据 | 位置 |
|---|---|
| 设置 | `%APPDATA%\cc-disksage\llm.json`（不含 API Key） |
| 自动清理设置 | `%APPDATA%\cc-disksage\auto-clean.json` |
| API Key | Windows 凭据管理器，service `cc-disksage.llm` |
| 审计日志 | `%LOCALAPPDATA%\cc-disksage\logs\audit.jsonl` |
| 自动清理日志 | `%LOCALAPPDATA%\cc-disksage\logs\auto-clean.jsonl`（仅计数、状态与说明，不含路径） |
| 隔离区（未来） | `%LOCALAPPDATA%\cc-disksage\quarantine\` |
| 用户规则 | `%LOCALAPPDATA%\cc-disksage\rules\*.toml` |

扫描快照目前保存在进程内存；持久化到 SQLite 属于后续里程碑。

## 发送给 LLM 的内容

只有在你点击「AI 分析」、查看完整 payload 并勾选同意后才会发送：

- finding ID、规则 ID/版本、类别、风险提示
- 路径模板（默认脱敏为 `%USERPROFILE%` 等）
- 实测分配/逻辑大小、文件数
- 最早/最近修改日期
- 扩展名分布
- 本地证据 ID 与证据描述
- 应用检测结果（如有）
- 可选：最多 10 个样本文件名（默认关闭，每个截断到 120 字符）

## 永远不会发送

- 文件内容
- 注册表值
- Cookie、密码、历史、书签
- 聊天数据库、图片、视频、已接收文件
- 完整用户目录路径（默认脱敏）
- API Key（仅用于 HTTP Authorization，不进入日志）

## LLM 输出约束

- 必须通过严格 JSON schema（未知字段拒绝）
- 只能引用本地提供的 evidence ID
- 正文禁止出现数字；数字统一由扫描器注入
- 禁止输出路径、命令、删除指令或新清理项
- 校验失败自动回退到本地规则说明

## 审计

每次清理写入本地 JSONL：计划 ID、规则、动作、路径、字节、结果、时间、是否提权。
自动清理另写 `auto-clean.jsonl`，只记录时间、状态、释放字节、删除/失败数量与说明，不记录文件路径。
LLM 调用审计（provider/model/payload hash/token）属于后续里程碑；当前不会记录 payload 或 Key。
