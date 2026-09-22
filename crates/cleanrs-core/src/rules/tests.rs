use super::non_empty_target;
use crate::model::CleanMethod;
use std::fs;
use tempfile::tempdir;

#[test]
fn non_empty_target_omits_empty_directories() {
    let root = tempdir().expect("temporary directory");
    let empty = root.path().join("empty-cache");
    fs::create_dir(&empty).expect("empty directory");

    let target = non_empty_target(empty, "empty cache", CleanMethod::TrashPath)
        .expect("target scan should succeed");

    assert!(target.is_none());
}

#[test]
fn non_empty_target_reports_reclaimable_directories() {
    let root = tempdir().expect("temporary directory");
    let cache = root.path().join("cache");
    fs::create_dir(&cache).expect("cache directory");
    fs::write(cache.join("artifact.bin"), b"reclaim me").expect("cache file");

    let target = non_empty_target(cache.clone(), "cache", CleanMethod::TrashPath)
        .expect("target scan should succeed")
        .expect("non-empty directory should become a target");

    assert_eq!(target.path, cache);
    assert_eq!(target.size_bytes, 10);
}
