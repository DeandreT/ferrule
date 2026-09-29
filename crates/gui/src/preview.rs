use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PreviewTarget {
    Primary,
    Named(String),
}

/// One declared field within the active output's target-scope path.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct PreviewBreakpoint {
    pub(super) target_path: Vec<String>,
    pub(super) field: String,
}

impl PreviewBreakpoint {
    pub(super) fn label(&self) -> String {
        if self.target_path.is_empty() {
            format!("<root> / {}", self.field)
        } else {
            format!("{} / {}", self.target_path.join(" / "), self.field)
        }
    }

    pub(super) fn matches(&self, write: &engine::PendingTargetWrite) -> bool {
        !write.field_truncated
            && self.target_path == write.scope.target_path
            && self.field == write.field
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ScalarValueType {
    #[default]
    String,
    Bool,
    Int,
    Float,
    Null,
    JsonNull,
    XmlNil,
}

impl ScalarValueType {
    pub(super) const ALL: [Self; 7] = [
        Self::String,
        Self::Bool,
        Self::Int,
        Self::Float,
        Self::Null,
        Self::JsonNull,
        Self::XmlNil,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::String => "String",
            Self::Bool => "Boolean",
            Self::Int => "Integer",
            Self::Float => "Number",
            Self::Null => "Absent null",
            Self::JsonNull => "JSON null",
            Self::XmlNil => "XML nil",
        }
    }

    fn trace_type(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Float => "float",
            Self::Null => "null",
            Self::JsonNull => "json null",
            Self::XmlNil => "xml nil",
        }
    }

    pub(super) fn needs_text(self) -> bool {
        matches!(self, Self::String | Self::Bool | Self::Int | Self::Float)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct BreakpointValueConditionDraft {
    pub(super) enabled: bool,
    pub(super) value_type: ScalarValueType,
    pub(super) text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DebugScalarCondition {
    value_type: ScalarValueType,
    canonical_preview: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct BreakpointPositionConditionDraft {
    pub(super) enabled: bool,
    pub(super) text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DebugPositionCondition {
    index: usize,
}

impl BreakpointPositionConditionDraft {
    pub(super) fn compile(&self) -> Result<Option<DebugPositionCondition>, &'static str> {
        if !self.enabled {
            return Ok(None);
        }
        let text = self.text.trim();
        if text.len() > 20 {
            return Err("Active item number exceeds the supported range.");
        }
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("Enter a positive 1-based active item number.");
        }
        let index = text
            .parse::<usize>()
            .map_err(|_| "Active item number exceeds the supported range.")?;
        if index == 0 {
            return Err("Active item number must be at least 1.");
        }
        Ok(Some(DebugPositionCondition { index }))
    }
}

impl DebugPositionCondition {
    pub(super) fn matches(&self, write: &engine::PendingTargetWrite) -> bool {
        write
            .positions
            .last()
            .is_some_and(|position| position.index == self.index)
    }
}

impl BreakpointValueConditionDraft {
    pub(super) fn compile(&self) -> Result<Option<DebugScalarCondition>, &'static str> {
        if !self.enabled {
            return Ok(None);
        }
        let canonical_preview = match self.value_type {
            ScalarValueType::String => {
                if self.text.chars().count() > 160 {
                    return Err("String conditions are limited to 160 characters.");
                }
                self.text.clone()
            }
            ScalarValueType::Bool => self
                .text
                .trim()
                .parse::<bool>()
                .map_err(|_| "Enter true or false for a Boolean condition.")?
                .to_string(),
            ScalarValueType::Int => self
                .text
                .trim()
                .parse::<i64>()
                .map_err(|_| "Enter a signed 64-bit integer.")?
                .to_string(),
            ScalarValueType::Float => {
                let value = self
                    .text
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| "Enter a finite number.")?;
                if !value.is_finite() {
                    return Err("Number conditions must be finite.");
                }
                value.to_string()
            }
            ScalarValueType::Null => "null".into(),
            ScalarValueType::JsonNull => "json-null".into(),
            ScalarValueType::XmlNil => "xml-nil".into(),
        };
        Ok(Some(DebugScalarCondition {
            value_type: self.value_type,
            canonical_preview,
        }))
    }
}

impl DebugScalarCondition {
    pub(super) fn matches(&self, write: &engine::PendingTargetWrite) -> bool {
        if write.pending.kind != engine::TraceOutputKind::Scalar {
            return false;
        }
        write.pending.value.as_ref().is_some_and(|value| {
            !value.truncated
                && value.value_type == self.value_type.trace_type()
                && value.preview == self.canonical_preview
        })
    }
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
    pub(super) debug_breakpoint: Option<PreviewBreakpoint>,
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
            debug_breakpoint: None,
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

pub(super) fn breakpoint_candidates(
    project: &mapping::Project,
    target: &PreviewTarget,
) -> Vec<PreviewBreakpoint> {
    const MAX_CANDIDATES: usize = 512;
    const MAX_SCOPES: usize = 4096;
    const MAX_DEBUG_FIELD_CHARS: usize = 160;
    let root = match target {
        PreviewTarget::Primary => &project.root,
        PreviewTarget::Named(name) => {
            let Some(target) = project
                .extra_targets
                .iter()
                .find(|target| target.name == *name)
            else {
                return Vec::new();
            };
            &target.root
        }
    };
    let mut candidates = BTreeSet::new();
    let mut scopes = vec![(root, Vec::<String>::new())];
    let mut visited_scopes = 0usize;
    while let Some((scope, target_path)) = scopes.pop() {
        visited_scopes += 1;
        for binding in &scope.bindings {
            if binding.target_field.chars().count() <= MAX_DEBUG_FIELD_CHARS {
                candidates.insert(PreviewBreakpoint {
                    target_path: target_path.clone(),
                    field: binding.target_field.clone(),
                });
            }
        }
        for child in &scope.children {
            if child.target_field.chars().count() <= MAX_DEBUG_FIELD_CHARS {
                candidates.insert(PreviewBreakpoint {
                    target_path: target_path.clone(),
                    field: child.target_field.clone(),
                });
            }
            let mut child_path = target_path.clone();
            child_path.push(child.target_field.clone());
            scopes.push((child, child_path));
        }
        if let Some(sequence) = scope.concatenated() {
            for segment in sequence.iter() {
                scopes.push((segment, target_path.clone()));
            }
        }
        if candidates.len() >= MAX_CANDIDATES || visited_scopes >= MAX_SCOPES {
            break;
        }
    }
    candidates.into_iter().take(MAX_CANDIDATES).collect()
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

    fn pending(
        value_type: &'static str,
        preview: &str,
        truncated: bool,
    ) -> engine::PendingTargetWrite {
        engine::PendingTargetWrite {
            scope: engine::TraceScope {
                target: engine::TraceTarget::Primary,
                target_path: Vec::new(),
                structural_path: Vec::new(),
            },
            positions: Vec::new(),
            source: engine::DebugSourceContext {
                frames: Vec::new(),
                omitted_outer_frames: 0,
            },
            field: "value".into(),
            field_truncated: false,
            binding: engine::TraceTargetFieldBinding::StaticChild,
            pending: engine::DebugInstancePreview {
                kind: engine::TraceOutputKind::Scalar,
                value: Some(engine::TraceValue {
                    value_type,
                    preview: preview.into(),
                    truncated,
                }),
                length: None,
            },
            draft: engine::DebugScopeDraft {
                fields: Vec::new(),
                omitted_fields: 0,
            },
        }
    }

    fn position(index: usize) -> engine::TracePosition {
        engine::TracePosition {
            collection: vec!["row".into()],
            index,
            grouped: false,
            join: None,
            join_position: None,
            document_path: None,
        }
    }

    #[test]
    fn source_position_condition_requires_a_positive_number_and_an_active_position() {
        let mut draft = BreakpointPositionConditionDraft {
            text: "invalid".into(),
            ..Default::default()
        };
        assert_eq!(draft.compile().unwrap(), None);
        draft.enabled = true;
        for invalid in ["", "0", "-1", "1.5", "nope", "999999999999999999999"] {
            draft.text = invalid.into();
            assert!(draft.compile().is_err(), "{invalid} must be rejected");
        }
        draft.text = "0002".into();
        let condition = draft.compile().unwrap().unwrap();
        let mut write = pending("string", "value", false);
        assert!(
            !condition.matches(&write),
            "root writes have no item position"
        );
        write.positions = vec![position(1)];
        assert!(!condition.matches(&write));
        write.positions.push(position(2));
        assert!(condition.matches(&write));
        write.positions.reverse();
        assert!(
            !condition.matches(&write),
            "only the innermost position matches"
        );
    }

    #[test]
    fn scalar_condition_rejects_overlong_strings_and_invalid_numbers() {
        let mut draft = BreakpointValueConditionDraft {
            enabled: true,
            text: "x".repeat(161),
            ..Default::default()
        };
        assert!(draft.compile().is_err());
        draft.value_type = ScalarValueType::Int;
        draft.text = "1.0".into();
        assert!(draft.compile().is_err());
        draft.text = "0001".into();
        assert_eq!(draft.compile().unwrap().unwrap().canonical_preview, "1");
        draft.value_type = ScalarValueType::Float;
        draft.text = "1.00".into();
        assert_eq!(draft.compile().unwrap().unwrap().canonical_preview, "1");
        for nonfinite in ["NaN", "inf", "-inf"] {
            draft.text = nonfinite.into();
            assert!(draft.compile().is_err(), "{nonfinite} must be rejected");
        }
    }

    #[test]
    fn scalar_condition_distinguishes_null_kinds_and_rejects_truncated_values() {
        let mut draft = BreakpointValueConditionDraft {
            enabled: true,
            value_type: ScalarValueType::Null,
            ..Default::default()
        };
        let absent = pending("null", "null", false);
        let json_null = pending("json null", "json-null", false);
        let xml_nil = pending("xml nil", "xml-nil", false);
        assert!(draft.compile().unwrap().unwrap().matches(&absent));
        assert!(!draft.compile().unwrap().unwrap().matches(&json_null));
        assert!(!draft.compile().unwrap().unwrap().matches(&xml_nil));
        draft.value_type = ScalarValueType::JsonNull;
        assert!(draft.compile().unwrap().unwrap().matches(&json_null));
        assert!(!draft.compile().unwrap().unwrap().matches(&absent));
        draft.value_type = ScalarValueType::XmlNil;
        assert!(draft.compile().unwrap().unwrap().matches(&xml_nil));
        assert!(!draft.compile().unwrap().unwrap().matches(&json_null));

        draft.value_type = ScalarValueType::String;
        draft.text = "x".repeat(160);
        let truncated = pending("string", &"x".repeat(160), true);
        assert!(!draft.compile().unwrap().unwrap().matches(&truncated));
    }

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
