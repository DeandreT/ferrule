//! File-backed pipeline inspection and run setup, separate from the project editor.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use mapping::{Pipeline, PipelineInput};

const MAX_PIPELINE_BYTES: u64 = 64 * 1024 * 1024;

pub(super) struct PipelineRunDraft {
    pub path: PathBuf,
    pub pipeline: Pipeline,
    pub inputs: Vec<PipelineInputDraft>,
    pub outputs: Vec<PipelineOutputDraft>,
    pub issues: Vec<String>,
    original_bytes: Vec<u8>,
}

pub(super) struct PipelineInputDraft {
    pub name: String,
    pub path: String,
}

pub(super) struct PipelineOutputDraft {
    pub stage: String,
    pub target: Option<String>,
    pub path: String,
}

impl PipelineRunDraft {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let bytes = read_pipeline(path)?;
        let pipeline: Pipeline = serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing pipeline {}", path.display()))?;
        let issues = engine::validate_pipeline(&pipeline)
            .into_iter()
            .map(|issue| issue.to_string())
            .collect();
        let mut input_names: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut outputs = Vec::new();
        for stage in &pipeline.stages {
            collect_host(
                &stage.source,
                stage.project.source_path.as_deref(),
                stage.mapping_path.as_deref(),
                path,
                &mut input_names,
            );
            for binding in &stage.extra_sources {
                let source_path = stage
                    .project
                    .extra_sources
                    .iter()
                    .find(|source| source.name == binding.name)
                    .map(|source| source.path.as_str());
                collect_host(
                    &binding.from,
                    source_path,
                    stage.mapping_path.as_deref(),
                    path,
                    &mut input_names,
                );
            }
            outputs.push(PipelineOutputDraft {
                stage: stage.id.clone(),
                target: None,
                path: String::new(),
            });
            outputs.extend(
                stage
                    .project
                    .extra_targets
                    .iter()
                    .map(|target| PipelineOutputDraft {
                        stage: stage.id.clone(),
                        target: Some(target.name.clone()),
                        path: String::new(),
                    }),
            );
        }
        let inputs = input_names
            .into_iter()
            .map(|(name, paths)| PipelineInputDraft {
                name,
                path: if paths.len() == 1 {
                    paths.into_iter().next().unwrap_or_default()
                } else {
                    String::new()
                },
            })
            .collect();
        Ok(Self {
            path: path.to_path_buf(),
            pipeline,
            inputs,
            outputs,
            issues,
            original_bytes: bytes,
        })
    }

    pub fn requests(
        &self,
    ) -> anyhow::Result<(Vec<cli::PipelineHostFile>, Vec<cli::PipelineOutputFile>)> {
        if !self.issues.is_empty() {
            bail!("pipeline has {} validation issue(s)", self.issues.len());
        }
        let inputs = self
            .inputs
            .iter()
            .map(|input| {
                Ok(cli::PipelineHostFile {
                    name: input.name.clone(),
                    path: self.resolve_path(&input.path).with_context(|| {
                        format!("host input `{}` needs a file path", input.name)
                    })?,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let outputs = self
            .outputs
            .iter()
            .filter(|output| !output.path.trim().is_empty())
            .map(|output| {
                Ok(cli::PipelineOutputFile {
                    stage: output.stage.clone(),
                    target: output.target.clone(),
                    path: self.resolve_path(&output.path)?,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        if outputs.is_empty() {
            bail!("choose at least one output path");
        }
        Ok((inputs, outputs))
    }

    pub fn ensure_unchanged(&self) -> anyhow::Result<()> {
        if read_pipeline(&self.path)? != self.original_bytes {
            bail!("pipeline file changed since it was opened; reopen it before running");
        }
        Ok(())
    }

    fn resolve_path(&self, value: &str) -> anyhow::Result<PathBuf> {
        let value = value.trim();
        if value.is_empty() {
            bail!("path is empty");
        }
        if value.split_once("://").is_some_and(|(scheme, _)| {
            scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
        }) {
            return Ok(PathBuf::from(value));
        }
        let path = PathBuf::from(value);
        if path.is_absolute() {
            Ok(path)
        } else {
            Ok(self
                .path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."))
                .join(path))
        }
    }
}

fn collect_host(
    input: &PipelineInput,
    stored_path: Option<&str>,
    mapping_path: Option<&str>,
    pipeline_path: &Path,
    names: &mut BTreeMap<String, BTreeSet<String>>,
) {
    if let PipelineInput::Host { name } = input {
        let paths = names.entry(name.clone()).or_default();
        if let Some(path) =
            stored_path.and_then(|path| stored_host_path(path, mapping_path, pipeline_path))
        {
            paths.insert(path);
        }
    }
}

fn stored_host_path(
    value: &str,
    mapping_path: Option<&str>,
    pipeline_path: &Path,
) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if value.split_once("://").is_some_and(|(scheme, _)| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    }) {
        return Some(value.into());
    }
    let path = Path::new(value);
    if path.is_absolute() {
        return Some(path.to_string_lossy().into_owned());
    }
    let pipeline_dir = pipeline_path.parent().unwrap_or_else(|| Path::new("."));
    let pipeline_dir = if pipeline_dir.is_absolute() {
        pipeline_dir.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(pipeline_dir)
    };
    let mapping_dir = mapping_path
        .filter(|path| !path.trim().is_empty())
        .map(Path::new)
        .map(|path| {
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                pipeline_dir.join(path)
            };
            path.parent().unwrap_or(&pipeline_dir).to_path_buf()
        })
        .unwrap_or(pipeline_dir);
    Some(mapping_dir.join(path).to_string_lossy().into_owned())
}

fn read_pipeline(path: &Path) -> anyhow::Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("opening pipeline {}", path.display()))?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_PIPELINE_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("reading pipeline {}", path.display()))?;
    if bytes.len() as u64 > MAX_PIPELINE_BYTES {
        bail!("pipeline file exceeds the 64 MiB inspection limit");
    }
    Ok(bytes)
}
