use std::fs::Metadata;

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

pub fn is_reparse_point(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

/// Best-effort classification for locked files. Windows sharing violations and
/// access-denied errors are common when a browser or app is open.
pub fn looks_locked(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("os error 32")
        || lower.contains("os error 5")
        || lower.contains("sharing violation")
        || lower.contains("being used by another process")
        || lower.contains("另一个程序正在使用")
        || lower.contains("另一个程序")
        || lower.contains("占用")
        || lower.contains("拒绝访问")
        || lower.contains("access is denied")
}
