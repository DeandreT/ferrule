use std::collections::BTreeSet;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_ARTIFACT_ID: AtomicU64 = AtomicU64::new(0);

struct StagedArtifact {
    temporary: PathBuf,
    destination: PathBuf,
}

impl Drop for StagedArtifact {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.temporary);
    }
}

struct CreatedDirectories {
    paths: Vec<PathBuf>,
    keep: bool,
}

impl CreatedDirectories {
    fn new() -> Self {
        Self {
            paths: Vec::new(),
            keep: false,
        }
    }

    fn keep(&mut self) {
        self.keep = true;
    }
}

impl Drop for CreatedDirectories {
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        for path in self.paths.iter().rev() {
            let _ = std::fs::remove_dir(path);
        }
    }
}

pub(super) fn write_artifacts(
    output_directory: &Path,
    artifacts: Vec<(PathBuf, String)>,
) -> io::Result<()> {
    let lexical_output = lexical_absolute(output_directory)?;
    std::fs::create_dir_all(&lexical_output)?;
    let output_directory = std::fs::canonicalize(&lexical_output)?;
    if !output_directory.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            format!(
                "artifact output path is not a directory: {}",
                output_directory.display()
            ),
        ));
    }
    let artifacts = artifacts
        .into_iter()
        .map(|(destination, contents)| {
            let lexical_destination = lexical_absolute(&destination)?;
            let relative = lexical_destination
                .strip_prefix(&lexical_output)
                .map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!(
                            "artifact destination escapes the output directory: {}",
                            destination.display()
                        ),
                    )
                })?;
            if relative.as_os_str().is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "artifact destination names the output directory: {}",
                        destination.display()
                    ),
                ));
            }
            Ok((output_directory.join(relative), contents))
        })
        .collect::<io::Result<Vec<_>>>()?;

    let mut destinations = BTreeSet::new();
    let mut parents = BTreeSet::from([output_directory.clone()]);
    for (destination, _) in &artifacts {
        if !destinations.insert(destination.clone()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("duplicate artifact destination: {}", destination.display()),
            ));
        }
        match std::fs::symlink_metadata(destination) {
            Ok(metadata) if metadata.file_type().is_dir() => {
                return Err(io::Error::new(
                    io::ErrorKind::IsADirectory,
                    format!(
                        "artifact destination is a directory: {}",
                        destination.display()
                    ),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        parents.insert(
            destination
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf(),
        );
    }
    for destination in &destinations {
        let mut ancestor = destination.parent();
        while let Some(path) = ancestor.filter(|path| *path != output_directory) {
            if destinations.contains(path) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "artifact destination `{}` is also a directory prefix of `{}`",
                        path.display(),
                        destination.display()
                    ),
                ));
            }
            ancestor = path.parent();
        }
    }

    let mut created = CreatedDirectories::new();
    for parent in parents {
        ensure_directory_tree(&output_directory, &parent, &mut created)?;
    }

    let mut staged = Vec::with_capacity(artifacts.len());
    for (destination, contents) in artifacts {
        staged.push(stage_artifact(destination, contents.as_bytes())?);
    }
    for artifact in &staged {
        std::fs::rename(&artifact.temporary, &artifact.destination)?;
    }
    created.keep();
    Ok(())
}

fn lexical_absolute(path: &Path) -> io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("path escapes its filesystem root: {}", path.display()),
                    ));
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    Ok(normalized)
}

fn ensure_directory_tree(
    output_directory: &Path,
    path: &Path,
    created: &mut CreatedDirectories,
) -> io::Result<()> {
    let relative = path.strip_prefix(output_directory).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "artifact directory escapes the output directory: {}",
                path.display()
            ),
        )
    })?;
    let mut current = output_directory.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("artifact directory is not canonical: {}", path.display()),
            ));
        };
        current.push(part);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "artifact directory contains a symbolic link: {}",
                        current.display()
                    ),
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(io::Error::new(
                    io::ErrorKind::NotADirectory,
                    format!(
                        "artifact directory ancestor is not a directory: {}",
                        current.display()
                    ),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                std::fs::create_dir(&current)?;
                created.paths.push(current.clone());
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn stage_artifact(destination: PathBuf, contents: &[u8]) -> io::Result<StagedArtifact> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("mapping");
    loop {
        let id = TEMP_ARTIFACT_ID.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".{file_name}.ferrule-{}-{id}.tmp",
            std::process::id()
        ));
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary);
        let mut file = match file {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let artifact = StagedArtifact {
            temporary,
            destination,
        };
        std::io::Write::write_all(&mut file, contents)?;
        file.sync_all()?;
        return Ok(artifact);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "ferrule_artifact_{label}_{}_{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap_or_else(|error| {
                panic!("temporary artifact directory should be created: {error}")
            });
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn nested_artifacts_reject_symlinked_parent_without_writing_outside() {
        let output = TempDir::new("output");
        let outside = TempDir::new("outside");
        let bundle = output.0.join("mapping-source-protobuf");
        symlink(&outside.0, &bundle)
            .unwrap_or_else(|error| panic!("bundle symlink should be created: {error}"));

        let result = write_artifacts(
            &output.0,
            vec![
                (bundle.join("api/root.proto"), "message Root {}".to_string()),
                (output.0.join("mapping.mfd"), "<mapping/>".to_string()),
            ],
        );
        let error = match result {
            Err(error) => error,
            Ok(()) => panic!("symlinked bundle directory must be rejected"),
        };
        assert!(error.to_string().contains("symbolic link"));
        assert!(!outside.0.join("api/root.proto").exists());
        assert!(!output.0.join("mapping.mfd").exists());
    }

    #[test]
    fn nested_artifacts_reject_file_directory_prefix_collisions_before_writing() {
        let output = TempDir::new("prefix");
        let prefix = output.0.join("schema.proto");
        let mapping = output.0.join("mapping.mfd");
        let result = write_artifacts(
            &output.0,
            vec![
                (prefix.clone(), "message Root {}".to_string()),
                (prefix.join("nested.proto"), "message Nested {}".to_string()),
                (mapping.clone(), "<mapping/>".to_string()),
            ],
        );
        let error = match result {
            Err(error) => error,
            Ok(()) => panic!("file/directory artifact collision must be rejected"),
        };
        assert!(error.to_string().contains("directory prefix"));
        assert!(!prefix.exists());
        assert!(!mapping.exists());
    }
}
