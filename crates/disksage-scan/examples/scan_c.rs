use disksage_scan::{CancelToken, ScanProgress, ScanRequest, enumerate_volumes, scan_volume};

fn main() {
    println!("volumes: {:#?}", enumerate_volumes());

    let started = std::time::Instant::now();
    let snapshot = scan_volume(
        ScanRequest::new(r"C:\"),
        |progress: ScanProgress| {
            if progress.files.is_multiple_of(100_000) {
                eprintln!(
                    "progress: {} files, {} dirs, {:.1} GB, {} errors, {}",
                    progress.files,
                    progress.dirs,
                    progress.bytes as f64 / 1024.0 / 1024.0 / 1024.0,
                    progress.errors,
                    progress.current
                );
            }
        },
        CancelToken::new(),
    )
    .expect("scan failed");

    println!(
        "done in {:.1}s: {} files, {} dirs, {:.2} GB, {} errors, node_limit_hit={}",
        started.elapsed().as_secs_f64(),
        snapshot.file_count,
        snapshot.dir_count,
        snapshot.allocated_bytes as f64 / 1024.0 / 1024.0 / 1024.0,
        snapshot.error_count,
        snapshot.node_limit_hit
    );
    println!("top dirs:");
    for dir in snapshot.top_dirs.iter().take(15) {
        println!(
            "  {:>10.2} GB  {}",
            dir.allocated_bytes as f64 / 1024.0 / 1024.0 / 1024.0,
            dir.path.display()
        );
    }
    println!("large files: {}", snapshot.large_files.len());
    for file in snapshot.large_files.iter().take(10) {
        println!(
            "  {:>10.2} GB  {}",
            file.allocated_bytes as f64 / 1024.0 / 1024.0 / 1024.0,
            file.path.display()
        );
    }
}
