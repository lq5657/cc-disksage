//! Portable/data-directory selection and one-time migration.
//!
//! Installed apps on a non-system drive keep their data next to that drive
//! (`<drive>:\CCDiskSageData\<user>`), so uninstalling the program does not
//! remove history. Apps installed on the system drive keep the legacy
//! `%LOCALAPPDATA%\cc-disksage` location.

use std::env;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::OnceLock;

pub const DATA_DIR_ENV: &str = "CC_DISKSAGE_DATA_DIR";

/// Set once by [`prepare_data_root`] so every path helper agrees with the
/// startup decision, including the fallback to the legacy location.
static CHOSEN_ROOT: OnceLock<PathBuf> = OnceLock::new();

pub fn data_root() -> PathBuf {
    if let Some(root) = CHOSEN_ROOT.get() {
        return root.clone();
    }
    selected_root()
}

fn selected_root() -> PathBuf {
    if let Some(custom) = env::var_os(DATA_DIR_ENV) {
        let path = PathBuf::from(custom);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }

    if let Some(drive) = current_exe_drive()
        && !is_system_drive(drive)
    {
        return PathBuf::from(format!("{}:\\CCDiskSageData", drive)).join(user_component());
    }

    legacy_local_root()
}

pub fn legacy_local_root() -> PathBuf {
    app_data_base("LOCALAPPDATA").join("cc-disksage")
}

pub fn legacy_roaming_root() -> PathBuf {
    app_data_base("APPDATA").join("cc-disksage")
}

pub fn legacy_webview_root() -> PathBuf {
    app_data_base("LOCALAPPDATA").join("com.ccdisksage.app")
}

pub fn db_path() -> PathBuf {
    data_root().join("disksage.db")
}

pub fn logs_dir() -> PathBuf {
    data_root().join("logs")
}

pub fn jobs_dir() -> PathBuf {
    data_root().join("jobs")
}

pub fn rules_dir() -> PathBuf {
    data_root().join("rules")
}

pub fn llm_config_path() -> PathBuf {
    data_root().join("llm.json")
}

pub fn auto_clean_config_path() -> PathBuf {
    data_root().join("auto-clean.json")
}

/// Create the selected data root and migrate legacy data when possible.
/// If migration cannot complete safely, keep using the legacy location.
pub fn prepare_data_root() -> PathBuf {
    if let Some(root) = CHOSEN_ROOT.get() {
        return root.clone();
    }

    let root = selected_root();
    let chosen = if root == legacy_local_root() {
        let _ = fs::create_dir_all(&root);
        root
    } else if let Err(error) = fs::create_dir_all(&root) {
        eprintln!(
            "failed to create data directory {}: {error}; using legacy location",
            root.display()
        );
        let legacy = legacy_local_root();
        let _ = fs::create_dir_all(&legacy);
        legacy
    } else {
        match migrate_legacy_data(&root) {
            Ok(()) => root,
            Err(error) => {
                eprintln!(
                    "failed to migrate legacy data to {}: {error}; using legacy location",
                    root.display()
                );
                let legacy = legacy_local_root();
                let _ = fs::create_dir_all(&legacy);
                legacy
            }
        }
    };

    let _ = CHOSEN_ROOT.set(chosen.clone());
    chosen
}

fn app_data_base(variable: &str) -> PathBuf {
    env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
}

fn current_exe_drive() -> Option<char> {
    let exe = env::current_exe().ok()?;
    disk_letter(&exe)
}

fn disk_letter(path: &Path) -> Option<char> {
    for component in path.components() {
        if let Component::Prefix(prefix) = component {
            return match prefix.kind() {
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                    Some(char::from(letter).to_ascii_uppercase())
                }
                _ => None,
            };
        }
    }
    None
}

fn is_system_drive(letter: char) -> bool {
    let system = env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    system
        .chars()
        .next()
        .map(|value| value.to_ascii_uppercase())
        .unwrap_or('C')
        == letter.to_ascii_uppercase()
}

fn user_component() -> String {
    let raw = env::var("USERNAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "default".into());
    let sanitized = raw
        .chars()
        .map(|character| {
            if matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            ) || character.is_control()
            {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "default".into()
    } else {
        sanitized
    }
}

fn migrate_legacy_data(root: &Path) -> io::Result<()> {
    let local = legacy_local_root();
    let roaming = legacy_roaming_root();
    let webview = legacy_webview_root();

    let mut copied: Vec<(PathBuf, PathBuf)> = Vec::new();
    let result = (|| -> io::Result<()> {
        // Keep the SQLite database and its WAL consistent as one unit.
        if !root.join("disksage.db").exists() {
            for name in ["disksage.db", "disksage.db-wal", "disksage.db-shm"] {
                let source = local.join(name);
                if source.exists() {
                    copy_entry(&source, &root.join(name), &mut copied)?;
                }
            }
        }

        for name in ["logs", "jobs", "rules", "quarantine"] {
            let source = local.join(name);
            if source.exists() {
                copy_entry(&source, &root.join(name), &mut copied)?;
            }
        }

        for name in ["llm.json", "auto-clean.json"] {
            let source = roaming.join(name);
            if source.exists() {
                copy_entry(&source, &root.join(name), &mut copied)?;
            }
        }

        let source_webview = webview.join("EBWebView");
        if source_webview.exists() {
            copy_entry(&source_webview, &root.join("EBWebView"), &mut copied)?;
        }
        Ok(())
    })();

    if let Err(error) = result {
        // Remove anything this attempt created, so the caller can safely keep
        // using the legacy data location.
        for (_, destination) in copied.into_iter().rev() {
            if destination.is_dir() {
                let _ = fs::remove_dir_all(&destination);
            } else {
                let _ = fs::remove_file(&destination);
            }
        }
        return Err(error);
    }

    // The destination is complete. Remove only sources that were actually
    // copied; if a destination already existed, its legacy source stays put.
    remove_legacy_sources(&copied, &local, &roaming, &webview);
    Ok(())
}

fn copy_entry(
    source: &Path,
    destination: &Path,
    copied: &mut Vec<(PathBuf, PathBuf)>,
) -> io::Result<()> {
    if !source.exists() || destination.exists() {
        return Ok(());
    }
    copied.push((source.to_path_buf(), destination.to_path_buf()));
    copy_path(source, destination)?;
    verify_copy(source, destination)
}

fn copy_path(source: &Path, destination: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        return Err(io::Error::other(format!(
            "refusing to migrate symlink {}",
            source.display()
        )));
    }
    if metadata.is_dir() {
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_path(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else if metadata.is_file() {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = fs::copy(source, destination)?;
        if bytes != metadata.len() {
            return Err(io::Error::other(format!(
                "incomplete copy for {}",
                source.display()
            )));
        }
    }
    Ok(())
}

fn verify_copy(source: &Path, destination: &Path) -> io::Result<()> {
    let (source_files, source_bytes) = tree_stats(source)?;
    let (destination_files, destination_bytes) = tree_stats(destination)?;
    if source_files != destination_files || source_bytes != destination_bytes {
        return Err(io::Error::other(format!(
            "migration verification failed for {}",
            source.display()
        )));
    }
    Ok(())
}

fn tree_stats(path: &Path) -> io::Result<(u64, u64)> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_file() {
        return Ok((1, metadata.len()));
    }
    if !metadata.is_dir() {
        return Ok((0, 0));
    }
    let mut files = 0;
    let mut bytes = 0;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let (child_files, child_bytes) = tree_stats(&entry.path())?;
        files += child_files;
        bytes += child_bytes;
    }
    Ok((files, bytes))
}

fn remove_legacy_sources(
    copied: &[(PathBuf, PathBuf)],
    local: &Path,
    roaming: &Path,
    webview: &Path,
) {
    for (source, _) in copied {
        if source.is_dir() {
            let _ = fs::remove_dir_all(source);
        } else {
            let _ = fs::remove_file(source);
        }
    }
    remove_if_empty(webview);
    remove_if_empty(local);
    remove_if_empty(roaming);
}

fn remove_if_empty(path: &Path) {
    if let Ok(mut entries) = fs::read_dir(path)
        && entries.next().is_none()
    {
        let _ = fs::remove_dir(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_drive_letters() {
        assert_eq!(disk_letter(Path::new(r"D:\Apps\app.exe")), Some('D'));
        assert_eq!(disk_letter(Path::new(r"\\?\C:\Apps\app.exe")), Some('C'));
        assert_eq!(disk_letter(Path::new(r"\\server\share\app.exe")), None);
    }

    #[test]
    fn sanitizes_user_component() {
        let component = user_component();
        assert!(!component.is_empty());
        assert!(!component.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*']));
    }
}
