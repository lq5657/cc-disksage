# 代码签名

发布包必须使用受信任的 Windows Authenticode 代码签名证书签名。仓库本身不包含证书、
私钥或密码；签名脚本从环境变量读取配置。

## 前置条件

- Windows SDK 中的 `signtool.exe`（脚本会在 PATH 和
  `%ProgramFiles(x86)%\Windows Kits\10\bin` 下查找 x64 版本）。
- 受信任的代码签名证书，二选一：
  - PFX/P12 文件：`CC_DISKSAGE_SIGN_PFX`，可选密码
    `CC_DISKSAGE_SIGN_PFX_PASSWORD`。
  - 已安装到证书存储的证书指纹：`CC_DISKSAGE_SIGN_THUMBPRINT`（适合 EV USB token
    或企业证书存储）。
- 可选：`CC_DISKSAGE_SIGN_TIMESTAMP_URL`，默认
  `http://timestamp.digicert.com`。设置为 `none` 可跳过时间戳，仅用于本地测试。
- 可选：`CC_DISKSAGE_SKIP_VERIFY=1` 跳过签名后的 `signtool verify`，仅用于没有把
  自签名根证书加入受信任存储的本地测试。正式发布不要设置。

## 构建签名安装包

```powershell
$env:CC_DISKSAGE_SIGN_PFX = "C:\secrets\cc-disksage-codesign.pfx"
$env:CC_DISKSAGE_SIGN_PFX_PASSWORD = "..."

# offline 包
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-installer.ps1 -Sign

# lite 包
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-installer.ps1 -Lite -Sign

# 也可以稍后签名已有安装包
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/sign-installers.ps1
```

`-Sign` 会让 Tauri 使用 `src-tauri/tauri.signed.conf.json` 中的 `signCommand`：

1. 签名并验证 sidecar `disksage-helper.exe`
2. 签名并验证主程序 `cc-disksage.exe`（Tauri 会先写入 bundle 信息）
3. 签名并验证最终 NSIS 安装包

脚本调用 `signtool sign /fd SHA256 /tr <timestamp> /td SHA256`，随后执行
`signtool verify /pa /v`。任一步失败都会终止构建，不会静默产出未签名包。

## 验证

```powershell
Get-AuthenticodeSignature .\dist-installers\cc-disksage-0.1.0-offline-setup.exe

# 或
& "$env:ProgramFiles(x86)\Windows Kits\10\bin\10.0.26100.0\x64\signtool.exe" `
  verify /pa /v .\dist-installers\cc-disksage-0.1.0-offline-setup.exe
```

`Status` 应为 `Valid`。未签名或证书链不受信任时不能作为正式发布包。

## CI 与密钥管理

- PFX 和密码只放在 CI secret 或受控密钥库中，不进入仓库、构建日志、安装包或审计日志。
- 构建后立即清理临时 PFX（如果从 secret 临时落盘）。
- EV 证书通常位于硬件 token；用 `CC_DISKSAGE_SIGN_THUMBPRINT` 指定证书指纹，
  并在构建机上安装 token 驱动。
- 轮换证书后需要重新签名并更新发布说明；旧签名不会因为更换证书而失效。
- 代码签名不保证 SmartScreen 立即放行新证书；声誉需要时间积累，EV 证书通常更快。

## 本地自签名测试（不能用于发布）

自签名证书只用于验证签名流水线。把它加入 `Cert:\CurrentUser\Root` 后可以完成
`signtool verify`，但用户机器不会信任它，SmartScreen 仍会提示。

```powershell
$cert = New-SelfSignedCertificate -Type CodeSigningCert `
  -Subject "CN=CC DiskSage Self Test" `
  -CertStoreLocation Cert:\CurrentUser\My `
  -KeyExportPolicy Exportable -NotAfter (Get-Date).AddDays(2)
$pwd = ConvertTo-SecureString "test-only" -Force -AsPlainText
Export-PfxCertificate -Cert $cert -FilePath "$env:TEMP\test.pfx" -Password $pwd

# 仅测试：临时信任根证书
Export-Certificate -Cert $cert -FilePath "$env:TEMP\test.cer"
Import-Certificate -FilePath "$env:TEMP\test.cer" -CertStoreLocation Cert:\CurrentUser\Root

$env:CC_DISKSAGE_SIGN_PFX = "$env:TEMP\test.pfx"
$env:CC_DISKSAGE_SIGN_PFX_PASSWORD = "test-only"
$env:CC_DISKSAGE_SIGN_TIMESTAMP_URL = "none"
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-installer.ps1 -Lite -Sign

# 测试后删除证书和临时文件
Remove-Item "Cert:\CurrentUser\Root\$($cert.Thumbprint)"
Remove-Item "Cert:\CurrentUser\My\$($cert.Thumbprint)"
```
