use disksage_scan::{CancelToken, ScanError, ScanRequest, scan_volume};

#[test]
fn aggregates_files_and_subdirectories() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::write(root.join("top.txt"), b"hello").unwrap();
    std::fs::create_dir(root.join("nested")).unwrap();
    std::fs::write(root.join("nested").join("child.bin"), vec![0u8; 10]).unwrap();

    let mut request = ScanRequest::new(root);
    request.large_file_threshold = 1;
    request.top_large_files = 10;

    let snapshot = scan_volume(request, |_| {}, CancelToken::new()).unwrap();

    assert_eq!(snapshot.file_count, 2);
    assert_eq!(snapshot.allocated_bytes, 15);
    assert_eq!(snapshot.top_dirs.len(), 1);
    assert_eq!(snapshot.top_dirs[0].name, "nested");
    assert_eq!(snapshot.top_dirs[0].file_count, 1);
    assert_eq!(snapshot.large_files.len(), 2);
}

#[test]
fn respects_cancellation() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("file.txt"), b"data").unwrap();

    let cancel = CancelToken::new();
    cancel.cancel();

    let error = scan_volume(ScanRequest::new(temp.path()), |_| {}, cancel).unwrap_err();
    assert!(matches!(error, ScanError::Cancelled));
}

#[test]
fn report_progress_at_least_once_for_small_tree() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("file.txt"), b"data").unwrap();

    let reports = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = reports.clone();
    let snapshot = scan_volume(
        ScanRequest::new(temp.path()),
        move |_progress| {
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        },
        CancelToken::new(),
    )
    .unwrap();

    assert_eq!(snapshot.file_count, 1);
    assert!(
        reports.load(std::sync::atomic::Ordering::Relaxed) > 0,
        "progress callback should fire at least once"
    );
}
