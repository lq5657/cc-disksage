# v1 验证清单

## 自动化

- [ ] `cargo test --workspace`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo fmt --check`
- [ ] `npm run test`
- [ ] `npm run build`
- [ ] `cargo test -p disksage-clean --test pipeline trash_action_moves_file_to_recycle_bin -- --ignored`（Windows 真机）
- [ ] `cargo test -p cc-disksage scheduled_task_roundtrip_with_spaced_path -- --ignored`（创建/查询/删除真实计划任务，含空格路径）
- [ ] `cargo run -p disksage-rules --example quick_scan`（只读，核对真实 C 盘）
- [ ] `cargo run --release -p disksage-scan --example scan_c`（只读，性能基线）

## 手工 E2E（VM/真机）

- [ ] 断网用 offline 安装包安装并启动
- [ ] 全盘扫描 C:，treemap 可下钻；与 Explorer/WizTree 对比 ±2%
- [ ] 快速扫描生成候选；证据抽屉显示 E1–E8
- [ ] 默认只勾选安全项；谨慎项需手动勾选
- [ ] 预览计划 → 勾选确认 → 执行 → 回收站可恢复
- [ ] 浏览器仍保持登录；Cookie、历史、书签未变
- [ ] 微信/QQ/WPS 聊天记录、图片、文档未变（如适用）
- [ ] 锁定文件被跳过并报告，不强制删除
- [ ] 管理员项触发 UAC；helper 重新扫描；Windows Temp / Windows Update 缓存可清理
- [ ] 服务（wuauserv/bits/dosvc）在清理后恢复原本运行状态
- [ ] DISM Analyze 只读输出正常；StartComponentCleanup 不使用 /ResetBase
- [ ] helper 在非管理员环境拒绝执行
- [ ] 审计日志包含 elevation=true 的管理员记录
- [ ] Windows.old 引导打开 cleanmgr；还原点打开系统保护；分页文件打开系统属性
- [ ] 回收站清空通过系统 API 执行；清空后不可恢复提示正确
- [ ] 系统引导项不会直接删除文件
- [ ] 设置页启用自动清理 → 计划任务出现；停用 → 计划任务移除
- [ ] 自动清理只包含安全/默认/可重建/非管理员项，谨慎/危险项不执行
- [ ] 剩余空间高于阈值时计划运行跳过；低于阈值时执行并写 `auto-clean.jsonl`
- [ ] 自动清理默认进回收站；永久删除开关首次启用有二次确认
- [ ] 卸载后计划任务 `CC DiskSage Auto Clean` 被移除
- [ ] 审计日志与历史页记录正确
- [ ] LLM：payload 预览 → 同意 → 分析；无 Key/断网时回退
- [ ] LLM 输出含数字/未知证据 ID 时被拒绝
- [ ] 设置页 Key 只显示“已配置”，日志中无 Key
- [ ] 卸载/重装保留或按提示删除用户数据
- [ ] 无遥测；无 CDN 请求

## 安全红线（发布门禁）

- [ ] 所有内置规则 positive + redline fixture 通过
- [ ] WinSxS / Installer / WindowsApps / System Volume Information 零命中
- [ ] 任何规则都不能把 root 本身作为删除目标
- [ ] reparse point / junction 不被跟随
- [ ] frontend 无法传任意删除路径
- [ ] 永久删除/清空回收站需要二次确认
