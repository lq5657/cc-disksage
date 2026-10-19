//! Elevated helper launch.
//!
//! PowerShell stays the primary path because it works in the vast majority of
//! environments. A native `ShellExecuteExW("runas")` fallback is used only when
//! PowerShell cannot be created or fails before the helper is launched. A
//! user-cancelled UAC prompt is never retried, so the user only sees one
//! consent dialog.

use std::path::Path;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use std::path::PathBuf;
#[cfg(windows)]
use std::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const HELPER_LAUNCHED_MARKER: &str = "CC_DISKSAGE_HELPER_LAUNCHED";

#[derive(Debug, Clone, PartialEq, Eq)]
enum LaunchFailure {
    /// The user dismissed the UAC prompt. Do not try another mechanism.
    UserCancelled,
    /// PowerShell or the native elevation API is not available.
    Unavailable(String),
    /// The mechanism failed before the helper was launched.
    LaunchFailed(String),
    /// The helper actually ran and returned a non-zero exit code. Never retry:
    /// the caller reads the helper result file and reports the real outcome.
    HelperExited(String),
}

pub fn launch_elevated_helper(helper: &Path, job: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        launch_windows(helper, job)
    }
    #[cfg(not(windows))]
    {
        let _ = (helper, job);
        Err("elevated cleanup is only supported on Windows".into())
    }
}

#[cfg(windows)]
fn launch_windows(helper: &Path, job: &Path) -> Result<(), String> {
    let elevated = is_process_elevated();
    log_launch(
        "start",
        helper,
        job,
        if elevated {
            "attempt=direct"
        } else {
            "attempt=powershell"
        },
    );

    if let Err(message) = preflight_helper(helper, job) {
        log_launch("failure", helper, job, &message);
        return Err(message);
    }

    if elevated {
        let result = launch_direct(helper, job);
        match &result {
            Ok(()) => log_launch("success", helper, job, "mechanism=direct"),
            Err(message) => log_launch(
                "failure",
                helper,
                job,
                &format!("mechanism=direct; {message}"),
            ),
        }
        return result;
    }

    let mut errors = Vec::new();

    match launch_via_powershell(helper, job) {
        Ok(()) => {
            log_launch("success", helper, job, "mechanism=powershell");
            return Ok(());
        }
        Err(LaunchFailure::UserCancelled) => {
            log_launch("cancelled", helper, job, "mechanism=powershell");
            return Err("用户取消了管理员授权".into());
        }
        Err(LaunchFailure::HelperExited(message)) => {
            log_launch(
                "helper_exited",
                helper,
                job,
                &format!("mechanism=powershell; {message}"),
            );
            return Err(message);
        }
        Err(LaunchFailure::Unavailable(message)) => {
            errors.push(format!("PowerShell 不可用：{message}"));
        }
        Err(LaunchFailure::LaunchFailed(message)) => {
            errors.push(format!("PowerShell 提权失败：{message}"));
        }
    }

    log_launch(
        "fallback",
        helper,
        job,
        &format!("mechanism=native; {}", errors.join("；")),
    );

    match launch_via_shellexecute(helper, job) {
        Ok(()) => {
            log_launch("success", helper, job, "mechanism=native");
            return Ok(());
        }
        Err(LaunchFailure::UserCancelled) => {
            log_launch("cancelled", helper, job, "mechanism=native");
            return Err("用户取消了管理员授权".into());
        }
        Err(LaunchFailure::HelperExited(message)) => {
            log_launch(
                "helper_exited",
                helper,
                job,
                &format!("mechanism=native; {message}"),
            );
            return Err(message);
        }
        Err(LaunchFailure::Unavailable(message)) => {
            errors.push(format!("原生提权不可用：{message}"));
        }
        Err(LaunchFailure::LaunchFailed(message)) => {
            errors.push(format!("原生提权失败：{message}"));
        }
    }

    let message = format!("无法启动提权 helper：{}", errors.join("；"));
    log_launch("failure", helper, job, &message);
    Err(message)
}

#[cfg(windows)]
fn preflight_helper(helper: &Path, job: &Path) -> Result<(), String> {
    let metadata = std::fs::metadata(helper)
        .map_err(|error| format!("helper 不可访问：{}（{error}）", helper.display()))?;
    if !metadata.is_file() {
        return Err(format!("helper 不是文件：{}", helper.display()));
    }
    if metadata.len() == 0 {
        return Err(format!(
            "helper 文件为空，可能被安全软件隔离：{}",
            helper.display()
        ));
    }
    if !job.is_file() {
        return Err(format!("job 文件不存在：{}", job.display()));
    }
    Ok(())
}

#[cfg(windows)]
fn log_launch(event: &str, helper: &Path, job: &Path, detail: &str) {
    use std::io::Write;

    let directory = disksage_core::paths::logs_dir();
    if std::fs::create_dir_all(&directory).is_err() {
        return;
    }
    let record = serde_json::json!({
        "at": chrono::Utc::now().to_rfc3339(),
        "event": event,
        "helper": helper.display().to_string(),
        "job": job.display().to_string(),
        "elevated": is_process_elevated(),
        "detail": detail,
    });
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("helper-launch.jsonl"))
    {
        let _ = writeln!(file, "{record}");
    }
}

#[cfg(windows)]
fn launch_direct(helper: &Path, job: &Path) -> Result<(), String> {
    let status = Command::new(helper)
        .arg("--job")
        .arg(job)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|error| format!("无法直接启动 helper：{error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("helper 退出码 {}", status.code().unwrap_or(-1)))
    }
}

#[cfg(windows)]
fn launch_via_powershell(helper: &Path, job: &Path) -> Result<(), LaunchFailure> {
    let powershell = powershell_path().ok_or_else(|| {
        LaunchFailure::Unavailable(
            "未找到 %SystemRoot%\\System32\\WindowsPowerShell\\v1.0\\powershell.exe".into(),
        )
    })?;
    let script = format!(
        "try {{ \
           $p = Start-Process -FilePath {helper} -ArgumentList '--job',{job} -Verb RunAs -PassThru -WindowStyle Hidden -ErrorAction Stop; \
           if (-not $p) {{ [Console]::Error.WriteLine('Start-Process did not return a process'); exit 1 }}; \
           [Console]::Out.WriteLine('{marker}'); \
           $p.WaitForExit(); \
           exit $p.ExitCode \
         }} catch {{ \
           $e = $_.Exception; \
           $inner = $e.InnerException; \
           $native = 0; \
           if ($inner -and $inner.NativeErrorCode) {{ $native = $inner.NativeErrorCode }}; \
           $hr = 0; \
           if ($e.HResult) {{ $hr = [uint32]$e.HResult }}; \
           if ($native -eq 1223 -or $hr -eq 0x800704C7 -or $e.Message -match 'canceled|cancelled|取消') {{ exit 1223 }}; \
           [Console]::Error.WriteLine($e.Message); \
           exit 1 \
         }}",
        helper = quote_path(helper),
        job = quote_path(job),
        marker = HELPER_LAUNCHED_MARKER,
    );

    let output = Command::new(&powershell)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .current_dir(
            powershell
                .parent()
                .unwrap_or_else(|| Path::new(r"C:\Windows\System32")),
        )
        .output()
        .map_err(|error| {
            LaunchFailure::Unavailable(format!("无法创建 PowerShell 子进程：{error}"))
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let result_exists = job.with_extension("result.json").exists();
    classify_powershell_result(output.status.code(), &stdout, &stderr, result_exists)
}

#[cfg(windows)]
fn classify_powershell_result(
    code: Option<i32>,
    stdout: &str,
    stderr: &str,
    result_exists: bool,
) -> Result<(), LaunchFailure> {
    if code == Some(0) {
        return Ok(());
    }

    let launched = result_exists || stdout.contains(HELPER_LAUNCHED_MARKER);
    if !launched && (code == Some(1223) || is_cancel_message(stderr)) {
        return Err(LaunchFailure::UserCancelled);
    }

    let detail = if stderr.is_empty() {
        format!("PowerShell 退出码 {}", code.unwrap_or(-1))
    } else {
        format!("PowerShell 退出码 {}：{stderr}", code.unwrap_or(-1))
    };

    if launched {
        Err(LaunchFailure::HelperExited(detail))
    } else {
        Err(LaunchFailure::LaunchFailed(detail))
    }
}

#[cfg(windows)]
fn is_cancel_message(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("canceled") || lower.contains("cancelled") || message.contains("取消")
}

#[cfg(windows)]
fn powershell_path() -> Option<PathBuf> {
    let windows = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let candidate = windows
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    candidate.is_file().then_some(candidate)
}

#[cfg(windows)]
fn quote_path(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "''"))
}

#[cfg(windows)]
fn launch_via_shellexecute(helper: &Path, job: &Path) -> Result<(), LaunchFailure> {
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
    use windows::Win32::UI::Shell::{
        SEE_MASK_FLAG_NO_UI, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    use windows::core::PCWSTR;

    let verb = wide("runas");
    let file = wide(&helper.to_string_lossy());
    let parameters = wide(&format!("--job \"{}\"", job.display()));
    let directory = wide(&std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()));

    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_NO_UI,
        lpVerb: PCWSTR::from_raw(verb.as_ptr()),
        lpFile: PCWSTR::from_raw(file.as_ptr()),
        lpParameters: PCWSTR::from_raw(parameters.as_ptr()),
        lpDirectory: PCWSTR::from_raw(directory.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };

    unsafe { ShellExecuteExW(&mut info) }.map_err(|error| {
        if error.code().0 as u32 == 0x8007_04C7 {
            LaunchFailure::UserCancelled
        } else {
            LaunchFailure::LaunchFailed(format!("ShellExecuteExW: {error}"))
        }
    })?;

    let process = info.hProcess;
    if process.is_invalid() {
        return Err(LaunchFailure::LaunchFailed(
            "ShellExecuteExW 未返回进程句柄".into(),
        ));
    }

    let wait = unsafe { WaitForSingleObject(process, INFINITE) };
    if wait != WAIT_OBJECT_0 {
        let error = windows::core::Error::from_thread();
        let _ = unsafe { CloseHandle(process) };
        return Err(LaunchFailure::HelperExited(format!(
            "等待 helper 退出失败：{error}"
        )));
    }

    let mut exit_code = 0u32;
    let result = unsafe { GetExitCodeProcess(process, &mut exit_code) };
    let _ = unsafe { CloseHandle(process) };
    result
        .map_err(|error| LaunchFailure::HelperExited(format!("读取 helper 退出码失败：{error}")))?;

    if exit_code == 0 {
        Ok(())
    } else {
        Err(LaunchFailure::HelperExited(format!(
            "helper 退出码 {exit_code}"
        )))
    }
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn is_process_elevated() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut token = HANDLE::default();
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }

        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        let result = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut core::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        let _ = CloseHandle(token);

        result.is_ok() && elevation.TokenIsElevated != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn recognizes_cancel_messages() {
        assert!(is_cancel_message("The operation was canceled by the user."));
        assert!(is_cancel_message("操作已被用户取消。"));
        assert!(!is_cancel_message("Access is denied."));
    }

    #[cfg(windows)]
    #[test]
    fn classifies_powershell_outcomes() {
        assert!(matches!(
            classify_powershell_result(Some(0), HELPER_LAUNCHED_MARKER, "", false),
            Ok(())
        ));
        assert!(matches!(
            classify_powershell_result(Some(1223), "", "canceled", false),
            Err(LaunchFailure::UserCancelled)
        ));
        assert!(matches!(
            classify_powershell_result(Some(1), "", "Access is denied.", false),
            Err(LaunchFailure::LaunchFailed(_))
        ));
        assert!(matches!(
            classify_powershell_result(Some(1), HELPER_LAUNCHED_MARKER, "", false),
            Err(LaunchFailure::HelperExited(_))
        ));
        assert!(matches!(
            classify_powershell_result(Some(1), "", "", true),
            Err(LaunchFailure::HelperExited(_))
        ));
    }

    #[cfg(windows)]
    #[test]
    fn elevation_check_is_available() {
        let _ = is_process_elevated();
    }
}
