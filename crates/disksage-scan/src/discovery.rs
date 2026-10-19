//! Read-only deep discovery.
//!
//! This walker only reports files that may be worth reviewing. It never
//! deletes or modifies anything, never follows reparse points, and skips the
//! protected/system directories that must not be traversed.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use walkdir::WalkDir;

use crate::walker::{CancelToken, is_skipped_dir};

const DEFAULT_MIN_LARGE_BYTES: u64 = 100 * 1024 * 1024;
const DEFAULT_MIN_LOG_BYTES: u64 = 50 * 1024 * 1024;
const DEFAULT_MIN_INSTALLER_BYTES: u64 = 100 * 1024 * 1024;
const DEFAULT_MIN_AGE_DAYS: u32 = 180;
const DEFAULT_MAX_ITEMS: usize = 500;
const DEFAULT_MAX_COLLECTED: usize = 5_000;

#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

#[derive(Debug, Clone)]
pub struct DiscoveryRequest {
    pub root: PathBuf,
    pub min_large_bytes: u64,
    pub min_log_bytes: u64,
    pub min_installer_bytes: u64,
    pub min_age_days: u32,
    pub max_items: usize,
    pub max_collected: usize,
}

impl DiscoveryRequest {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        // `SystemDrive` is usually `C:`; WalkDir would otherwise produce
        // drive-relative paths such as `C:Users\...`.
        let root = if root.to_string_lossy().ends_with(':') {
            PathBuf::from(format!("{}\\", root.display()))
        } else {
            root
        };
        Self {
            root,
            min_large_bytes: DEFAULT_MIN_LARGE_BYTES,
            min_log_bytes: DEFAULT_MIN_LOG_BYTES,
            min_installer_bytes: DEFAULT_MIN_INSTALLER_BYTES,
            min_age_days: DEFAULT_MIN_AGE_DAYS,
            max_items: DEFAULT_MAX_ITEMS,
            max_collected: DEFAULT_MAX_COLLECTED,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveryProgress {
    pub files: u64,
    pub dirs: u64,
    pub elapsed_ms: u64,
    pub current: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveryItem {
    pub index: usize,
    pub path: String,
    pub name: String,
    pub bytes: u64,
    pub modified_at: Option<i64>,
    pub extension: String,
    pub category: String,
    pub reason_zh: String,
    pub reason_en: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveryResult {
    pub volume: char,
    pub scanned_files: u64,
    pub scanned_dirs: u64,
    pub elapsed_ms: u64,
    pub items: Vec<DiscoveryItem>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy)]
enum Category {
    LargeFile,
    LargeLog,
    Dump,
    OldInstaller,
    OldArchive,
}

impl Category {
    fn key(self) -> &'static str {
        match self {
            Self::LargeFile => "large_file",
            Self::LargeLog => "large_log",
            Self::Dump => "dump",
            Self::OldInstaller => "old_installer",
            Self::OldArchive => "old_archive",
        }
    }

    fn reason_zh(self) -> &'static str {
        match self {
            Self::LargeFile => "大文件；请人工确认用途。本工具只读展示，不会自动删除。",
            Self::LargeLog => "较大的日志/ETL；请确认是否仍需要这些诊断信息。",
            Self::Dump => "较大的转储文件；请确认是否仍需要历史崩溃诊断。",
            Self::OldInstaller => "较旧的安装包或镜像；请确认是否仍需要保留。",
            Self::OldArchive => "较旧的压缩包；请确认内容是否仍需要保留。",
        }
    }

    fn reason_en(self) -> &'static str {
        match self {
            Self::LargeFile => {
                "Large file; review its purpose manually. This tool is read-only and never deletes it."
            }
            Self::LargeLog => "Large log/ETL; confirm whether the diagnostic data is still needed.",
            Self::Dump => {
                "Large dump; confirm whether historical crash diagnostics are still needed."
            }
            Self::OldInstaller => "Old installer or image; confirm whether it is still needed.",
            Self::OldArchive => "Old archive; confirm whether its contents are still needed.",
        }
    }
}

pub fn run_discovery(
    request: DiscoveryRequest,
    progress: impl Fn(DiscoveryProgress) + Send + Sync + 'static,
    cancel: CancelToken,
) -> Result<DiscoveryResult, String> {
    let started = Instant::now();
    let mut files = 0u64;
    let mut dirs = 1u64;
    let mut items = Vec::new();
    let mut truncated = false;
    let mut last_report = Instant::now() - Duration::from_secs(1);

    let age_cutoff = if request.min_age_days == 0 {
        None
    } else {
        SystemTime::now().checked_sub(Duration::from_secs(
            u64::from(request.min_age_days) * 86_400,
        ))
    };

    let walker = WalkDir::new(&request.root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            if entry.path_is_symlink() {
                return false;
            }
            !(entry.file_type().is_dir() && should_skip_dir(entry.path()))
        });

    for entry_result in walker {
        if cancel.is_cancelled() {
            return Err("扫描已取消".into());
        }
        let entry = match entry_result {
            Ok(entry) => entry,
            Err(_) => continue,
        };

        if entry.file_type().is_dir() {
            dirs = dirs.saturating_add(1);
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        files = files.saturating_add(1);

        let metadata = match fs::symlink_metadata(entry.path()) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if is_reparse(&metadata) {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if matches!(
            name.as_str(),
            "pagefile.sys" | "hiberfil.sys" | "swapfile.sys"
        ) {
            continue;
        }

        let size = metadata.len();
        let modified = metadata.modified().ok();
        let extension = entry
            .path()
            .extension()
            .map(|value| value.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();

        if let Some(category) = classify(&request, &extension, size, modified, age_cutoff) {
            if items.len() < request.max_collected {
                items.push(DiscoveryItem {
                    index: 0,
                    path: entry.path().display().to_string(),
                    name: entry.file_name().to_string_lossy().into_owned(),
                    bytes: size,
                    modified_at: modified.and_then(system_time_to_timestamp),
                    extension,
                    category: category.key().into(),
                    reason_zh: category.reason_zh().into(),
                    reason_en: category.reason_en().into(),
                });
            } else {
                truncated = true;
            }
        }

        if last_report.elapsed() >= Duration::from_millis(250) {
            last_report = Instant::now();
            progress(DiscoveryProgress {
                files,
                dirs,
                elapsed_ms: started.elapsed().as_millis() as u64,
                current: entry.path().display().to_string(),
            });
        }
    }

    items.sort_by(|left, right| {
        right
            .bytes
            .cmp(&left.bytes)
            .then_with(|| left.path.cmp(&right.path))
    });
    truncated |= items.len() > request.max_items;
    items.truncate(request.max_items);
    for (index, item) in items.iter_mut().enumerate() {
        item.index = index;
    }

    progress(DiscoveryProgress {
        files,
        dirs,
        elapsed_ms: started.elapsed().as_millis() as u64,
        current: String::new(),
    });

    Ok(DiscoveryResult {
        volume: volume_letter(&request.root),
        scanned_files: files,
        scanned_dirs: dirs,
        elapsed_ms: started.elapsed().as_millis() as u64,
        items,
        truncated,
    })
}

fn classify(
    request: &DiscoveryRequest,
    extension: &str,
    size: u64,
    modified: Option<SystemTime>,
    age_cutoff: Option<SystemTime>,
) -> Option<Category> {
    let is_log = matches!(extension, "log" | "etl");
    let is_dump = matches!(extension, "dmp" | "mdmp");
    let is_installer = matches!(
        extension,
        "msi" | "exe" | "iso" | "msp" | "msu" | "appx" | "msix"
    );
    let is_archive = matches!(extension, "zip" | "7z" | "rar");

    if is_dump && size >= request.min_log_bytes {
        return Some(Category::Dump);
    }
    if is_log && size >= request.min_log_bytes {
        return Some(Category::LargeLog);
    }
    if (is_installer || is_archive) && size >= request.min_installer_bytes {
        let old_enough = match (modified, age_cutoff) {
            (Some(modified), Some(cutoff)) => modified <= cutoff,
            (Some(_), None) => true,
            (None, _) => false,
        };
        if old_enough {
            return Some(if is_installer {
                Category::OldInstaller
            } else {
                Category::OldArchive
            });
        }
    }
    if size >= request.min_large_bytes {
        Some(Category::LargeFile)
    } else {
        None
    }
}

fn should_skip_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if is_skipped_dir(name) {
        return true;
    }
    matches!(
        name.to_ascii_lowercase().as_str(),
        "winsxs"
            | "installer"
            | "system32"
            | "syswow64"
            | "servicing"
            | "driverstore"
            | "program files"
            | "program files (x86)"
            | "node_modules"
            | ".git"
            | ".svn"
            | ".hg"
            | "target"
            | "tencent"
            | "tencent files"
            | "wechat"
            | "wechat files"
            | "xwechat_files"
            | "qq"
            | "dingtalk"
            | "wps cloud files"
    )
}

fn volume_letter(root: &Path) -> char {
    root.to_string_lossy()
        .chars()
        .next()
        .map(|letter| letter.to_ascii_uppercase())
        .unwrap_or('C')
}

fn system_time_to_timestamp(time: SystemTime) -> Option<i64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs() as i64)
}

#[cfg(windows)]
fn is_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, bytes: usize) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, vec![b'x'; bytes]).unwrap();
    }

    #[test]
    fn classifies_and_skips_protected_paths() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        write(&root.join("large.bin"), 2);
        write(&root.join("service.log"), 2);
        write(&root.join("crash.dmp"), 2);
        write(&root.join("old.iso"), 2);
        write(&root.join("old.zip"), 2);
        write(&root.join("small.bin"), 1);
        write(&root.join("pagefile.sys"), 2);
        write(&root.join("WinSxS").join("big.bin"), 2);
        write(&root.join("Program Files").join("app.exe"), 2);

        let mut request = DiscoveryRequest::new(root);
        request.min_large_bytes = 2;
        request.min_log_bytes = 2;
        request.min_installer_bytes = 2;
        request.min_age_days = 0;
        request.max_items = 100;
        request.max_collected = 100;

        let result = run_discovery(request, |_| {}, CancelToken::new()).unwrap();
        let categories = result
            .items
            .iter()
            .map(|item| (item.name.as_str(), item.category.as_str()))
            .collect::<Vec<_>>();

        assert!(categories.contains(&("large.bin", "large_file")));
        assert!(categories.contains(&("service.log", "large_log")));
        assert!(categories.contains(&("crash.dmp", "dump")));
        assert!(categories.contains(&("old.iso", "old_installer")));
        assert!(categories.contains(&("old.zip", "old_archive")));
        assert!(!categories.iter().any(|(name, _)| *name == "small.bin"));
        assert!(!categories.iter().any(|(name, _)| *name == "pagefile.sys"));
        assert!(!categories.iter().any(|(name, _)| *name == "big.bin"));
        assert!(!categories.iter().any(|(name, _)| *name == "app.exe"));
    }

    #[test]
    fn normalizes_drive_only_root() {
        let request = DiscoveryRequest::new("C:");
        assert_eq!(request.root, PathBuf::from(r"C:\"));
    }

    #[test]
    fn respects_cancellation() {
        let temp = tempfile::tempdir().unwrap();
        write(&temp.path().join("large.bin"), 2);
        let cancel = CancelToken::new();
        cancel.cancel();

        let mut request = DiscoveryRequest::new(temp.path());
        request.min_large_bytes = 1;
        let error = run_discovery(request, |_| {}, cancel).unwrap_err();
        assert!(error.contains("取消"));
    }

    #[test]
    #[ignore = "read-only deep discovery over the real system drive; can take a minute"]
    fn scans_real_system_drive_read_only() {
        let root = std::env::var_os("SystemDrive")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\"));
        let result =
            run_discovery(DiscoveryRequest::new(root), |_| {}, CancelToken::new()).unwrap();
        println!(
            "deep discovery: {} files, {} dirs, {} items in {} ms",
            result.scanned_files,
            result.scanned_dirs,
            result.items.len(),
            result.elapsed_ms
        );
        for item in result.items.iter().take(20) {
            println!("{} {} {}", item.category, item.bytes, item.path);
        }
        assert!(result.scanned_files > 0);
    }
}
