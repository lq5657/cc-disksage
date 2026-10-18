//! NTFS MFT scan. This is read-only and requires an elevated process because it
//! opens the raw volume. The UI never runs this in-process; `disksage-helper`
//! executes it and writes a snapshot for the normal user process.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use disksage_core::{DirNode, LargeFile, ScanSnapshot};
use ntfs_reader::{DefaultPathCache, FileInfo, MftScan, Volume};

use crate::walker::{CancelToken, ScanError, ScanProgress, is_skipped_dir};

const DEFAULT_MAX_NODES: u64 = 2_000_000;
const DEFAULT_LARGE_FILE_BYTES: u64 = 500 * 1024 * 1024;
const DEFAULT_TOP_LARGE_FILES: usize = 200;
const MAX_COLLECTED_LARGE_FILES: usize = 10_000;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const NTFS_METADATA_ROOTS: &[&str] = &[
    "$MFT", "$MFTMirr", "$LogFile", "$Volume", "$AttrDef", "$Bitmap", "$Boot", "$BadClus",
    "$Secure", "$UpCase", "$Extend", "$Quota", "$ObjId", "$Reparse", "$UsnJrnl", "$Deleted",
];

fn is_ntfs_metadata_root(name: &str) -> bool {
    NTFS_METADATA_ROOTS
        .iter()
        .any(|metadata| metadata.eq_ignore_ascii_case(name))
}

#[derive(Debug, Clone)]
pub struct MftScanRequest {
    pub volume: char,
    pub max_nodes: u64,
    pub large_file_threshold: u64,
    pub top_large_files: usize,
}

impl MftScanRequest {
    pub fn new(volume: char) -> Self {
        Self {
            volume: volume.to_ascii_uppercase(),
            max_nodes: DEFAULT_MAX_NODES,
            large_file_threshold: DEFAULT_LARGE_FILE_BYTES,
            top_large_files: DEFAULT_TOP_LARGE_FILES,
        }
    }
}

struct FlatNode {
    name: String,
    path: PathBuf,
    parent: Option<usize>,
    children: Vec<usize>,
    direct_bytes: u64,
    direct_files: u64,
    allocated_bytes: u64,
    logical_bytes: u64,
    file_count: u64,
}

struct TreeBuilder {
    volume: char,
    nodes: Vec<FlatNode>,
    index: HashMap<String, usize>,
    max_nodes: u64,
    node_limit_hit: bool,
    errors: u64,
    files: u64,
    bytes: u64,
    large_threshold: u64,
    large_files: Vec<LargeFile>,
}

impl TreeBuilder {
    fn new(volume: char, max_nodes: u64, large_threshold: u64) -> Self {
        let root_path = PathBuf::from(format!("{}:\\", volume.to_ascii_uppercase()));
        let root = FlatNode {
            name: root_path.display().to_string(),
            path: root_path,
            parent: None,
            children: Vec::new(),
            direct_bytes: 0,
            direct_files: 0,
            allocated_bytes: 0,
            logical_bytes: 0,
            file_count: 0,
        };
        let mut index = HashMap::new();
        index.insert(String::new(), 0);
        Self {
            volume: volume.to_ascii_uppercase(),
            nodes: vec![root],
            index,
            max_nodes,
            node_limit_hit: false,
            errors: 0,
            files: 0,
            bytes: 0,
            large_threshold,
            large_files: Vec::new(),
        }
    }

    fn add_entry(
        &mut self,
        components: Vec<String>,
        is_directory: bool,
        size: u64,
        modified_at: Option<i64>,
        reparse: bool,
    ) {
        if components.is_empty() || reparse {
            return;
        }
        if components.iter().any(|component| is_skipped_dir(component)) {
            return;
        }
        if is_ntfs_metadata_root(&components[0]) {
            return;
        }

        if is_directory {
            let _ = self.ensure_dir(&components);
            return;
        }

        let Some((file_name, parent_components)) = components.split_last() else {
            return;
        };
        let Some(parent) = self.ensure_dir(parent_components) else {
            return;
        };
        self.nodes[parent].direct_bytes = self.nodes[parent].direct_bytes.saturating_add(size);
        self.nodes[parent].direct_files = self.nodes[parent].direct_files.saturating_add(1);
        self.files = self.files.saturating_add(1);
        self.bytes = self.bytes.saturating_add(size);

        if size >= self.large_threshold && self.large_files.len() < MAX_COLLECTED_LARGE_FILES {
            self.large_files.push(LargeFile {
                path: self.nodes[parent].path.join(file_name),
                allocated_bytes: size,
                logical_bytes: size,
                modified_at,
                extension: Path::new(file_name)
                    .extension()
                    .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
                    .unwrap_or_default(),
            });
        }
    }

    fn ensure_dir(&mut self, components: &[String]) -> Option<usize> {
        let mut parent = 0usize;
        let mut key = String::new();
        for (index, component) in components.iter().enumerate() {
            if index > 0 {
                key.push('\\');
            }
            key.push_str(&component.to_lowercase());

            if let Some(&existing) = self.index.get(&key) {
                parent = existing;
                continue;
            }
            if self.nodes.len() as u64 >= self.max_nodes {
                self.node_limit_hit = true;
                return None;
            }

            let path = display_path(self.volume, &components[..=index]);
            let node_index = self.nodes.len();
            self.nodes.push(FlatNode {
                name: component.clone(),
                path,
                parent: Some(parent),
                children: Vec::new(),
                direct_bytes: 0,
                direct_files: 0,
                allocated_bytes: 0,
                logical_bytes: 0,
                file_count: 0,
            });
            self.nodes[parent].children.push(node_index);
            self.index.insert(key.clone(), node_index);
            parent = node_index;
        }
        Some(parent)
    }

    fn finish(
        self,
        started_at: i64,
        duration_ms: u64,
        free_bytes: u64,
        top_large_files: usize,
    ) -> ScanSnapshot {
        let mut nodes = self.nodes;
        for node in &mut nodes {
            node.allocated_bytes = node.direct_bytes;
            node.logical_bytes = node.direct_bytes;
            node.file_count = node.direct_files;
        }
        for index in (1..nodes.len()).rev() {
            let bytes = nodes[index].allocated_bytes;
            let files = nodes[index].file_count;
            if let Some(parent) = nodes[index].parent {
                nodes[parent].allocated_bytes = nodes[parent].allocated_bytes.saturating_add(bytes);
                nodes[parent].logical_bytes = nodes[parent].logical_bytes.saturating_add(bytes);
                nodes[parent].file_count = nodes[parent].file_count.saturating_add(files);
            }
        }

        let sizes = nodes
            .iter()
            .map(|node| node.allocated_bytes)
            .collect::<Vec<_>>();
        for node in &mut nodes {
            node.children
                .sort_by_key(|index| std::cmp::Reverse(sizes[*index]));
        }

        let top_dirs = nodes[0]
            .children
            .iter()
            .map(|index| to_dir_node(&nodes, *index))
            .collect::<Vec<_>>();
        let root = &nodes[0];

        let mut large_files = self.large_files;
        large_files.sort_by_key(|file| std::cmp::Reverse(file.allocated_bytes));
        large_files.truncate(top_large_files);

        ScanSnapshot {
            id: uuid::Uuid::new_v4().to_string(),
            volume: self.volume,
            engine: "mft".into(),
            started_at,
            duration_ms,
            total_bytes: root.logical_bytes,
            allocated_bytes: root.allocated_bytes,
            free_bytes,
            file_count: root.file_count,
            dir_count: nodes.len() as u64,
            error_count: self.errors,
            node_limit_hit: self.node_limit_hit,
            top_dirs,
            large_files,
        }
    }
}

fn to_dir_node(nodes: &[FlatNode], index: usize) -> DirNode {
    let node = &nodes[index];
    DirNode {
        name: node.name.clone(),
        path: node.path.clone(),
        allocated_bytes: node.allocated_bytes,
        logical_bytes: node.logical_bytes,
        file_count: node.file_count,
        children: node
            .children
            .iter()
            .map(|child| to_dir_node(nodes, *child))
            .collect(),
    }
}

fn display_path(volume: char, components: &[String]) -> PathBuf {
    let mut path = PathBuf::from(format!("{}:\\", volume.to_ascii_uppercase()));
    for component in components {
        path.push(component);
    }
    path
}

/// Convert the device-relative path returned by `ntfs-reader`
/// (`\\.\C:\Users\...`) into normal component names.
fn relative_components(path: &Path, volume: char) -> Option<Vec<String>> {
    let text = path.to_string_lossy();
    let volume = volume.to_ascii_uppercase();
    for prefix in [
        format!(r"\\.\{}:", volume),
        format!(r"\\?\{}:", volume),
        format!("{}:", volume),
    ] {
        if text.len() >= prefix.len() && text[..prefix.len()].eq_ignore_ascii_case(&prefix) {
            return split_components(&text[prefix.len()..]);
        }
    }

    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => components.push(name.to_string_lossy().into_owned()),
            Component::ParentDir => return None,
        }
    }
    Some(components)
}

fn split_components(text: &str) -> Option<Vec<String>> {
    let mut components = Vec::new();
    for component in text.split(['\\', '/']) {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            return None;
        }
        components.push(component.to_string());
    }
    Some(components)
}

fn system_time_to_timestamp(time: SystemTime) -> Option<i64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs() as i64)
}

pub fn scan_volume_mft(
    request: MftScanRequest,
    progress: impl Fn(ScanProgress) + Send + Sync + 'static,
    cancel: CancelToken,
) -> Result<ScanSnapshot, ScanError> {
    let started = Instant::now();
    let started_at = system_time_to_timestamp(SystemTime::now()).unwrap_or_default();
    let volume = request.volume.to_ascii_uppercase();
    let volume_path = format!(r"\\.\{}:", volume);
    let ntfs_volume = Volume::new(volume_path.as_str())
        .map_err(|error| ScanError::Mft(format!("无法打开 NTFS 卷：{error}")))?;
    let mut scan = MftScan::new(ntfs_volume)
        .map_err(|error| ScanError::Mft(format!("无法读取 MFT：{error}")))?;

    let mut cache = DefaultPathCache::new();
    let mut builder = TreeBuilder::new(volume, request.max_nodes, request.large_file_threshold);
    let mut last_report = Instant::now() - Duration::from_secs(1);

    while let Some(chunk) = scan
        .next_chunk()
        .map_err(|error| ScanError::Mft(format!("读取 MFT 失败：{error}")))?
    {
        for file in chunk.files() {
            if cancel.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            let info = FileInfo::with_cache(&file, &mut cache);
            if info.is_deleted {
                continue;
            }
            let Some(path) = info.path.as_deref() else {
                builder.errors = builder.errors.saturating_add(1);
                continue;
            };
            let Some(components) = relative_components(path, volume) else {
                builder.errors = builder.errors.saturating_add(1);
                continue;
            };
            let modified_at = info.modified.map(|time| time.unix_timestamp());
            builder.add_entry(
                components,
                info.is_directory,
                info.size,
                modified_at,
                info.file_attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0,
            );

            if last_report.elapsed() >= Duration::from_millis(100) {
                last_report = Instant::now();
                progress(ScanProgress {
                    files: builder.files,
                    dirs: builder.nodes.len() as u64,
                    bytes: builder.bytes,
                    errors: builder.errors,
                    elapsed_ms: started.elapsed().as_millis() as u64,
                    current: path.display().to_string(),
                });
            }
        }
    }

    let free_bytes = crate::enumerate_volumes()
        .into_iter()
        .find(|volume_info| volume_info.letter == volume)
        .map(|volume_info| volume_info.free_bytes)
        .unwrap_or(0);

    Ok(builder.finish(
        started_at,
        started.elapsed().as_millis() as u64,
        free_bytes,
        request.top_large_files,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add_file(builder: &mut TreeBuilder, path: &[&str], size: u64) {
        builder.add_entry(
            path.iter().map(|part| part.to_string()).collect(),
            false,
            size,
            None,
            false,
        );
    }

    fn add_dir(builder: &mut TreeBuilder, path: &[&str]) {
        builder.add_entry(
            path.iter().map(|part| part.to_string()).collect(),
            true,
            0,
            None,
            false,
        );
    }

    #[test]
    fn parses_device_paths() {
        assert_eq!(
            relative_components(Path::new(r"\\.\C:\Users\alice\file.bin"), 'C').unwrap(),
            vec!["Users", "alice", "file.bin"]
        );
        assert_eq!(
            relative_components(Path::new(r"\\?\D:\data\a.txt"), 'D').unwrap(),
            vec!["data", "a.txt"]
        );
        assert_eq!(
            relative_components(Path::new(r"C:\x\y"), 'C').unwrap(),
            vec!["x", "y"]
        );
    }

    #[test]
    fn aggregates_directory_sizes_and_files() {
        let mut builder = TreeBuilder::new('C', 100, 100);
        add_dir(&mut builder, &["Users"]);
        add_dir(&mut builder, &["Users", "alice"]);
        add_file(&mut builder, &["Users", "alice", "a.bin"], 100);
        add_file(&mut builder, &["Users", "alice", "b.bin"], 50);
        add_file(&mut builder, &["Windows", "x.sys"], 25);

        let snapshot = builder.finish(0, 10, 123, 200);
        assert_eq!(snapshot.engine, "mft");
        assert_eq!(snapshot.file_count, 3);
        assert_eq!(snapshot.allocated_bytes, 175);
        assert_eq!(snapshot.total_bytes, 175);
        assert_eq!(snapshot.free_bytes, 123);
        assert_eq!(snapshot.top_dirs.len(), 2);
        assert_eq!(snapshot.top_dirs[0].name, "Users");
        assert_eq!(snapshot.top_dirs[0].allocated_bytes, 150);
        assert_eq!(snapshot.top_dirs[0].file_count, 2);
        assert_eq!(snapshot.top_dirs[0].children[0].name, "alice");
        assert_eq!(snapshot.large_files.len(), 1);
        assert_eq!(snapshot.large_files[0].allocated_bytes, 100);
    }

    #[test]
    fn merges_case_variants_and_skips_protected_or_reparse_entries() {
        let mut builder = TreeBuilder::new('C', 100, 1000);
        add_dir(&mut builder, &["Users"]);
        add_dir(&mut builder, &["users"]);
        add_file(&mut builder, &["Users", "Alice", "a.bin"], 10);
        add_file(&mut builder, &["users", "alice", "b.bin"], 20);
        add_file(&mut builder, &["WindowsApps", "huge.bin"], 100);
        add_file(&mut builder, &["$Recycle.Bin", "old.bin"], 100);
        add_file(&mut builder, &["System Volume Information", "x"], 100);
        builder.add_entry(vec!["link".into(), "x".into()], false, 100, None, true);

        let snapshot = builder.finish(0, 10, 0, 200);
        assert_eq!(snapshot.file_count, 2);
        assert_eq!(snapshot.allocated_bytes, 30);
        assert_eq!(snapshot.top_dirs.len(), 1);
        assert_eq!(snapshot.top_dirs[0].children.len(), 1);
        assert_eq!(snapshot.top_dirs[0].children[0].file_count, 2);
        assert!(snapshot.top_dirs[0].children[0].children.is_empty());
    }

    #[test]
    fn keeps_recovery_staging_but_skips_ntfs_metadata() {
        let mut builder = TreeBuilder::new('C', 100, 1000);
        add_file(&mut builder, &["$WinREAgent", "Backup", "winre.wim"], 100);
        add_file(&mut builder, &["$Extend", "$UsnJrnl", "x"], 100);
        add_file(&mut builder, &["$MFT"], 100);
        let snapshot = builder.finish(0, 10, 0, 200);
        assert_eq!(snapshot.file_count, 1);
        assert_eq!(snapshot.allocated_bytes, 100);
        assert_eq!(snapshot.top_dirs.len(), 1);
        assert_eq!(snapshot.top_dirs[0].name, "$WinREAgent");
    }

    #[test]
    fn stops_at_node_cap() {
        let mut builder = TreeBuilder::new('C', 3, 1000);
        add_file(&mut builder, &["a", "b", "c", "d", "file.bin"], 10);
        add_file(&mut builder, &["e", "f", "file.bin"], 20);
        let snapshot = builder.finish(0, 10, 0, 200);
        assert!(snapshot.node_limit_hit);
        assert!(snapshot.dir_count <= 3);
        assert_eq!(snapshot.file_count, 0);
    }
}
