use std::process::{Command, Output};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn powershell(script: &str) -> std::io::Result<Output> {
    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.output()
}

/// Empty the Windows Recycle Bin through the system API exposed by PowerShell.
/// This is irreversible and is only called after an explicit plan confirmation.
pub fn empty_recycle_bin() -> Result<(), String> {
    match powershell("Clear-RecycleBin -Force -ErrorAction Stop") {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let message = if stderr.is_empty() { stdout } else { stderr };
            Err(if message.is_empty() {
                "Clear-RecycleBin failed".into()
            } else {
                message.chars().take(500).collect()
            })
        }
        Err(error) => Err(format!("failed to run PowerShell: {error}")),
    }
}
