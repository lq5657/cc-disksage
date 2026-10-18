//! Elevated helper process entry point.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

use disksage_helper::job::{load_job, result_path, write_error_result, write_result};
#[cfg(windows)]
use disksage_helper::run_mft_job;
use disksage_helper::{run_job, run_quick_scan_job};

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let job_path = args
        .iter()
        .position(|argument| argument == "--job")
        .and_then(|index| args.get(index + 1))
        .map(PathBuf::from);

    let Some(job_path) = job_path else {
        std::process::exit(2);
    };

    // The job directory is the directory containing the job file. This keeps
    // the helper working when UAC elevates into a different administrator
    // profile; job contents are still limited to allowlisted rule ids.
    let job_dir = job_path.parent().unwrap_or_else(|| Path::new("."));
    match load_job(&job_path, job_dir) {
        Ok(job) => {
            #[cfg(windows)]
            let result = if job.task == "mft_scan" {
                run_mft_job(&job, &job_path)
            } else if job.task == "quick_scan" {
                run_quick_scan_job(&job, &job_path)
            } else {
                run_job(&job)
            };
            #[cfg(not(windows))]
            let result = if job.task == "quick_scan" {
                run_quick_scan_job(&job, &job_path)
            } else {
                run_job(&job)
            };
            let path = result_path(&job_path);
            if let Err(error) = write_result(&path, &result) {
                write_error_result(&job_path, error);
                std::process::exit(1);
            }
            if result.failed.is_empty() {
                std::process::exit(0);
            } else {
                std::process::exit(1);
            }
        }
        Err(error) => {
            write_error_result(&job_path, error);
            std::process::exit(1);
        }
    }
}
