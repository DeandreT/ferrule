//! A file-backed pipeline document independent of the current mapping canvas.

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context as _, bail};
use mapping::{Pipeline, PipelineInput, PipelineNamedInput, PipelineStage, Project};

const MAX_DOCUMENT_BYTES: u64 = 64 * 1024 * 1024;
static NEXT_SAVE: AtomicU64 = AtomicU64::new(0);

pub(super) struct PipelineEditorDocument {
    pub path: PathBuf,
    pub pipeline: Pipeline,
    original_bytes: Option<Vec<u8>>,
    saved_semantic: Option<Vec<u8>>,
}

impl PipelineEditorDocument {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if path.symlink_metadata()?.file_type().is_symlink() {
            bail!(
                "pipeline editor cannot save through a symbolic link; open the resolved file path"
            );
        }
        let bytes = read_bounded(path)?;
        let pipeline: Pipeline = serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing pipeline {}", path.display()))?;
        let saved_semantic = serde_json::to_vec(&pipeline)?;
        Ok(Self {
            path: path.to_path_buf(),
            pipeline,
            original_bytes: Some(bytes),
            saved_semantic: Some(saved_semantic),
        })
    }

    pub fn create(path: &Path) -> anyhow::Result<Self> {
        if path.exists() || path.symlink_metadata().is_ok() {
            bail!("{} already exists; open it to edit", path.display());
        }
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if !parent.is_dir() {
            bail!("pipeline directory {} does not exist", parent.display());
        }
        Ok(Self {
            path: path.to_path_buf(),
            pipeline: Pipeline {
                main_mapping_path: None,
                stages: Vec::new(),
            },
            original_bytes: None,
            saved_semantic: None,
        })
    }

    pub fn is_dirty(&self) -> bool {
        self.saved_semantic.as_ref().is_none_or(|saved| {
            serde_json::to_vec(&self.pipeline).map_or(true, |current| &current != saved)
        })
    }

    pub fn is_saved(&self) -> bool {
        self.original_bytes.is_some()
    }

    pub fn issues(&self) -> Vec<String> {
        engine::validate_pipeline(&self.pipeline)
            .into_iter()
            .map(|issue| issue.to_string())
            .collect()
    }

    pub fn add_project(&mut self, project_path: &Path) -> anyhow::Result<usize> {
        if self.pipeline.stages.len() >= 1_024 {
            bail!("pipeline already contains 1024 stages");
        }
        let bytes = read_bounded(project_path)?;
        let mut project: Project = serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing project {}", project_path.display()))?;
        cli::rebase_project_paths(&mut project, project_path, &self.path)?;
        let base = project_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("stage");
        let base = stage_id_base(base);
        let mut id = base.clone();
        let mut suffix = 2;
        while self.pipeline.stages.iter().any(|stage| stage.id == id) {
            id = format!("{base}-{suffix}");
            suffix += 1;
        }
        let mapping_path = mapping_identity(project_path, &self.path)?;
        if mapping_path.len() > 4096 {
            bail!("project path is too long to use as a mapping identity");
        }
        let extra_sources = project
            .extra_sources
            .iter()
            .filter(|source| source.dynamic_path.is_none())
            .map(|source| PipelineNamedInput {
                name: source.name.clone(),
                from: PipelineInput::Host {
                    name: format!("{id}-{}", source.name),
                },
            })
            .collect();
        self.pipeline.stages.push(PipelineStage {
            id: id.clone(),
            mapping_path: Some(mapping_path),
            project,
            source: PipelineInput::Host { name: id },
            extra_sources,
        });
        Ok(self.pipeline.stages.len() - 1)
    }

    pub fn rename_stage(&mut self, index: usize, new_id: &str) -> anyhow::Result<()> {
        let old_id = self
            .pipeline
            .stages
            .get(index)
            .map(|stage| stage.id.clone())
            .context("stage no longer exists")?;
        if new_id.is_empty() || new_id.len() > 256 || new_id.contains('\0') {
            bail!("stage ID must be nonempty, at most 256 bytes, and contain no NUL");
        }
        if self
            .pipeline
            .stages
            .iter()
            .enumerate()
            .any(|(other, stage)| other != index && stage.id == new_id)
        {
            bail!("stage ID `{new_id}` already exists");
        }
        if old_id == new_id {
            return Ok(());
        }
        // Rewrite all references on a cloned graph, then publish the complete edit.
        let mut candidate = self.pipeline.clone();
        candidate.stages[index].id = new_id.to_owned();
        for stage in &mut candidate.stages {
            rewrite_stage_reference(&mut stage.source, &old_id, new_id);
            for binding in &mut stage.extra_sources {
                rewrite_stage_reference(&mut binding.from, &old_id, new_id);
            }
        }
        self.pipeline = candidate;
        Ok(())
    }

    pub fn set_input(
        &mut self,
        stage_index: usize,
        binding_index: usize,
        input: PipelineInput,
    ) -> anyhow::Result<()> {
        let stage = self
            .pipeline
            .stages
            .get_mut(stage_index)
            .context("stage no longer exists")?;
        let destination = if binding_index == 0 {
            &mut stage.source
        } else {
            &mut stage
                .extra_sources
                .get_mut(binding_index - 1)
                .context("named source binding no longer exists")?
                .from
        };
        if let PipelineInput::Host { name } = &input
            && (name.is_empty() || name.contains('\0') || name.len() > 256)
        {
            bail!("host input name must be nonempty, at most 256 bytes, and contain no NUL");
        }
        *destination = input;
        Ok(())
    }

    /// Adds host bindings for static named sources absent from a loaded stage.
    /// Existing bindings, including ones with validation issues, are left for
    /// the user to edit explicitly.
    pub fn add_missing_static_bindings(&mut self, stage_index: usize) -> anyhow::Result<usize> {
        let stage = self
            .pipeline
            .stages
            .get(stage_index)
            .context("stage no longer exists")?;
        let missing = stage
            .project
            .extra_sources
            .iter()
            .enumerate()
            .filter(|(_, source)| {
                source.dynamic_path.is_none()
                    && !stage
                        .extra_sources
                        .iter()
                        .any(|binding| binding.name == source.name)
            })
            .map(|(index, source)| (index, source.name.clone()))
            .collect::<Vec<_>>();
        if missing.is_empty() {
            return Ok(0);
        }
        let mut used = self
            .pipeline
            .stages
            .iter()
            .flat_map(|stage| {
                std::iter::once(&stage.source)
                    .chain(stage.extra_sources.iter().map(|binding| &binding.from))
            })
            .filter_map(|input| match input {
                PipelineInput::Host { name } => Some(name.clone()),
                PipelineInput::StageTarget { .. } => None,
            })
            .collect::<BTreeSet<_>>();
        let stage = &mut self.pipeline.stages[stage_index];
        for (index, name) in &missing {
            let mut base = format!("{}-{name}", stage.id);
            if base.len() > 240 || base.contains('\0') {
                base = format!("{}-input-{index}", stage_id_base(&stage.id));
            }
            let mut host = base.clone();
            let mut suffix = 2usize;
            while used.contains(&host) {
                host = format!("{base}-{suffix}");
                suffix += 1;
            }
            used.insert(host.clone());
            stage.extra_sources.push(PipelineNamedInput {
                name: name.clone(),
                from: PipelineInput::Host { name: host },
            });
        }
        Ok(missing.len())
    }

    pub fn remove_stage(&mut self, index: usize) -> anyhow::Result<()> {
        let id = self
            .pipeline
            .stages
            .get(index)
            .context("stage no longer exists")?
            .id
            .clone();
        let referenced = self
            .pipeline
            .stages
            .iter()
            .enumerate()
            .any(|(other, stage)| {
                other != index
                    && (refers_to(&stage.source, &id)
                        || stage
                            .extra_sources
                            .iter()
                            .any(|binding| refers_to(&binding.from, &id)))
            });
        if referenced {
            bail!("stage `{id}` supplies another stage; change those bindings first");
        }
        self.pipeline.stages.remove(index);
        Ok(())
    }

    pub fn ensure_unchanged(&self) -> anyhow::Result<()> {
        if self
            .path
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            bail!(
                "pipeline path became a symbolic link; reopen the resolved file path before saving or running"
            );
        }
        match &self.original_bytes {
            Some(original) if &read_bounded(&self.path)? != original => {
                bail!("pipeline file changed since it was opened; reopen it before saving")
            }
            None if self.path.symlink_metadata().is_ok() => {
                bail!(
                    "pipeline file appeared after the new document was started; choose another path"
                )
            }
            _ => Ok(()),
        }
    }

    pub fn save(&mut self) -> anyhow::Result<()> {
        let issues = self.issues();
        if !issues.is_empty() {
            bail!(
                "pipeline has {} validation issue(s): {}",
                issues.len(),
                issues[0]
            );
        }
        self.ensure_unchanged()?;
        let mut bytes = serde_json::to_vec_pretty(&self.pipeline)?;
        bytes.push(b'\n');
        if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
            bail!("serialized pipeline exceeds the 64 MiB file limit");
        }
        atomic_replace(&self.path, &bytes)?;
        self.original_bytes = Some(bytes);
        self.saved_semantic = Some(serde_json::to_vec(&self.pipeline)?);
        Ok(())
    }
}

fn refers_to(input: &PipelineInput, id: &str) -> bool {
    matches!(input, PipelineInput::StageTarget { stage, .. } if stage == id)
}

fn rewrite_stage_reference(input: &mut PipelineInput, from: &str, to: &str) {
    if let PipelineInput::StageTarget { stage, .. } = input
        && stage == from
    {
        *stage = to.to_owned();
    }
}

fn stage_id_base(stem: &str) -> String {
    let mut value: String = stem
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '-'
            }
        })
        .take(128)
        .collect();
    if value.is_empty() {
        value = "stage".into();
    }
    value
}

fn mapping_identity(project_path: &Path, pipeline_path: &Path) -> anyhow::Result<String> {
    let absolute = normalized_absolute(project_path)?;
    let pipeline_parent = normalized_absolute(
        pipeline_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new(".")),
    )?;
    let from = pipeline_parent.components().collect::<Vec<_>>();
    let to = absolute.components().collect::<Vec<_>>();
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| left == right)
        .count();
    if common == 0 {
        bail!("project and pipeline paths cannot share a portable relative identity");
    }
    let mut value = PathBuf::new();
    for component in &from[common..] {
        if matches!(component, std::path::Component::Normal(_)) {
            value.push("..");
        }
    }
    for component in &to[common..] {
        value.push(component.as_os_str());
    }
    value
        .to_str()
        .map(str::to_owned)
        .context("project path is not valid Unicode")
}

fn normalized_absolute(path: &Path) -> anyhow::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::Prefix(_)
            | std::path::Component::RootDir
            | std::path::Component::Normal(_) => normalized.push(component.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
        }
    }
    Ok(normalized)
}

fn read_bounded(path: &Path) -> anyhow::Result<Vec<u8>> {
    let mut file = fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(MAX_DOCUMENT_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("reading {}", path.display()))?;
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        bail!("{} exceeds the 64 MiB file limit", path.display());
    }
    Ok(bytes)
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .context("pipeline path has no file name")?
        .to_string_lossy();
    for _ in 0..32 {
        let nonce = NEXT_SAVE.fetch_add(1, Ordering::Relaxed);
        let staged = parent.join(format!(".{name}.{}-{nonce}.tmp", std::process::id()));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged);
        let mut file = match file {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error).with_context(|| format!("staging {}", path.display())),
        };
        let result = (|| -> anyhow::Result<()> {
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&staged, path).with_context(|| format!("publishing {}", path.display()))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&staged);
        }
        return result;
    }
    bail!("could not reserve a temporary file for {}", path.display())
}
