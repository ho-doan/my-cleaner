use anyhow::Result;
use jwalk::WalkDir;
use rayon::prelude::*;
use std::path::Path;

/// Calculate the size of a file or directory.
///
/// jwalk parallelizes traversal and rayon parallelizes the metadata pass. A
/// symlink is never followed into another tree.
pub fn dir_size(path: &Path) -> Result<u64> {
    if !path.exists() {
        return Ok(0);
    }

    if path.is_file() {
        return Ok(std::fs::symlink_metadata(path)?.len());
    }

    let entries = WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let size = entries
        .par_iter()
        .filter_map(|entry| std::fs::symlink_metadata(entry.path()).ok())
        .filter(|metadata| metadata.file_type().is_file())
        .map(|metadata| metadata.len())
        .sum();

    Ok(size)
}

#[cfg(test)]
mod tests {
    use super::dir_size;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn calculates_nested_file_sizes() {
        let root = tempdir().expect("temp directory");
        fs::write(root.path().join("one.txt"), b"12345").expect("write first file");
        fs::create_dir(root.path().join("nested")).expect("create nested directory");
        fs::write(root.path().join("nested/two.txt"), b"1234567").expect("write second file");

        assert_eq!(dir_size(root.path()).expect("directory size"), 12);
    }

    #[test]
    fn missing_paths_have_zero_size() {
        let root = tempdir().expect("temp directory");
        assert_eq!(
            dir_size(&root.path().join("missing")).expect("directory size"),
            0
        );
    }
}
