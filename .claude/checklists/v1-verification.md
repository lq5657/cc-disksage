# v1 验证清单

## 自动化

- [ ] `cargo test --workspace`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo fmt --check`
- [ ] `npm run test`
- [ ] `npm run build`
- [ ] `cargo test -p disksage-clean --test pipeline trash_action_moves_file_to_recycle_bin -- --ignored`（Windows 真机）
- [ ] `cargo test -p cc-disksage scheduled_task_roundtrip_with_spaced_path -- --ignored`（创建/查询/删除真实计划任务，含空格路径）
- [ ] `cargo test -p disksage-scan scans_real_user_profile_read_only -- --ignored`（只读真实重复扫描）
- [ ] `powershell -File scripts/build-installer.ps1 -Lite -Sign`（需证书，验证签名流水线）
- [ ] `cargo run -p disksage-rules --example quick_scan`（只读，核对真实 C 盘）
- [ ] `cargo run --release -p disksage-scan --example scan_c`（只读，性能基线）

## 手工 E2E（VM/真机）

- [ ] 断网用 offline 安装包安装并启动
- [ ] 安装到 D 盘（`/D=D:\CCDiskSage`）后启动；数据目录为 `D:\CCDiskSageData\<用户名>`，C 盘旧数据目录不再重建
- [x] 旧 C 盘数据一次性迁移：DB/日志/扫描与重复结果/审计/WebView2 到 D 盘，旧目录已清空
- [x] 关闭主窗口隐藏到托盘，进程继续运行；托盘菜单显示/退出可用
- [x] 清理建议页「管理员扫描」触发 UAC，helper 回填管理员项实测大小；取消 UAC 保留普通结果
- [x] 仪表盘「管理员全盘扫描」入口可见（MFT）
- [x] 清理执行显示实时进度（已处理/总数、字节、失败、当前路径）
- [x] 400 文件 fixture 回收站清理约 1.5s；永久删除 400 文件即时完成（二次确认）
- [x] 仪表盘显示上次快速扫描摘要，并可跳转到清理建议
- [x] 大文件 AI payload 预览：默认路径为 `<file>` 掩码、文件名不发送、需勾选同意
- [x] 大文件打开目录入口可见；大文件确认后移到回收站，快照更新并持久化
- [x] 0.2.0 安装包覆盖安装、版本 0.2.0、D 盘数据目录与新增入口验证
- [x] 系统安装目录守卫单元测试通过（Windows/Program Files/ProgramData）
- [ ] UI 对系统安装目录大文件删除的拒绝提示手工验证
- [x] 窗口隐藏到托盘时完成快速扫描，出现 Windows 通知（其他命令共用同一通知函数）
- [x] 首次关闭到托盘时出现「已隐藏到系统托盘」提示
- [ ] 托盘菜单「显示主界面」「退出」手工点击验证
- [ ] 全盘扫描 C:，treemap 可下钻；与 Explorer/WizTree 对比 ±2%
- [x] 快速扫描生成候选；19 项；release 冷缓存 4.97s / 热缓存 2.72s
- [ ] 证据抽屉显示 E1–E8
- [ ] 默认只勾选安全项；谨慎项需手动勾选
- [x] 完成一次扫描后重启应用：仪表盘恢复最近扫描，历史页显示扫描记录
- [x] 完成一次重复扫描后重启应用：重复文件页恢复最近结果
- [ ] 预览计划 → 勾选确认 → 执行 → 回收站可恢复
- [ ] 浏览器仍保持登录；Cookie、历史、书签未变
- [ ] 微信/QQ/WPS 聊天记录、图片、文档未变（如适用）
- [ ] 锁定文件被跳过并报告，不强制删除
- [x] 管理员项触发 UAC；helper 重新扫描；Windows Temp/WER/Minidump 清理实测（233 文件/约 160MiB，3 个占用文件失败）
- [ ] 服务（wuauserv/bits/dosvc）在清理后恢复原本运行状态
- [x] DISM Analyze 只读输出正常；StartComponentCleanup 不使用 /ResetBase（3010/需重启，已加 /NoRestart 与超时）
- [x] helper 在非管理员环境拒绝执行
- [x] 审计日志包含 elevation=true 的管理员记录
- [ ] Windows.old 引导打开 cleanmgr；还原点打开系统保护；分页文件打开系统属性
- [ ] 回收站清空通过系统 API 执行；清空后不可恢复提示正确
- [ ] 系统引导项不会直接删除文件
- [x] 设置页启用自动清理 → 计划任务出现；停用 → 计划任务移除（计划任务创建/删除已验证）
- [x] 自动清理只包含安全/默认/可重建/非管理员项，谨慎/危险项不执行（只读候选核验）
- [x] 剩余空间高于阈值时计划运行跳过，并写 `auto-clean.jsonl`
- [ ] 低于阈值/到点时实际执行一次并验证结果
- [x] 自动清理默认进回收站；永久删除开关首次启用有二次确认
- [x] 卸载后计划任务 `CC DiskSage Auto Clean` 被移除
- [x] MFT helper 提权扫描实际通过（652,696 文件 / 204,702 目录 / 122.57GiB / 5.76s / 0 读取错误）
- [ ] 应用内点击 MFT 秒扫按钮、UAC、快照回读与 treemap 下钻完整 UI 流程
- [ ] 非 NTFS 卷点击 MFT 秒扫被拒绝并提示使用普通扫描；UAC 取消不影响普通扫描
- [ ] 重复文件扫描 UI 只覆盖用户内容目录；AppData/Tencent/node_modules 不出现（扫描核心已验证）
- [x] 重复文件至少保留一个才能删除；删除进回收站可恢复；路径/大小变化后拒绝删除（核心 fixture 验证）
- [x] 重复文件删除 Tauri UI 点击流程与 ID-only IPC（手工删除副本到回收站，审计与 DB 更新正常）
- [ ] 签名安装包验证（当前决定暂缓签名）
- [x] SQLite 文件数据库跨重启 roundtrip、应用启动建库与表结构验证通过
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
