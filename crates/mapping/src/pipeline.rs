//! An ordered, typed graph of complete mapping projects.
//!
//! A pipeline is separate from [`crate::Project`] so existing single-mapping
//! files keep their wire format. Each stage can read a host document or another
//! stage result; the execution engine chooses a stable topological
//! order, so declarations need not be sorted by dependency.

use serde::{Deserialize, Serialize};

use crate::Project;

/// A graph of mappings whose outputs may become later inputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
    /// Optional identity of the top-level mapping design. File hosts resolve
    /// relative paths from the pipeline file; absent keeps legacy behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_mapping_path: Option<String>,
    pub stages: Vec<PipelineStage>,
}

/// One complete mapping and the bindings for its input boundaries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStage {
    /// Stable identity used by downstream stage references.
    pub id: String,
    /// Optional local identity for the active mapping file at this stage.
    /// Relative paths are resolved from the pipeline file by file hosts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mapping_path: Option<String>,
    pub project: Project,
    pub source: PipelineInput,
    /// Bindings for static named sources declared by `project.extra_sources`.
    /// Dynamic sources remain resolved by the execution host's loader.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_sources: Vec<PipelineNamedInput>,
}

/// A binding for one static named source of a stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineNamedInput {
    pub name: String,
    pub from: PipelineInput,
}

/// Where a stage input obtains its value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PipelineInput {
    /// A host-owned input, identified independently of file paths.
    Host { name: String },
    /// The primary target, or one named target, of another stage.
    StageTarget {
        stage: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<String>,
    },
}
