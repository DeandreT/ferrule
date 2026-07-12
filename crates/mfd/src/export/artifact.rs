use std::io;
use std::path::{Path, PathBuf};
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

pub(super) fn write_artifacts(artifacts: Vec<(PathBuf, String)>) -> io::Result<()> {
    for (destination, _) in &artifacts {
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
    }

    let mut staged = Vec::with_capacity(artifacts.len());
    for (destination, contents) in artifacts {
        staged.push(stage_artifact(destination, contents.as_bytes())?);
    }
    for artifact in &staged {
        std::fs::rename(&artifact.temporary, &artifact.destination)?;
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
