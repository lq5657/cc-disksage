# 隐私与数据边界

CC DiskSage 默认完全本地运行，无遥测。LLM 是可选功能，默认关闭。

## 本地数据

数据目录 `<数据目录>`：

- 程序安装在非系统盘（如 D 盘）：`D:\CCDiskSageData\<用户名>`
- 程序安装在系统盘：`%LOCALAPPDATA%\cc-disksage`
- 可用 `CC_DISKSAGE_DATA_DIR` 环境变量覆盖

| 数据 | 位置 |
|---|---|
| LLM 设置 | `<数据目录>\llm.json`（不含 API Key） |
| 自动清理设置 | `<数据目录>\auto-clean.json` |
| API Key | Windows 凭据管理器，service `cc-disksage.llm`（不随数据目录迁移） |
| 审计日志 | `<数据目录>\logs\audit.jsonl` |
| 自动清理日志 | `<数据目录>\logs\auto-clean.jsonl`（仅计数、状态与说明，不含路径） |
| 持久化数据库 | `<数据目录>\disksage.db`（最近 20 次扫描摘要、重复文件结果、扫描历史） |
| 隔离区（未来） | `<数据目录>\quarantine\` |
| 用户规则 | `<数据目录>\rules\*.toml` |
| WebView2 数据 | `<数据目录>\EBWebView\` |

旧版 C 盘数据（`%LOCALAPPDATA%\cc-disksage`、`%APPDATA%\cc-disksage` 和
`%LOCALAPPDATA%\com.ccdisksage.app`）会在首次以 D 盘安装版启动时迁移到新目录；复制并
校验成功后才删除旧文件，失败则保留旧目录并回退。

SQLite 保存扫描摘要（目录树前 3 层、Top 大文件）和重复文件分组/路径，不保存文件内容、
注册表值、Cookie 或聊天内容。保留最近 20 次扫描与 20 次重复扫描，可在设置外手动删除
`disksage.db` 清空。删除数据库前请先退出应用，避免 WAL 文件被占用。

MFT 秒扫由本机提权 helper 读取 NTFS 元数据，重复文件扫描只在本机读取文件内容计算
BLAKE3 哈希；两者都不会上传任何数据。代码签名只处理本地构建产物，不涉及用户数据。

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
