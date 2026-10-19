use std::process::{Command, Output};

#[cfg(windows)]
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
#[cfg(windows)]
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
#[cfg(windows)]
use windows::Win32::UI::Shell::{
    SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHEmptyRecycleBinW,
};
#[cfg(windows)]
use windows::core::PCWSTR;

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

/// Empty the Windows Recycle Bin. PowerShell is the primary path; if it is
/// unavailable or fails, fall back to the native Shell API. This is
/// irreversible and is only called after an explicit plan confirmation.
pub fn empty_recycle_bin() -> Result<(), String> {
    let powershell_error = match powershell("Clear-RecycleBin -Force -ErrorAction Stop") {
        Ok(output) if output.status.success() => return Ok(()),
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let message = if stderr.is_empty() { stdout } else { stderr };
            if message.is_empty() {
                "Clear-RecycleBin failed".to_string()
            } else {
                format!(
                    "Clear-RecycleBin failed: {}",
                    message.chars().take(500).collect::<String>()
                )
            }
        }
        Err(error) => format!("failed to run PowerShell: {error}"),
    };

    #[cfg(windows)]
    {
        match empty_recycle_bin_native() {
            Ok(()) => Ok(()),
            Err(native_error) => Err(format!(
                "{powershell_error}; native SHEmptyRecycleBinW failed: {native_error}"
            )),
        }
    }
    #[cfg(not(windows))]
    {
        Err(powershell_error)
    }
}

#[cfg(windows)]
fn empty_recycle_bin_native() -> Result<(), String> {
    let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let initialized = hr.is_ok();
    if !initialized && hr != RPC_E_CHANGED_MODE {
        return Err(format!(
            "CoInitializeEx failed: HRESULT 0x{:08X}",
            hr.0 as u32
        ));
    }

    let result = unsafe {
        SHEmptyRecycleBinW(
            None,
            PCWSTR::null(),
            SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND,
        )
    }
    .map_err(|error| format!("SHEmptyRecycleBinW: {error}"));

    if initialized {
        unsafe { CoUninitialize() };
    }

    result
}
