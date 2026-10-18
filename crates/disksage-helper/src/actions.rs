use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use disksage_clean::fs_safety::{is_reparse_point, looks_locked};
use disksage_core::path_safety::{is_forbidden_delete_target, is_system_red_line};
use disksage_core::{CleanupResult, EnvMap};
use walkdir::WalkDir;

const SERVICE_ALLOWLIST: &[&str] = &["wuauserv", "bits", "dosvc"];
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn is_elevated() -> bool {
    // SID S-1-16-12288 is the High mandatory level; S-1-16-16384 is System.
    if let Ok(output) = Command::new("whoami").args(["/groups"]).output() {
        let text = String::from_utf8_lossy(&output.stdout);
        if text.contains("S-1-16-12288") || text.contains("S-1-16-16384") {
            return true;
        }
    }

    // Fallback for unusual environments where whoami is unavailable.
    Command::new("net")
        .args(["session"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn command(program: &str) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

fn powershell(script: &str) -> Result<Output, String> {
    command("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
        .map_err(|error| format!("failed to run PowerShell: {error}"))
}

fn output_summary(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let text = if stdout.trim().is_empty() {
        stderr.trim().to_string()
    } else {
        stdout.trim().to_string()
    };
    let mut summary = text.chars().take(2000).collect::<String>();
    if text.chars().count() > 2000 {
        summary.push('…');
    }
    summary
}

fn validate_service(service: &str) -> Result<(), String> {
    if SERVICE_ALLOWLIST
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(service))
    {
        Ok(())
    } else {
        Err(format!("service is not allowlisted: {service}"))
    }
}

fn service_is_running(service: &str) -> Result<bool, String> {
    validate_service(service)?;
    let script = format!(
        "(Get-Service -Name '{}' -ErrorAction Stop).Status -eq 'Running'",
        service
    );
    let output = powershell(&script)?;
    if !output.status.success() {
        return Err(output_summary(&output));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim()
        .eq_ignore_ascii_case("True"))
}

fn stop_service(service: &str) -> Result<(), String> {
    validate_service(service)?;
    let script = format!("Stop-Service -Name '{}' -Force -ErrorAction Stop", service);
    let output = powershell(&script)?;
    if !output.status.success() {
        return Err(output_summary(&output));
    }

    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if !service_is_running(service)? {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Err(format!("service {service} did not stop within 20 seconds"))
}

fn start_service(service: &str) -> Result<(), String> {
    validate_service(service)?;
    let script = format!("Start-Service -Name '{}' -ErrorAction Stop", service);
    let output = powershell(&script)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(output_summary(&output))
    }
}

/// Stop allowlisted services, clear fixed cache directories, then restore the
/// services that were originally running.
pub fn service_cleanup(
    services: &[String],
    paths: &[PathBuf],
    env: &EnvMap,
    result: &mut CleanupResult,
) {
    let mut running = Vec::new();

    for service in services {
        if let Err(error) = validate_service(service) {
            result.notes.push(error);
            continue;
        }
        match service_is_running(service) {
            Ok(true) => {
                running.push(service.clone());
                match stop_service(service) {
                    Ok(()) => result.notes.push(format!("已停止服务 {service}")),
                    Err(error) => {
                        result
                            .notes
                            .push(format!("停止服务 {service} 失败：{error}"));
                    }
                }
            }
            Ok(false) => {}
            Err(error) => result
                .notes
                .push(format!("查询服务 {service} 状态失败：{error}")),
        }
    }

    for path in paths {
        if is_system_red_line(path, env) || is_forbidden_delete_target(path, env) {
            result
                .notes
                .push(format!("拒绝清理受保护路径：{}", path.display()));
            continue;
        }
        if !path.exists() {
            result
                .notes
                .push(format!("路径不存在，跳过：{}", path.display()));
            continue;
        }
        match clear_directory_contents(path, result) {
            Ok((files, bytes)) => {
                result.deleted = result.deleted.saturating_add(files);
                result.freed_bytes = result.freed_bytes.saturating_add(bytes);
                result.notes.push(format!(
                    "已清理 {} 个文件，共 {}，路径 {}",
                    files,
                    format_bytes(bytes),
                    path.display()
                ));
            }
            Err(error) => result
                .notes
                .push(format!("清理 {} 失败：{error}", path.display())),
        }
    }

    for service in running.iter().rev() {
        match start_service(service) {
            Ok(()) => result.notes.push(format!("已恢复服务 {service}")),
            Err(error) => result
                .notes
                .push(format!("恢复服务 {service} 失败：{error}")),
        }
    }
}

/// Delete the contents of a directory (never the directory itself). Reparse
/// points are skipped, locked files are reported, and the root is preserved.
pub fn clear_directory_contents(
    root: &Path,
    result: &mut CleanupResult,
) -> Result<(u64, u64), String> {
    let metadata = root
        .symlink_metadata()
        .map_err(|error| format!("cannot read {}: {error}", root.display()))?;
    if is_reparse_point(&metadata) {
        return Err("root is a reparse point".into());
    }
    if metadata.is_file() {
        let bytes = metadata.len();
        fs::remove_file(root).map_err(|error| error.to_string())?;
        return Ok((1, bytes));
    }
    if !metadata.is_dir() {
        return Err("root is neither a file nor a directory".into());
    }

    let mut files = 0u64;
    let mut bytes = 0u64;
    let walker = WalkDir::new(root)
        .min_depth(1)
        .follow_links(false)
        .contents_first(true);

    for entry in walker.into_iter().filter_map(Result::ok) {
        let path = entry.path();
        let metadata = match path.symlink_metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if is_reparse_point(&metadata) {
            continue;
        }
        if metadata.is_file() {
            match fs::remove_file(path) {
                Ok(()) => {
                    files = files.saturating_add(1);
                    bytes = bytes.saturating_add(metadata.len());
                }
                Err(error) => {
                    if looks_locked(&error.to_string()) {
                        result.skipped_locked.push(path.to_path_buf());
                    } else {
                        result.failed.push((path.to_path_buf(), error.to_string()));
                    }
                }
            }
        } else if metadata.is_dir() {
            let _ = fs::remove_dir(path);
        }
    }

    Ok((files, bytes))
}

pub fn dism_analyze(result: &mut CleanupResult) {
    run_dism(
        &["/Online", "/Cleanup-Image", "/AnalyzeComponentStore"],
        "DISM 组件存储分析",
        result,
    );
}

pub fn dism_cleanup(result: &mut CleanupResult) {
    run_dism(
        &["/Online", "/Cleanup-Image", "/StartComponentCleanup"],
        "DISM 组件存储清理",
        result,
    );
}

fn run_dism(args: &[&str], label: &str, result: &mut CleanupResult) {
    match command("dism.exe").args(args).output() {
        Ok(output) => {
            let code = output.status.code().unwrap_or(-1);
            let summary = output_summary(&output);
            result.notes.push(format!(
                "{label} 退出码 {code}{}：{summary}",
                if code == 3010 {
                    "（需要重启）"
                } else {
                    ""
                }
            ));
        }
        Err(error) => result.notes.push(format!("{label} 启动失败：{error}")),
    }
}

pub fn hibernate_off(result: &mut CleanupResult) {
    let hiberfil = std::env::var_os("SYSTEMDRIVE")
        .map(PathBuf::from)
        .map(|drive| drive.join("hiberfil.sys"));
    let before = hiberfil
        .as_ref()
        .and_then(|path| fs::metadata(path).ok())
        .map(|metadata| metadata.len());

    match command("powercfg.exe").args(["/hibernate", "off"]).output() {
        Ok(output) if output.status.success() => {
            let still_exists = hiberfil.as_ref().is_some_and(|path| path.exists());
            if let Some(bytes) = before
                && !still_exists
            {
                result.deleted = result.deleted.saturating_add(1);
                result.freed_bytes = result.freed_bytes.saturating_add(bytes);
                result
                    .notes
                    .push(format!("已关闭休眠并释放 {}", format_bytes(bytes)));
            }
            if still_exists {
                result
                    .notes
                    .push("休眠已关闭，但 hiberfil.sys 仍然存在（可能需要重启）".into());
            } else {
                result
                    .notes
                    .push("休眠已关闭（powercfg /hibernate off）".into());
            }
        }
        Ok(output) => result
            .notes
            .push(format!("powercfg 失败：{}", output_summary(&output))),
        Err(error) => result.notes.push(format!("powercfg 启动失败：{error}")),
    }
}

pub fn empty_recycle_bin(result: &mut CleanupResult) {
    match powershell("Clear-RecycleBin -Force -ErrorAction Stop") {
        Ok(output) if output.status.success() => {
            result.notes.push("回收站已清空".into());
        }
        Ok(output) => result
            .notes
            .push(format!("清空回收站失败：{}", output_summary(&output))),
        Err(error) => result.notes.push(format!("清空回收站失败：{error}")),
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes == 0 {
        return "0 B".into();
    }
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clears_directory_contents_but_keeps_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(root.join("a.tmp"), b"12345").unwrap();
        let nested = root.join("nested");
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(nested.join("b.tmp"), b"123").unwrap();

        let mut result = CleanupResult::default();
        let (files, bytes) = clear_directory_contents(root, &mut result).unwrap();

        assert_eq!(files, 2);
        assert_eq!(bytes, 8);
        assert!(root.exists());
        assert!(!root.join("a.tmp").exists());
        assert!(!nested.exists());
    }

    #[test]
    fn rejects_unknown_services() {
        let mut result = CleanupResult::default();
        service_cleanup(&["evil-service".into()], &[], &EnvMap::new(), &mut result);
        assert!(result.notes.iter().any(|note| note.contains("allowlisted")));
    }
}
