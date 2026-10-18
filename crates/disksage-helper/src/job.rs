use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use disksage_core::CleanupResult;
use serde::{Deserialize, Serialize};

pub const JOB_TTL_SECONDS: i64 = 900;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelperJob {
    pub task: String,
    pub job_id: String,
    pub nonce: String,
    pub expires_at: i64,
    pub rule_ids: Vec<String>,
}

pub fn jobs_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("cc-disksage")
        .join("jobs")
}

pub fn result_path(job_path: &Path) -> PathBuf {
    job_path.with_extension("result.json")
}

pub fn load_job(job_path: &Path, allowed_dir: &Path) -> Result<HelperJob, String> {
    let allowed = allowed_dir
        .canonicalize()
        .map_err(|error| format!("cannot resolve jobs directory: {error}"))?;
    let path = job_path
        .canonicalize()
        .map_err(|error| format!("cannot resolve job file: {error}"))?;

    if path.parent() != Some(allowed.as_path()) {
        return Err("job file must be directly inside the jobs directory".into());
    }

    let text = fs::read_to_string(&path).map_err(|error| format!("cannot read job: {error}"))?;
    let job: HelperJob =
        serde_json::from_str(&text).map_err(|error| format!("invalid job JSON: {error}"))?;

    if job.task != "clean" {
        return Err(format!("unsupported task: {}", job.task));
    }
    if job.job_id.is_empty() || job.job_id.len() > 128 {
        return Err("invalid job_id".into());
    }
    if job.nonce.len() < 16 || job.nonce.len() > 128 {
        return Err("invalid nonce".into());
    }
    let now = Utc::now().timestamp();
    if job.expires_at < now {
        return Err("job has expired".into());
    }
    if job.expires_at > now + JOB_TTL_SECONDS {
        return Err("job expiry is too far in the future".into());
    }
    if job.rule_ids.is_empty() || job.rule_ids.len() > 100 {
        return Err("invalid rule_ids count".into());
    }
    let unique = job.rule_ids.iter().collect::<HashSet<_>>();
    if unique.len() != job.rule_ids.len() {
        return Err("duplicate rule ids in job".into());
    }
    for rule_id in &job.rule_ids {
        if rule_id.is_empty()
            || !rule_id.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
            })
        {
            return Err(format!("invalid rule id: {rule_id}"));
        }
    }

    Ok(job)
}

pub fn write_result(path: &Path, result: &CleanupResult) -> Result<(), String> {
    let text = serde_json::to_string_pretty(result).map_err(|error| error.to_string())?;
    fs::write(path, text).map_err(|error| format!("cannot write result: {error}"))
}

pub fn write_error_result(job_path: &Path, message: String) {
    let job_id = job_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".into());
    let result = CleanupResult {
        job_id,
        cancelled: false,
        notes: vec![message],
        ..CleanupResult::default()
    };
    let _ = write_result(&result_path(job_path), &result);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_job(dir: &Path) -> PathBuf {
        let path = dir.join("job-1.json");
        let job = HelperJob {
            task: "clean".into(),
            job_id: "job-1".into(),
            nonce: "0123456789abcdef".into(),
            expires_at: Utc::now().timestamp() + 300,
            rule_ids: vec!["system.windows_temp".into()],
        };
        fs::write(&path, serde_json::to_string(&job).unwrap()).unwrap();
        path
    }

    #[test]
    fn accepts_valid_job() {
        let temp = tempfile::tempdir().unwrap();
        let path = valid_job(temp.path());
        let job = load_job(&path, temp.path()).unwrap();
        assert_eq!(job.job_id, "job-1");
    }

    #[test]
    fn rejects_expired_job() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("expired.json");
        let job = HelperJob {
            task: "clean".into(),
            job_id: "expired".into(),
            nonce: "0123456789abcdef".into(),
            expires_at: Utc::now().timestamp() - 1,
            rule_ids: vec!["system.windows_temp".into()],
        };
        fs::write(&path, serde_json::to_string(&job).unwrap()).unwrap();
        assert!(load_job(&path, temp.path()).is_err());
    }

    #[test]
    fn rejects_job_outside_directory() {
        let temp = tempfile::tempdir().unwrap();
        let allowed = temp.path().join("jobs");
        fs::create_dir(&allowed).unwrap();
        let outside = temp.path().join("outside.json");
        fs::write(&outside, "{}").unwrap();
        assert!(load_job(&outside, &allowed).is_err());
    }

    #[test]
    fn rejects_duplicate_rules() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("duplicate.json");
        let job = HelperJob {
            task: "clean".into(),
            job_id: "duplicate".into(),
            nonce: "0123456789abcdef".into(),
            expires_at: Utc::now().timestamp() + 300,
            rule_ids: vec!["system.windows_temp".into(), "system.windows_temp".into()],
        };
        fs::write(&path, serde_json::to_string(&job).unwrap()).unwrap();
        assert!(load_job(&path, temp.path()).is_err());
    }
}
