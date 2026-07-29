use std::io::Read as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PreviewTarget {
    Primary,
    Named(String),
}

impl PreviewTarget {
    pub(super) fn selection(&self) -> cli::TargetSelection<'_> {
        match self {
            Self::Primary => cli::TargetSelection::Primary,
            Self::Named(name) => cli::TargetSelection::Named(name),
        }
    }

    pub(super) fn label(&self) -> &str {
        match self {
            Self::Primary => "Primary",
            Self::Named(name) => name,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PreviewDraft {
    pub(super) target: PreviewTarget,
    pub(super) input_identity: String,
    pub(super) output_identity: String,
    pub(super) input_text: String,
}

impl PreviewDraft {
    pub(super) fn new(
        target: PreviewTarget,
        input_identity: String,
        output_identity: String,
    ) -> Self {
        Self {
            target,
            input_identity,
            output_identity,
            input_text: String::new(),
        }
    }

    pub(super) fn can_execute(&self) -> bool {
        !self.input_identity.trim().is_empty()
            && !self.output_identity.trim().is_empty()
            && self.input_identity.trim().len() <= cli::MAX_PAYLOAD_PATH_BYTES
            && self.output_identity.trim().len() <= cli::MAX_PAYLOAD_PATH_BYTES
            && self.input_text.len() <= cli::MAX_PAYLOAD_DOCUMENT_BYTES
    }
}

pub(super) struct LoadedPreviewSource {
    pub(super) name: String,
    pub(super) path: PathBuf,
    pub(super) bytes: Vec<u8>,
}

pub(super) fn load_required_sources(
    project: &mapping::Project,
    project_path: Option<&Path>,
    target: &PreviewTarget,
) -> anyhow::Result<Vec<LoadedPreviewSource>> {
    let required = cli::required_sources_for_target(project, target.selection())?;
    if !required.dynamic_sources.is_empty() {
        let names = required
            .dynamic_sources
            .iter()
            .map(|source| source.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        bail!(
            "Preview cannot pre-load graph-computed secondary source(s): {names}. Use Run, or supply a host workflow that enumerates every computed logical path."
        );
    }

    required
        .static_sources
        .into_iter()
        .map(|source| load_source(source, project_path))
        .collect()
}

fn load_source(
    source: &mapping::NamedSource,
    project_path: Option<&Path>,
) -> anyhow::Result<LoadedPreviewSource> {
    if is_http(&source.path) {
        bail!(
            "Preview cannot fetch HTTP secondary source `{}`. Use Run or replace it with an existing local project input.",
            source.name
        );
    }
    let stored = PathBuf::from(source.path.trim());
    if stored.as_os_str().is_empty() {
        bail!(
            "secondary source `{}` has no configured input path",
            source.name
        );
    }
    let resolved = if stored.is_absolute() {
        stored
    } else {
        let project_path = project_path.with_context(|| {
            format!(
                "secondary source `{}` uses relative path `{}`; save the project first so Preview has a base directory",
                source.name,
                stored.display()
            )
        })?;
        let parent = project_path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        parent.join(stored)
    };
    let canonical = std::fs::canonicalize(&resolved).with_context(|| {
        format!(
            "resolving secondary source `{}` at {}",
            source.name,
            resolved.display()
        )
    })?;
    let metadata = std::fs::metadata(&canonical).with_context(|| {
        format!(
            "reading metadata for secondary source `{}` at {}",
            source.name,
            canonical.display()
        )
    })?;
    if !metadata.is_file() {
        bail!(
            "secondary source `{}` is not a regular file: {}",
            source.name,
            canonical.display()
        );
    }
    let bytes = read_bounded(&canonical, &source.name)?;
    Ok(LoadedPreviewSource {
        name: source.name.clone(),
        path: canonical,
        bytes,
    })
}

fn read_bounded(path: &Path, name: &str) -> anyhow::Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("opening secondary source `{name}` at {}", path.display()))?;
    let limit = u64::try_from(cli::MAX_PAYLOAD_DOCUMENT_BYTES)
        .context("payload document limit does not fit in u64")?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .with_context(|| format!("reading secondary source `{name}` at {}", path.display()))?;
    if bytes.len() > cli::MAX_PAYLOAD_DOCUMENT_BYTES {
        bail!(
            "secondary source `{name}` exceeds the {} MiB preview limit",
            cli::MAX_PAYLOAD_DOCUMENT_BYTES / (1024 * 1024)
        );
    }
    Ok(bytes)
}

fn is_http(path: &str) -> bool {
    let lowercase = path.to_ascii_lowercase();
    lowercase.starts_with("http://") || lowercase.starts_with("https://")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::{ScalarType, SchemaNode};
    use mapping::{DynamicSourcePath, FormatOptions, NamedSource, Node, Scope};

    fn project_with_source(source: NamedSource) -> mapping::Project {
        let mut project = mapping::Project {
            source: SchemaNode::group("source", Vec::new()),
            target: SchemaNode::group("target", Vec::new()),
            source_path: None,
            target_path: None,
            source_options: FormatOptions::default(),
            target_options: FormatOptions::default(),
            extra_sources: vec![source],
            extra_targets: Vec::new(),
            failure_rules: Vec::new(),
            user_functions: std::collections::BTreeMap::new(),
            graph: mapping::Graph::default(),
            root: Scope::default(),
        };
        project.graph.nodes.insert(
            1,
            Node::SourceField {
                path: vec!["catalog".into(), "value".into()],
                frame: None,
            },
        );
        project.root.bindings.push(mapping::Binding {
            target_field: "unused".into(),
            node: 1,
        });
        project
    }

    fn static_source(path: &str) -> NamedSource {
        NamedSource {
            name: "catalog".into(),
            path: path.into(),
            schema: SchemaNode::group(
                "catalog",
                vec![SchemaNode::scalar("value", ScalarType::String)],
            ),
            options: FormatOptions::default(),
            dynamic_path: None,
        }
    }

    #[test]
    fn relative_sources_require_a_saved_project_base() -> anyhow::Result<()> {
        let project = project_with_source(static_source("catalog.json"));
        let Err(error) = load_required_sources(&project, None, &PreviewTarget::Primary) else {
            bail!("relative source unexpectedly loaded without a project base");
        };
        assert!(error.to_string().contains("save the project first"));
        Ok(())
    }

    #[test]
    fn static_sources_are_loaded_from_the_saved_project_directory() -> anyhow::Result<()> {
        let directory =
            std::env::temp_dir().join(format!("ferrule-gui-preview-static-{}", std::process::id()));
        std::fs::create_dir_all(&directory)?;
        let source_path = directory.join("catalog.json");
        std::fs::write(&source_path, r#"{"value":"A"}"#)?;
        let project_path = directory.join("mapping.json");
        let project = project_with_source(static_source("catalog.json"));

        let sources =
            load_required_sources(&project, Some(&project_path), &PreviewTarget::Primary)?;

        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, "catalog");
        assert_eq!(sources[0].path, source_path.canonicalize()?);
        assert_eq!(sources[0].bytes, br#"{"value":"A"}"#);
        std::fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn dynamic_sources_fail_preflight_instead_of_being_omitted() -> anyhow::Result<()> {
        let mut source = static_source("unused.json");
        source.dynamic_path = Some(DynamicSourcePath {
            node: 2,
            iteration: Vec::new(),
        });
        let mut project = project_with_source(source);
        project.graph.nodes.insert(
            2,
            Node::Const {
                value: ir::Value::String("catalog.json".into()),
            },
        );

        let Err(error) = load_required_sources(&project, None, &PreviewTarget::Primary) else {
            bail!("dynamic source was silently omitted");
        };
        assert!(error.to_string().contains("graph-computed"));
        assert!(error.to_string().contains("catalog"));
        Ok(())
    }
}
