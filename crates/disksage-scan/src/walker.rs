use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use disksage_core::{DirNode, LargeFile, ScanSnapshot};
use rayon::prelude::*;

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

const DEFAULT_MAX_NODES: u64 = 2_000_000;
const DEFAULT_LARGE_FILE_BYTES: u64 = 500 * 1024 * 1024;
const DEFAULT_TOP_LARGE_FILES: usize = 200;
const MAX_COLLECTED_LARGE_FILES: usize = 10_000;

const SKIPPED_DIR_NAMES: &[&str] = &[
    "$Recycle.Bin",
    "System Volume Information",
    "WindowsApps",
    "Recovery",
];

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("scan cancelled")]
    Cancelled,
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("mft error: {0}")]
    Mft(String),
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ScanProgress {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub errors: u64,
    pub elapsed_ms: u64,
    pub current: String,
}

#[derive(Debug, Clone)]
pub struct ScanRequest {
    pub root: PathBuf,
    pub max_nodes: u64,
    pub large_file_threshold: u64,
    pub top_large_files: usize,
}

impl ScanRequest {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            max_nodes: DEFAULT_MAX_NODES,
            large_file_threshold: DEFAULT_LARGE_FILE_BYTES,
            top_large_files: DEFAULT_TOP_LARGE_FILES,
        }
    }
}

/// Cooperative cancellation token shared with the UI.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

struct ScanContext {
    cancel: CancelToken,
    nodes: AtomicU64,
    dirs: AtomicU64,
    files: AtomicU64,
    bytes: AtomicU64,
    errors: AtomicU64,
    node_limit_hit: AtomicBool,
    max_nodes: u64,
    large_threshold: u64,
    large_files: Mutex<Vec<LargeFile>>,
    progress: Box<dyn Fn(ScanProgress) + Send + Sync>,
    started: Instant,
    last_report: Mutex<Instant>,
}

impl ScanContext {
    fn reserve_node(&self) -> bool {
        let node = self.nodes.fetch_add(1, Ordering::Relaxed) + 1;
        if node > self.max_nodes {
            self.node_limit_hit.store(true, Ordering::Relaxed);
            return false;
        }
        self.dirs.fetch_add(1, Ordering::Relaxed);
        true
    }

    fn report(&self, current: &Path) {
        if let Ok(mut last) = self.last_report.lock() {
            if last.elapsed() < Duration::from_millis(100) {
                return;
            }
            *last = Instant::now();
        }
        (self.progress)(ScanProgress {
            files: self.files.load(Ordering::Relaxed),
            dirs: self.dirs.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            elapsed_ms: self.started.elapsed().as_millis() as u64,
            current: current.display().to_string(),
        });
    }

    fn record_large_file(
        &self,
        path: &Path,
        len: u64,
        modified_at: Option<i64>,
        extension: String,
    ) {
        if len < self.large_threshold {
            return;
        }
        if let Ok(mut files) = self.large_files.lock() {
            if files.len() >= MAX_COLLECTED_LARGE_FILES {
                return;
            }
            files.push(LargeFile {
                path: path.to_path_buf(),
                allocated_bytes: len,
                logical_bytes: len,
                modified_at,
                extension,
            });
        }
    }
}

fn system_time_to_timestamp(time: SystemTime) -> Option<i64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs() as i64)
}

pub(crate) fn is_skipped_dir(name: &str) -> bool {
    SKIPPED_DIR_NAMES
        .iter()
        .any(|skipped| skipped.eq_ignore_ascii_case(name))
}

fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn extension_of(path: &Path) -> String {
    path.extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Read one directory's immediate files and collect its subdirectories
/// without descending.
fn scan_shallow(path: &Path, ctx: &ScanContext) -> (DirNode, Vec<PathBuf>) {
    let mut node = DirNode {
        name: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string()),
        path: path.to_path_buf(),
        ..Default::default()
    };
    let mut subdirs = Vec::new();

    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => {
            ctx.errors.fetch_add(1, Ordering::Relaxed);
            return (node, subdirs);
        }
    };

    for entry in entries {
        if ctx.cancel.is_cancelled() {
            break;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                ctx.errors.fetch_add(1, Ordering::Relaxed);
                continue;
            }
        };
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => {
                ctx.errors.fetch_add(1, Ordering::Relaxed);
                continue;
            }
        };
        if is_reparse_point(&metadata) {
            continue;
        }
        if metadata.is_dir() {
            let name = entry.file_name();
            if is_skipped_dir(&name.to_string_lossy()) {
                continue;
            }
            subdirs.push(entry.path());
        } else if metadata.is_file() {
            let len = metadata.len();
            node.logical_bytes = node.logical_bytes.saturating_add(len);
            node.allocated_bytes = node.allocated_bytes.saturating_add(len);
            node.file_count = node.file_count.saturating_add(1);
            ctx.files.fetch_add(1, Ordering::Relaxed);
            ctx.bytes.fetch_add(len, Ordering::Relaxed);
            ctx.record_large_file(
                &entry.path(),
                len,
                metadata.modified().ok().and_then(system_time_to_timestamp),
                extension_of(&entry.path()),
            );
        }
    }

    (node, subdirs)
}

fn scan_dir(path: &Path, ctx: &ScanContext) -> DirNode {
    if ctx.cancel.is_cancelled() || !ctx.reserve_node() {
        return DirNode {
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
            path: path.to_path_buf(),
            ..Default::default()
        };
    }

    let (mut node, subdirs) = scan_shallow(path, ctx);
    for subdir in subdirs {
        if ctx.cancel.is_cancelled() {
            break;
        }
        let child = scan_dir(&subdir, ctx);
        node.allocated_bytes = node.allocated_bytes.saturating_add(child.allocated_bytes);
        node.logical_bytes = node.logical_bytes.saturating_add(child.logical_bytes);
        node.file_count = node.file_count.saturating_add(child.file_count);
        node.children.push(child);
    }
    ctx.report(path);
    node
}

/// Full-volume scan. Top-level directories are scanned in parallel; each
/// subtree is walked sequentially to keep the implementation predictable.
pub fn scan_volume(
    request: ScanRequest,
    progress: impl Fn(ScanProgress) + Send + Sync + 'static,
    cancel: CancelToken,
) -> Result<ScanSnapshot, ScanError> {
    let started = Instant::now();
    let started_at = system_time_to_timestamp(SystemTime::now()).unwrap_or_default();

    let ctx = ScanContext {
        cancel: cancel.clone(),
        nodes: AtomicU64::new(0),
        dirs: AtomicU64::new(0),
        files: AtomicU64::new(0),
        bytes: AtomicU64::new(0),
        errors: AtomicU64::new(0),
        node_limit_hit: AtomicBool::new(false),
        max_nodes: request.max_nodes,
        large_threshold: request.large_file_threshold,
        large_files: Mutex::new(Vec::new()),
        progress: Box::new(progress),
        started,
        last_report: Mutex::new(Instant::now() - Duration::from_secs(1)),
    };

    ctx.reserve_node();
    let (mut root_node, subdirs) = scan_shallow(&request.root, &ctx);
    ctx.report(&request.root);

    let children: Vec<DirNode> = subdirs
        .par_iter()
        .map(|subdir| scan_dir(subdir, &ctx))
        .collect();

    for child in children {
        root_node.allocated_bytes = root_node
            .allocated_bytes
            .saturating_add(child.allocated_bytes);
        root_node.logical_bytes = root_node.logical_bytes.saturating_add(child.logical_bytes);
        root_node.file_count = root_node.file_count.saturating_add(child.file_count);
        root_node.children.push(child);
    }

    if ctx.cancel.is_cancelled() {
        return Err(ScanError::Cancelled);
    }

    root_node.children.sort_by(|a, b| {
        b.allocated_bytes
            .cmp(&a.allocated_bytes)
            .then_with(|| a.name.cmp(&b.name))
    });

    let mut large_files = ctx
        .large_files
        .lock()
        .map(|files| files.clone())
        .unwrap_or_default();
    large_files.sort_by_key(|file| std::cmp::Reverse(file.allocated_bytes));
    large_files.truncate(request.top_large_files);

    let volume = request
        .root
        .to_string_lossy()
        .chars()
        .next()
        .unwrap_or('C')
        .to_ascii_uppercase();
    let free_bytes = super::enumerate_volumes()
        .into_iter()
        .find(|volume_info| volume_info.letter == volume)
        .map(|volume_info| volume_info.free_bytes)
        .unwrap_or(0);

    Ok(ScanSnapshot {
        id: uuid::Uuid::new_v4().to_string(),
        volume,
        engine: "walker".into(),
        started_at,
        duration_ms: started.elapsed().as_millis() as u64,
        total_bytes: root_node.logical_bytes,
        allocated_bytes: root_node.allocated_bytes,
        free_bytes,
        file_count: root_node.file_count,
        dir_count: ctx.dirs.load(Ordering::Relaxed),
        error_count: ctx.errors.load(Ordering::Relaxed),
        node_limit_hit: ctx.node_limit_hit.load(Ordering::Relaxed),
        top_dirs: root_node.children,
        large_files,
    })
}
