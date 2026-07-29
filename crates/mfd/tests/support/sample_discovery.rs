use std::io;
use std::path::{Path, PathBuf};

const MAX_SAMPLE_DEPTH: usize = 32;
const MAX_SAMPLE_FILES: usize = 10_000;

pub(crate) fn discover_sample_paths(samples_dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut directories = vec![(samples_dir.to_path_buf(), 0usize)];
    let mut sample_paths = Vec::new();
    while let Some((directory, depth)) = directories.pop() {
        let mut entries = std::fs::read_dir(&directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                if depth >= MAX_SAMPLE_DEPTH {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "sample directory nesting exceeds {MAX_SAMPLE_DEPTH} levels at {}",
                            path.display()
                        ),
                    ));
                }
                directories.push((path, depth + 1));
                continue;
            }
            if file_type.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("mfd"))
            {
                sample_paths.push(path);
                if sample_paths.len() > MAX_SAMPLE_FILES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("sample corpus exceeds {MAX_SAMPLE_FILES} mapping files"),
                    ));
                }
            }
        }
    }
    sample_paths.sort();
    Ok(sample_paths)
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> io::Result<Self> {
            let path = std::env::temp_dir().join(format!(
                "ferrule_mfd_sample_discovery_{}",
                std::process::id()
            ));
            match std::fs::remove_dir_all(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            std::fs::create_dir_all(&path)?;
            Ok(Self(path))
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn recursively_discovers_mfd_files_in_stable_order() -> Result<(), Box<dyn Error>> {
        let directory = TestDir::new()?;
        let nested = directory.0.join("tutorial/part-1");
        std::fs::create_dir_all(&nested)?;
        std::fs::write(directory.0.join("root.mfd"), "")?;
        std::fs::write(nested.join("nested.MFD"), "")?;
        std::fs::write(nested.join("ignored.txt"), "")?;

        let paths = discover_sample_paths(&directory.0)?;
        let relative = paths
            .iter()
            .map(|path| {
                path.strip_prefix(&directory.0)
                    .map(|path| path.to_string_lossy().replace('\\', "/"))
            })
            .collect::<Result<Vec<_>, _>>()?;

        assert_eq!(relative, ["root.mfd", "tutorial/part-1/nested.MFD"]);
        Ok(())
    }
}
