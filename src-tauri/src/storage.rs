//! Local SQLite persistence for scan snapshots and duplicate results.
//! This store never contains file contents, LLM payloads, or API keys.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use disksage_core::ScanSnapshot;
use disksage_scan::DuplicateScanResult;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

const MAX_SCANS: usize = 20;
const MAX_DUPLICATE_SCANS: usize = 20;

#[derive(Debug, Clone, Serialize)]
pub struct ScanHistoryEntry {
    pub id: String,
    pub volume: String,
    pub engine: String,
    pub started_at: i64,
    pub duration_ms: u64,
    pub file_count: u64,
    pub dir_count: u64,
    pub allocated_bytes: u64,
    pub free_bytes: u64,
    pub error_count: u64,
    pub node_limit_hit: bool,
}

pub struct Storage {
    connection: Mutex<Connection>,
}

impl Storage {
    pub fn default_path() -> PathBuf {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("cc-disksage")
            .join("disksage.db")
    }

    pub fn open_default() -> Result<Self, String> {
        let path = Self::default_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        Self::open(&path)
    }

    pub fn open(path: &Path) -> Result<Self, String> {
        let connection = Connection::open(path).map_err(|error| error.to_string())?;
        Self::initialize(connection)
    }

    pub fn in_memory() -> Result<Self, String> {
        let connection = Connection::open_in_memory().map_err(|error| error.to_string())?;
        Self::initialize(connection)
    }

    fn initialize(connection: Connection) -> Result<Self, String> {
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 PRAGMA synchronous=NORMAL;
                 PRAGMA busy_timeout=5000;
                 CREATE TABLE IF NOT EXISTS scans (
                     id TEXT PRIMARY KEY,
                     volume TEXT NOT NULL,
                     engine TEXT NOT NULL,
                     started_at INTEGER NOT NULL,
                     duration_ms INTEGER NOT NULL,
                     total_bytes INTEGER NOT NULL,
                     allocated_bytes INTEGER NOT NULL,
                     free_bytes INTEGER NOT NULL,
                     file_count INTEGER NOT NULL,
                     dir_count INTEGER NOT NULL,
                     error_count INTEGER NOT NULL,
                     node_limit_hit INTEGER NOT NULL,
                     summary_json TEXT NOT NULL,
                     created_at INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS scans_started_idx ON scans(started_at DESC);
                 CREATE TABLE IF NOT EXISTS duplicate_scans (
                     scan_id TEXT PRIMARY KEY,
                     total_wasted_bytes INTEGER NOT NULL,
                     group_count INTEGER NOT NULL,
                     files_scanned INTEGER NOT NULL,
                     duration_ms INTEGER NOT NULL,
                     result_json TEXT NOT NULL,
                     created_at INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS duplicate_scans_started_idx
                     ON duplicate_scans(created_at DESC);
                 PRAGMA user_version=1;",
            )
            .map_err(|error| error.to_string())?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, String> {
        self.connection
            .lock()
            .map_err(|_| "storage lock poisoned".to_string())
    }

    pub fn save_scan(&self, snapshot: &ScanSnapshot) -> Result<(), String> {
        let json = serde_json::to_string(snapshot).map_err(|error| error.to_string())?;
        let now = unix_timestamp();
        let connection = self.lock()?;
        connection
            .execute(
                "INSERT INTO scans
                 (id, volume, engine, started_at, duration_ms, total_bytes, allocated_bytes,
                  free_bytes, file_count, dir_count, error_count, node_limit_hit, summary_json,
                  created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                 ON CONFLICT(id) DO UPDATE SET
                   volume=excluded.volume, engine=excluded.engine,
                   started_at=excluded.started_at, duration_ms=excluded.duration_ms,
                   total_bytes=excluded.total_bytes, allocated_bytes=excluded.allocated_bytes,
                   free_bytes=excluded.free_bytes, file_count=excluded.file_count,
                   dir_count=excluded.dir_count, error_count=excluded.error_count,
                   node_limit_hit=excluded.node_limit_hit, summary_json=excluded.summary_json,
                   created_at=excluded.created_at",
                params![
                    snapshot.id,
                    snapshot.volume.to_string(),
                    snapshot.engine,
                    snapshot.started_at,
                    snapshot.duration_ms as i64,
                    snapshot.total_bytes as i64,
                    snapshot.allocated_bytes as i64,
                    snapshot.free_bytes as i64,
                    snapshot.file_count as i64,
                    snapshot.dir_count as i64,
                    snapshot.error_count as i64,
                    i64::from(snapshot.node_limit_hit),
                    json,
                    now,
                ],
            )
            .map_err(|error| error.to_string())?;
        connection
            .execute(
                "DELETE FROM scans WHERE id NOT IN (
                     SELECT id FROM scans ORDER BY created_at DESC, started_at DESC LIMIT ?1
                 )",
                params![MAX_SCANS as i64],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn load_scan(&self, id: &str) -> Result<Option<ScanSnapshot>, String> {
        let connection = self.lock()?;
        let json = connection
            .query_row(
                "SELECT summary_json FROM scans WHERE id = ?1",
                params![id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        json.map(|text| serde_json::from_str(&text).map_err(|error| error.to_string()))
            .transpose()
    }

    pub fn latest_scan(&self) -> Result<Option<ScanSnapshot>, String> {
        let connection = self.lock()?;
        let json = connection
            .query_row(
                "SELECT summary_json FROM scans ORDER BY started_at DESC, created_at DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        json.map(|text| serde_json::from_str(&text).map_err(|error| error.to_string()))
            .transpose()
    }

    pub fn scan_history(&self, limit: usize) -> Result<Vec<ScanHistoryEntry>, String> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare(
                "SELECT id, volume, engine, started_at, duration_ms, file_count, dir_count,
                        allocated_bytes, free_bytes, error_count, node_limit_hit
                 FROM scans ORDER BY started_at DESC, created_at DESC LIMIT ?1",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![limit.clamp(1, 100) as i64], |row| {
                Ok(ScanHistoryEntry {
                    id: row.get(0)?,
                    volume: row.get(1)?,
                    engine: row.get(2)?,
                    started_at: row.get(3)?,
                    duration_ms: row.get::<_, i64>(4)? as u64,
                    file_count: row.get::<_, i64>(5)? as u64,
                    dir_count: row.get::<_, i64>(6)? as u64,
                    allocated_bytes: row.get::<_, i64>(7)? as u64,
                    free_bytes: row.get::<_, i64>(8)? as u64,
                    error_count: row.get::<_, i64>(9)? as u64,
                    node_limit_hit: row.get::<_, i64>(10)? != 0,
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn save_duplicate_scan(&self, result: &DuplicateScanResult) -> Result<(), String> {
        let json = serde_json::to_string(result).map_err(|error| error.to_string())?;
        let now = unix_timestamp();
        let connection = self.lock()?;
        connection
            .execute(
                "INSERT INTO duplicate_scans
                 (scan_id, total_wasted_bytes, group_count, files_scanned, duration_ms, result_json,
                  created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(scan_id) DO UPDATE SET
                   total_wasted_bytes=excluded.total_wasted_bytes,
                   group_count=excluded.group_count,
                   files_scanned=excluded.files_scanned,
                   duration_ms=excluded.duration_ms,
                   result_json=excluded.result_json,
                   created_at=excluded.created_at",
                params![
                    result.scan_id,
                    result.total_wasted_bytes as i64,
                    result.groups.len() as i64,
                    result.files_scanned as i64,
                    result.duration_ms as i64,
                    json,
                    now,
                ],
            )
            .map_err(|error| error.to_string())?;
        connection
            .execute(
                "DELETE FROM duplicate_scans WHERE scan_id NOT IN (
                     SELECT scan_id FROM duplicate_scans ORDER BY created_at DESC LIMIT ?1
                 )",
                params![MAX_DUPLICATE_SCANS as i64],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn load_duplicate_scan(
        &self,
        scan_id: &str,
    ) -> Result<Option<DuplicateScanResult>, String> {
        let connection = self.lock()?;
        let json = connection
            .query_row(
                "SELECT result_json FROM duplicate_scans WHERE scan_id = ?1",
                params![scan_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        json.map(|text| serde_json::from_str(&text).map_err(|error| error.to_string()))
            .transpose()
    }

    pub fn latest_duplicate_scan(&self) -> Result<Option<DuplicateScanResult>, String> {
        let connection = self.lock()?;
        let json = connection
            .query_row(
                "SELECT result_json FROM duplicate_scans ORDER BY created_at DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        json.map(|text| serde_json::from_str(&text).map_err(|error| error.to_string()))
            .transpose()
    }
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use disksage_core::{LargeFile, ScanSnapshot};

    fn snapshot(id: &str, started_at: i64) -> ScanSnapshot {
        ScanSnapshot {
            id: id.into(),
            volume: 'C',
            engine: "walker".into(),
            started_at,
            duration_ms: 10,
            total_bytes: 100,
            allocated_bytes: 100,
            free_bytes: 50,
            file_count: 2,
            dir_count: 1,
            error_count: 0,
            node_limit_hit: false,
            top_dirs: Vec::new(),
            large_files: vec![LargeFile {
                path: std::path::PathBuf::from(r"C:\big.bin"),
                allocated_bytes: 50,
                logical_bytes: 50,
                modified_at: None,
                extension: "bin".into(),
            }],
        }
    }

    fn duplicate_result(id: &str) -> DuplicateScanResult {
        DuplicateScanResult {
            scan_id: id.into(),
            roots: vec![std::path::PathBuf::from(r"C:\Users\test\Downloads")],
            groups: Vec::new(),
            total_wasted_bytes: 0,
            files_scanned: 3,
            candidate_files: 2,
            hashed_files: 1,
            error_count: 0,
            duration_ms: 5,
            truncated: false,
            min_size: 1024,
        }
    }

    #[test]
    fn round_trips_scans_and_history() {
        let storage = Storage::in_memory().unwrap();
        storage.save_scan(&snapshot("older", 10)).unwrap();
        storage.save_scan(&snapshot("newer", 20)).unwrap();
        assert_eq!(storage.load_scan("older").unwrap().unwrap().file_count, 2);
        assert_eq!(storage.latest_scan().unwrap().unwrap().id, "newer");
        let history = storage.scan_history(10).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].id, "newer");
        assert_eq!(history[0].engine, "walker");
    }

    #[test]
    fn round_trips_duplicate_results() {
        let storage = Storage::in_memory().unwrap();
        storage
            .save_duplicate_scan(&duplicate_result("dup-1"))
            .unwrap();
        let loaded = storage.load_duplicate_scan("dup-1").unwrap().unwrap();
        assert_eq!(loaded.files_scanned, 3);
        assert_eq!(
            storage.latest_duplicate_scan().unwrap().unwrap().scan_id,
            "dup-1"
        );
    }

    #[test]
    fn persists_to_a_file_across_reopen() {
        let dir = std::env::temp_dir().join(format!("ccds-storage-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.db");
        {
            let storage = Storage::open(&path).unwrap();
            storage.save_scan(&snapshot("persisted", 30)).unwrap();
            storage
                .save_duplicate_scan(&duplicate_result("dup-file"))
                .unwrap();
        }
        let storage = Storage::open(&path).unwrap();
        assert_eq!(storage.latest_scan().unwrap().unwrap().id, "persisted");
        assert_eq!(
            storage.latest_duplicate_scan().unwrap().unwrap().scan_id,
            "dup-file"
        );
        drop(storage);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
