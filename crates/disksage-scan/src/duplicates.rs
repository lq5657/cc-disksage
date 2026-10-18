//! Duplicate file discovery. Files are grouped by size, then by a partial
//! BLAKE3 hash, then by a full hash. The scan is read-only.

use std::collections::HashMap;
use std::fs::{File, Metadata};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use blake3::Hash;
use walkdir::WalkDir;

use crate::walker::{CancelToken, ScanError, is_skipped_dir};

const PARTIAL_HASH_BYTES: u64 = 64 * 1024;
const HASH_BUFFER_BYTES: usize = 1024 * 1024;
const DUPLICATE_SKIPPED_DIRS: &[&str] = &[
    "AppData",
    "Application Data",
    "$Recycle.Bin",
    "System Volume Information",
    "WindowsApps",
    "Recovery",
    "node_modules",
    ".git",
    ".svn",
    ".hg",
    "target",
    "Tencent",
    "Tencent Files",
    "WeChat",
    "WeChat Files",
    "QQ",
    "DingTalk",
    "WPS Cloud Files",
];
#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
#[cfg(windows)]
const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
#[cfg(windows)]
const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;

#[derive(Debug, Clone)]
pub struct DuplicateRequest {
    pub roots: Vec<PathBuf>,
    pub min_size: u64,
    pub max_files: u64,
    pub max_groups: usize,
    pub max_files_per_group: usize,
}

impl DuplicateRequest {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            roots,
            min_size: 1024 * 1024,
            max_files: 2_000_000,
            max_groups: 300,
            max_files_per_group: 50,
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct DuplicateScanProgress {
    pub phase: String,
    pub files_seen: u64,
    pub candidate_files: u64,
    pub hashed_files: u64,
    pub groups_found: u64,
    pub bytes_hashed: u64,
    pub elapsed_ms: u64,
    pub current: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DuplicateFile {
    pub id: String,
    pub path: PathBuf,
    pub modified_at: Option<i64>,
    pub allocated_bytes: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DuplicateGroup {
    pub id: String,
    pub size: u64,
    pub hash: String,
    pub files: Vec<DuplicateFile>,
    pub wasted_bytes: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DuplicateScanResult {
    pub scan_id: String,
    pub roots: Vec<PathBuf>,
    pub groups: Vec<DuplicateGroup>,
    pub total_wasted_bytes: u64,
    pub files_scanned: u64,
    pub candidate_files: u64,
    pub hashed_files: u64,
    pub error_count: u64,
    pub duration_ms: u64,
    pub truncated: bool,
    pub min_size: u64,
}

#[derive(Clone)]
struct Candidate {
    path: PathBuf,
    modified_at: Option<i64>,
}

struct Stats {
    files_seen: u64,
    candidate_files: u64,
    hashed_files: u64,
    bytes_hashed: u64,
    errors: u64,
    truncated: bool,
}

struct Reporter {
    started: Instant,
    last: Instant,
    progress: Box<dyn Fn(DuplicateScanProgress) + Send + Sync>,
}

impl Reporter {
    fn send(&mut self, phase: &str, stats: &Stats, groups_found: u64, current: &Path) {
        if self.last.elapsed() < Duration::from_millis(100) {
            return;
        }
        self.last = Instant::now();
        (self.progress)(DuplicateScanProgress {
            phase: phase.into(),
            files_seen: stats.files_seen,
            candidate_files: stats.candidate_files,
            hashed_files: stats.hashed_files,
            groups_found,
            bytes_hashed: stats.bytes_hashed,
            elapsed_ms: self.started.elapsed().as_millis() as u64,
            current: current.display().to_string(),
        });
    }
}

fn is_duplicate_skipped_dir(name: &str) -> bool {
    is_skipped_dir(name)
        || DUPLICATE_SKIPPED_DIRS
            .iter()
            .any(|skipped| skipped.eq_ignore_ascii_case(name))
}

fn modified_timestamp(metadata: &Metadata) -> Option<i64> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
}

fn is_unsafe_to_read(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        let attributes = metadata.file_attributes();
        attributes
            & (FILE_ATTRIBUTE_REPARSE_POINT
                | FILE_ATTRIBUTE_OFFLINE
                | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
            != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}

#[cfg(windows)]
fn file_identity(path: &Path) -> Option<((u64, u64), u32)> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let file = File::open(path).ok()?;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    unsafe {
        GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut information).ok()?;
    }
    Some((
        (
            u64::from(information.dwVolumeSerialNumber),
            (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
        ),
        information.nNumberOfLinks,
    ))
}

#[cfg(not(windows))]
fn file_identity(_path: &Path) -> Option<((u64, u64), u32)> {
    None
}

enum HashFailure {
    Cancelled,
    Io,
}

fn partial_hash(path: &Path, size: u64) -> Result<Hash, io::Error> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0u8; PARTIAL_HASH_BYTES as usize];
    let head = file.read(&mut buffer)?;
    hasher.update(&buffer[..head]);
    if size > PARTIAL_HASH_BYTES * 2 {
        file.seek(SeekFrom::End(-(PARTIAL_HASH_BYTES as i64)))?;
        let tail = file.read(&mut buffer)?;
        hasher.update(&buffer[..tail]);
    }
    Ok(hasher.finalize())
}

fn full_hash(path: &Path, cancel: &CancelToken) -> Result<Hash, HashFailure> {
    let mut file = File::open(path).map_err(|_| HashFailure::Io)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0u8; HASH_BUFFER_BYTES];
    loop {
        if cancel.is_cancelled() {
            return Err(HashFailure::Cancelled);
        }
        let read = file.read(&mut buffer).map_err(|_| HashFailure::Io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize())
}

fn collect_candidates(
    request: &DuplicateRequest,
    cancel: &CancelToken,
    stats: &mut Stats,
    reporter: &mut Reporter,
) -> Result<HashMap<u64, Vec<Candidate>>, ScanError> {
    let mut sizes: HashMap<u64, Vec<Candidate>> = HashMap::new();

    for root in &request.roots {
        if !root.exists() {
            continue;
        }
        let walker = WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| {
                if entry.depth() == 0 {
                    return true;
                }
                let name = entry.file_name().to_string_lossy();
                if entry.file_type().is_dir() {
                    if is_duplicate_skipped_dir(&name) {
                        return false;
                    }
                    if entry
                        .metadata()
                        .map(|metadata| is_unsafe_to_read(&metadata))
                        .unwrap_or(true)
                    {
                        return false;
                    }
                }
                if entry.file_type().is_symlink() {
                    return false;
                }
                true
            });

        for entry in walker {
            if cancel.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    stats.errors = stats.errors.saturating_add(1);
                    continue;
                }
            };
            if !entry.file_type().is_file() {
                continue;
            }
            stats.files_seen = stats.files_seen.saturating_add(1);
            if stats.files_seen > request.max_files {
                stats.truncated = true;
                break;
            }
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(_) => {
                    stats.errors = stats.errors.saturating_add(1);
                    continue;
                }
            };
            if is_unsafe_to_read(&metadata) {
                continue;
            }
            let size = metadata.len();
            if size < request.min_size || size == 0 {
                continue;
            }
            stats.candidate_files = stats.candidate_files.saturating_add(1);
            sizes.entry(size).or_default().push(Candidate {
                path: entry.path().to_path_buf(),
                modified_at: modified_timestamp(&metadata),
            });
            reporter.send("collecting", stats, 0, entry.path());
        }
        if stats.truncated {
            break;
        }
    }
    Ok(sizes)
}

/// Opening every unique-size file just to detect hard links is wasteful, so
/// this runs after size grouping. A hard-linked file is one file, not a
/// duplicate; keep the first path seen.
fn dedupe_hard_links(
    sizes: &mut HashMap<u64, Vec<Candidate>>,
    cancel: &CancelToken,
) -> Result<(), ScanError> {
    for candidates in sizes.values_mut() {
        if candidates.len() < 2 {
            continue;
        }
        let mut seen = std::collections::HashSet::new();
        let mut index = 0;
        while index < candidates.len() {
            if cancel.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            if let Some((identity, links)) = file_identity(&candidates[index].path)
                && (links > 1 || !seen.insert(identity))
            {
                // A file with multiple hard links is one file occupying one
                // set of clusters; removing one name does not free space.
                candidates.remove(index);
                continue;
            }
            index += 1;
        }
    }
    Ok(())
}

fn group_stage(
    cancel: &CancelToken,
    stats: &mut Stats,
    reporter: &mut Reporter,
    sizes: HashMap<u64, Vec<Candidate>>,
    full: bool,
) -> Result<Vec<(u64, Hash, Vec<Candidate>)>, ScanError> {
    let phase = if full { "full_hash" } else { "partial_hash" };
    let mut groups: HashMap<(u64, [u8; 32]), Vec<Candidate>> = HashMap::new();

    for (size, candidates) in sizes {
        if candidates.len() < 2 {
            continue;
        }
        for candidate in candidates {
            if cancel.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            let current = candidate.path.clone();
            let hash = if full {
                match full_hash(&candidate.path, cancel) {
                    Ok(hash) => hash,
                    Err(HashFailure::Cancelled) => return Err(ScanError::Cancelled),
                    Err(HashFailure::Io) => {
                        stats.errors = stats.errors.saturating_add(1);
                        continue;
                    }
                }
            } else {
                match partial_hash(&candidate.path, size) {
                    Ok(hash) => hash,
                    Err(_) => {
                        stats.errors = stats.errors.saturating_add(1);
                        continue;
                    }
                }
            };
            stats.hashed_files = stats.hashed_files.saturating_add(1);
            stats.bytes_hashed = stats.bytes_hashed.saturating_add(if full {
                size
            } else {
                size.min(PARTIAL_HASH_BYTES * 2)
            });
            groups
                .entry((size, *hash.as_bytes()))
                .or_default()
                .push(candidate);
            reporter.send(phase, stats, groups.len() as u64, &current);
        }
    }

    let mut result = Vec::new();
    for ((size, hash), candidates) in groups {
        if candidates.len() < 2 {
            continue;
        }
        result.push((size, Hash::from_bytes(hash), candidates));
    }
    Ok(result)
}

pub fn find_duplicates(
    request: DuplicateRequest,
    progress: impl Fn(DuplicateScanProgress) + Send + Sync + 'static,
    cancel: CancelToken,
) -> Result<DuplicateScanResult, ScanError> {
    let started = Instant::now();
    let mut stats = Stats {
        files_seen: 0,
        candidate_files: 0,
        hashed_files: 0,
        bytes_hashed: 0,
        errors: 0,
        truncated: false,
    };
    let mut reporter = Reporter {
        started,
        last: Instant::now() - Duration::from_secs(1),
        progress: Box::new(progress),
    };

    let mut sizes = collect_candidates(&request, &cancel, &mut stats, &mut reporter)?;
    dedupe_hard_links(&mut sizes, &cancel)?;
    let partial = group_stage(&cancel, &mut stats, &mut reporter, sizes, false)?;
    let partial_sizes = partial
        .into_iter()
        .map(|(size, _, candidates)| (size, candidates))
        .collect::<HashMap<_, _>>();
    let full = group_stage(&cancel, &mut stats, &mut reporter, partial_sizes, true)?;

    let mut groups = Vec::new();
    for (size, hash, mut candidates) in full {
        candidates.sort_by(|a, b| a.path.cmp(&b.path));
        if candidates.len() > request.max_files_per_group {
            candidates.truncate(request.max_files_per_group);
            stats.truncated = true;
        }
        let group_id = uuid::Uuid::new_v4().to_string();
        let files = candidates
            .into_iter()
            .map(|candidate| DuplicateFile {
                id: uuid::Uuid::new_v4().to_string(),
                path: candidate.path,
                modified_at: candidate.modified_at,
                allocated_bytes: size,
            })
            .collect::<Vec<_>>();
        let wasted_bytes = size.saturating_mul(files.len().saturating_sub(1) as u64);
        groups.push(DuplicateGroup {
            id: group_id,
            size,
            hash: hash.to_hex().to_string(),
            files,
            wasted_bytes,
        });
    }

    groups.sort_by_key(|group| std::cmp::Reverse(group.wasted_bytes));
    let mut truncated = stats.truncated;
    if groups.len() > request.max_groups {
        groups.truncate(request.max_groups);
        truncated = true;
    }
    let total_wasted_bytes = groups
        .iter()
        .map(|group| group.wasted_bytes)
        .fold(0u64, u64::saturating_add);

    Ok(DuplicateScanResult {
        scan_id: uuid::Uuid::new_v4().to_string(),
        roots: request.roots,
        groups,
        total_wasted_bytes,
        files_scanned: stats.files_seen,
        candidate_files: stats.candidate_files,
        hashed_files: stats.hashed_files,
        error_count: stats.errors,
        duration_ms: started.elapsed().as_millis() as u64,
        truncated,
        min_size: request.min_size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn finds_only_real_duplicates() {
        let temp = tempfile::tempdir().unwrap();
        write(&temp.path().join("a.bin"), &vec![7u8; 4096]);
        write(&temp.path().join("nested").join("b.bin"), &vec![7u8; 4096]);
        write(&temp.path().join("c.bin"), &vec![8u8; 4096]);
        write(&temp.path().join("small-a.bin"), b"same");
        write(&temp.path().join("small-b.bin"), b"same");

        let mut request = DuplicateRequest::new(vec![temp.path().to_path_buf()]);
        request.min_size = 1024;
        let result = find_duplicates(request, |_| {}, CancelToken::new()).unwrap();
        assert_eq!(result.groups.len(), 1);
        assert_eq!(result.groups[0].files.len(), 2);
        assert_eq!(result.groups[0].size, 4096);
        assert_eq!(result.groups[0].wasted_bytes, 4096);
        assert_eq!(result.total_wasted_bytes, 4096);
    }

    #[test]
    fn skips_protected_directories_and_reparse_links() {
        let temp = tempfile::tempdir().unwrap();
        write(&temp.path().join("a.bin"), &vec![1u8; 2048]);
        write(
            &temp.path().join("WindowsApps").join("b.bin"),
            &vec![1u8; 2048],
        );
        write(
            &temp.path().join("System Volume Information").join("c.bin"),
            &vec![1u8; 2048],
        );
        write(&temp.path().join("AppData").join("d.bin"), &vec![1u8; 2048]);
        write(&temp.path().join("Tencent").join("e.bin"), &vec![1u8; 2048]);
        write(
            &temp.path().join("Tencent Files").join("f.bin"),
            &vec![1u8; 2048],
        );
        let mut request = DuplicateRequest::new(vec![temp.path().to_path_buf()]);
        request.min_size = 1024;
        let result = find_duplicates(request, |_| {}, CancelToken::new()).unwrap();
        assert!(result.groups.is_empty());
    }

    #[test]
    #[ignore = "read-only duplicate scan of the real user profile; can be slow"]
    fn scans_real_user_profile_read_only() {
        let Some(profile) = std::env::var_os("USERPROFILE").map(PathBuf::from) else {
            return;
        };
        let roots = ["Documents", "Downloads", "Pictures", "Videos", "Desktop"]
            .into_iter()
            .map(|name| profile.join(name))
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>();
        let mut request = DuplicateRequest::new(roots);
        request.min_size = 1024 * 1024;
        request.max_groups = 20;
        request.max_files_per_group = 10;
        let result = find_duplicates(request, |_| {}, CancelToken::new()).unwrap();
        println!(
            "groups={} wasted={} files={} candidates={} hashed={} errors={} duration_ms={} truncated={}",
            result.groups.len(),
            result.total_wasted_bytes,
            result.files_scanned,
            result.candidate_files,
            result.hashed_files,
            result.error_count,
            result.duration_ms,
            result.truncated
        );
        for group in &result.groups {
            println!(
                "{} bytes x {} -> {}",
                group.size,
                group.files.len(),
                group.files[0].path.display()
            );
        }
    }

    #[test]
    fn respects_min_size_and_cancellation() {
        let temp = tempfile::tempdir().unwrap();
        write(&temp.path().join("a.bin"), &vec![1u8; 4096]);
        write(&temp.path().join("b.bin"), &vec![1u8; 4096]);
        let mut request = DuplicateRequest::new(vec![temp.path().to_path_buf()]);
        request.min_size = 8192;
        let result = find_duplicates(request, |_| {}, CancelToken::new()).unwrap();
        assert!(result.groups.is_empty());

        let cancelled = CancelToken::default();
        cancelled.cancel();
        let mut request = DuplicateRequest::new(vec![temp.path().to_path_buf()]);
        request.min_size = 1024;
        assert!(matches!(
            find_duplicates(request, |_| {}, cancelled),
            Err(ScanError::Cancelled)
        ));
    }
}
