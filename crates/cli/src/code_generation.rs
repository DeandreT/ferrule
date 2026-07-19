use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use codegen::ArtifactSet;

use super::load_project;

/// Source language and runtime linkage for one generated mapping project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerateTarget {
    Rust { runtime_path: PathBuf },
    CSharp,
}

/// Files written by a successful atomic generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerateOutcome {
    pub output_directory: PathBuf,
    pub files_written: usize,
}

/// Lowers a project and atomically writes a complete generated source tree.
///
/// The destination must not already exist. This keeps an unsupported mapping
/// or interrupted write from publishing a partial project.
pub fn generate_project(
    project_path: &Path,
    output_directory: &Path,
    target: GenerateTarget,
) -> anyhow::Result<GenerateOutcome> {
    let project = load_project(project_path)?;
    let program = codegen::lower(&project).map_err(|error| {
        let details = error
            .diagnostics()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n  - ");
        anyhow::anyhow!("{error}:\n  - {details}")
    })?;
    let artifacts = match target {
        GenerateTarget::Rust { runtime_path } => {
            let runtime_path = fs::canonicalize(&runtime_path).with_context(|| {
                format!(
                    "resolving Rust codegen runtime path {}",
                    runtime_path.display()
                )
            })?;
            let runtime_path = runtime_path
                .to_str()
                .context("Rust codegen runtime path must be valid UTF-8")?;
            codegen_rust::emit(
                &program,
                &codegen_rust::Options {
                    package_name: "ferrule-generated-mapping".to_string(),
                    runtime_dependency: codegen_rust::RuntimeDependency::Path(
                        runtime_path.to_owned(),
                    ),
                },
            )?
        }
        GenerateTarget::CSharp => codegen_csharp::emit(&program)?,
    };
    write_artifacts(output_directory, &artifacts)?;
    Ok(GenerateOutcome {
        output_directory: output_directory.to_path_buf(),
        files_written: artifacts.len(),
    })
}

fn write_artifacts(output_directory: &Path, artifacts: &ArtifactSet) -> anyhow::Result<()> {
    if output_directory.exists() {
        bail!(
            "generated output directory {} already exists",
            output_directory.display()
        );
    }
    let parent = output_directory
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("creating generated output parent {}", parent.display()))?;
    let name = output_directory
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .context("generated output directory must have a UTF-8 file name")?;
    let mut staging = None;
    for attempt in 0..100_u32 {
        let candidate = parent.join(format!(
            ".{name}.ferrule-stage-{}-{attempt}",
            std::process::id()
        ));
        match fs::create_dir(&candidate) {
            Ok(()) => {
                staging = Some(candidate);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "creating generated staging directory {}",
                        candidate.display()
                    )
                });
            }
        }
    }
    let staging = staging.context("could not allocate a generated staging directory")?;
    let mut pending = PendingDirectory(Some(staging));
    let staging = pending.path();
    for file in artifacts.files() {
        let path = staging.join(file.path.as_str());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("creating generated artifact directory {}", parent.display())
            })?;
        }
        fs::write(&path, &file.contents)
            .with_context(|| format!("writing generated artifact {}", path.display()))?;
    }
    publish_directory(staging, output_directory).with_context(|| {
        format!(
            "publishing generated output directory {}",
            output_directory.display()
        )
    })?;
    pending.commit();
    Ok(())
}

#[cfg(any(
    target_os = "android",
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "visionos",
    target_os = "watchos",
    target_os = "redox",
))]
fn publish_directory(staging: &Path, output_directory: &Path) -> std::io::Result<()> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        staging,
        rustix::fs::CWD,
        output_directory,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(Into::into)
}

#[cfg(not(any(
    target_os = "android",
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "visionos",
    target_os = "watchos",
    target_os = "redox",
)))]
fn publish_directory(staging: &Path, output_directory: &Path) -> std::io::Result<()> {
    if output_directory.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "generated output destination already exists",
        ));
    }
    fs::rename(staging, output_directory)
}

struct PendingDirectory(Option<PathBuf>);

impl PendingDirectory {
    fn path(&self) -> &Path {
        self.0.as_deref().unwrap_or_else(|| Path::new("."))
    }

    fn commit(&mut self) {
        self.0 = None;
    }
}

impl Drop for PendingDirectory {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = fs::remove_dir_all(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn publish_never_replaces_an_existing_directory() -> anyhow::Result<()> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "ferrule_codegen_publish_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let staging = root.join("staging");
        let destination = root.join("destination");
        fs::create_dir_all(&staging)?;
        fs::write(staging.join("generated.txt"), "generated")?;
        fs::create_dir_all(&destination)?;

        let error = publish_directory(&staging, &destination)
            .expect_err("no-replace publication must reject an existing destination");

        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(fs::read_dir(&destination)?.next().is_none());
        assert_eq!(
            fs::read_to_string(staging.join("generated.txt"))?,
            "generated"
        );
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
