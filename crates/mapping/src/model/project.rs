use ir::SchemaNode;
use serde::{Deserialize, Serialize};

use crate::{FormatOptions, Graph, NodeId, Scope, SequenceExpr};

/// One ordered mapping failure evaluated before any target is produced.
///
/// The message is evaluated lazily in the first selected item's context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureRule {
    pub iteration: FailureIteration,
    pub selection: FailureSelection,
    /// Optional graph-computed error text. Absence is preserved distinctly
    /// from an expression that evaluates to an empty string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<NodeId>,
}

/// The item sequence inspected by a [`FailureRule`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FailureIteration {
    /// Walk a framed runtime source path. An empty collection iterates flat
    /// rows when the current source boundary is itself repeated.
    Source { collection: Vec<String> },
    /// Materialize a generated scalar sequence in the rule's parent context.
    Sequence { sequence: SequenceExpr },
}

/// Selects which iterated items trigger a [`FailureRule`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FailureSelection {
    All,
    WhenTrue { predicate: NodeId },
    WhenFalse { predicate: NodeId },
}

impl FailureSelection {
    pub fn predicate(self) -> Option<NodeId> {
        match self {
            Self::All => None,
            Self::WhenTrue { predicate } | Self::WhenFalse { predicate } => Some(predicate),
        }
    }
}

/// A complete mapping project: the source/target shapes, the graph, and the
/// scope tree that maps one into the other.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub source: SchemaNode,
    pub target: SchemaNode,
    /// Default source/target instances, carried over from imported designs
    /// and used to pick the component format on `.mfd` export. File paths are
    /// resolved relative to the project directory; typed HTTP GET sources
    /// retain their absolute URL. Explicit CLI input/output flags override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_path: Option<String>,
    #[serde(default)]
    pub source_options: FormatOptions,
    #[serde(default)]
    pub target_options: FormatOptions,
    /// Secondary inputs (reference/lookup data) loaded alongside the
    /// primary source. Each becomes addressable by its `name` from any
    /// scope or field path via outward fallback.
    #[serde(default)]
    pub extra_sources: Vec<NamedSource>,
    /// Additional independently shaped outputs evaluated from the same
    /// source frames and graph. The primary target remains in `target` and
    /// `root` for compatibility with single-output projects and hosts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_targets: Vec<NamedTarget>,
    /// Ordered failure rules evaluated before any primary or named target.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failure_rules: Vec<FailureRule>,
    pub graph: Graph,
    pub root: Scope,
}

/// A named secondary input. `path` is the instance file or typed HTTP GET URL
/// to load. Files are resolved relative to the project directory when not
/// absolute; URLs remain absolute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedSource {
    pub name: String,
    pub path: String,
    pub schema: SchemaNode,
    #[serde(default)]
    pub options: FormatOptions,
    /// A run-time path expression and the repeated primary-source collection
    /// that frames it. When present, the host loads one typed source instance
    /// for each produced path instead of preloading `path` once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic_path: Option<DynamicSourcePath>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicSourcePath {
    pub node: NodeId,
    pub iteration: Vec<String>,
}

/// One additional output document produced by a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedTarget {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub schema: SchemaNode,
    #[serde(default)]
    pub options: FormatOptions,
    pub root: Scope,
}
