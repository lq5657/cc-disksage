//! Compressed sidecar storage for full scan trees.
//!
//! SQLite keeps a small summary for history and the dashboard. Deep treemap
//! drill-down needs the complete directory tree, so the full snapshot is
//! written to `<data>/scans/<id>.json.gz`. Only the newest few full snapshots
//! are retained; older scans fall back to their summary.

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use disksage_core::ScanSnapshot;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

const MAX_FULL_SNAPSHOTS: usize = 5;
const SUFFIX: &str = ".json.gz";

fn directory() -> PathBuf {
    disksage_core::paths::data_root().join("scans")
}

fn path_in(directory: &Path, id: &str) -> PathBuf {
    directory.join(format!("{id}{SUFFIX}"))
}

pub fn save(snapshot: &ScanSnapshot) -> Result<(), String> {
    save_in(&directory(), snapshot)
}

pub fn load(id: &str) -> Result<Option<ScanSnapshot>, String> {
    load_from(&directory(), id)
}

fn save_in(directory: &Path, snapshot: &ScanSnapshot) -> Result<(), String> {
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;

    let target = path_in(directory, &snapshot.id);
    let temporary = target.with_extension("json.gz.tmp");
    {
        let file = File::create(&temporary).map_err(|error| error.to_string())?;
        let mut encoder = GzEncoder::new(BufWriter::new(file), Compression::default());
        serde_json::to_writer(&mut encoder, snapshot).map_err(|error| error.to_string())?;
        let mut writer = encoder.finish().map_err(|error| error.to_string())?;
        writer.flush().map_err(|error| error.to_string())?;
    }

    if target.exists() {
        fs::remove_file(&target).map_err(|error| error.to_string())?;
    }
    fs::rename(&temporary, &target).map_err(|error| error.to_string())?;
    prune_in(directory, MAX_FULL_SNAPSHOTS);
    Ok(())
}

fn load_from(directory: &Path, id: &str) -> Result<Option<ScanSnapshot>, String> {
    let path = path_in(directory, id);
    if !path.is_file() {
        return Ok(None);
    }
    let file = File::open(&path).map_err(|error| error.to_string())?;
    let reader = GzDecoder::new(BufReader::new(file));
    let snapshot = serde_json::from_reader(reader).map_err(|error| error.to_string())?;
    Ok(Some(snapshot))
}

fn prune_in(directory: &Path, keep: usize) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };

    let mut files = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(SUFFIX))
        })
        .map(|path| {
            let modified = fs::metadata(&path).and_then(|meta| meta.modified()).ok();
            (path, modified)
        })
        .collect::<Vec<_>>();

    files.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| right.0.cmp(&left.0)));
    for (path, _) in files.into_iter().skip(keep) {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use disksage_core::{DirNode, LargeFile};

    use super::*;

    fn snapshot(id: &str) -> ScanSnapshot {
        ScanSnapshot {
            id: id.into(),
            volume: 'C',
            engine: "walker".into(),
            started_at: 0,
            duration_ms: 0,
            total_bytes: 10,
            allocated_bytes: 10,
            free_bytes: 0,
            file_count: 1,
            dir_count: 1,
            error_count: 0,
            node_limit_hit: false,
            top_dirs: vec![DirNode {
                name: "Users".into(),
                path: PathBuf::from(r"C:\Users"),
                allocated_bytes: 10,
                logical_bytes: 10,
                file_count: 1,
                children: vec![DirNode {
                    name: "alice".into(),
                    path: PathBuf::from(r"C:\Users\alice"),
                    allocated_bytes: 10,
                    logical_bytes: 10,
                    file_count: 1,
                    children: Vec::new(),
                }],
            }],
            large_files: vec![LargeFile {
                path: PathBuf::from(r"C:\Users\alice\big.bin"),
                allocated_bytes: 10,
                logical_bytes: 10,
                modified_at: Some(0),
                extension: "bin".into(),
            }],
        }
    }

    #[test]
    fn roundtrips_full_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let original = snapshot("roundtrip");
        save_in(directory.path(), &original).unwrap();

        let loaded = load_from(directory.path(), "roundtrip").unwrap().unwrap();
        assert_eq!(loaded.id, "roundtrip");
        assert_eq!(
            loaded.top_dirs[0].children[0].path,
            PathBuf::from(r"C:\Users\alice")
        );
        assert_eq!(loaded.large_files.len(), 1);
        assert!(load_from(directory.path(), "missing").unwrap().is_none());
    }

    #[test]
    fn prunes_old_snapshots() {
        let directory = tempfile::tempdir().unwrap();
        for index in 0..7 {
            save_in(directory.path(), &snapshot(&format!("scan-{index}"))).unwrap();
            // Ensure distinct modified times on filesystems with coarse timestamps.
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let count = fs::read_dir(directory.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(SUFFIX))
            .count();
        assert_eq!(count, MAX_FULL_SNAPSHOTS);
    }
}
