use std::collections::BTreeMap;

use ir::{ScalarType, SchemaNode};
use serde::{Deserialize, Serialize};

use crate::{FormatOptions, Graph, NodeId, OwnedSourceRef, Scope, SequenceExpr, SourceSelector};

/// Stable identity of one user-defined function within a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FunctionId(u64);

impl FunctionId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Stable identity of one ordered input in a user-defined function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FunctionParameterId(u64);

impl FunctionParameterId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// One ordered scalar input exposed by a user-defined function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionParameter {
    pub id: FunctionParameterId,
    pub name: String,
    pub ty: ScalarType,
}

/// A reusable scalar mapping with an isolated graph and one output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserFunction {
    pub library: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: Vec<FunctionParameter>,
    pub output_name: String,
    pub output_type: ScalarType,
    pub body: Graph,
    pub output: NodeId,
}

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
#[serde(tag = "kind", rename_all = "snake_case", from = "FailureIterationWire")]
pub enum FailureIteration {
    /// Walk a framed runtime source path. An empty collection iterates flat
    /// rows when the current source boundary is itself repeated.
    Source { collection: Vec<String> },
    /// Materialize a generated scalar sequence in the rule's parent context.
    Sequence { sequence: SequenceExpr },
    /// Walk rows of exactly one explicitly owned endpoint. The path is
    /// endpoint-relative: no other endpoint, alias or same-path collection
    /// supplies rows when the selected owner's collection is absent.
    OwnedSource { source: OwnedSourceRef },
}

impl FailureIteration {
    /// The explicit owner and endpoint-relative path, for owned iteration only.
    pub fn owned_source(&self) -> Option<&OwnedSourceRef> {
        match self {
            Self::OwnedSource { source } => Some(source),
            Self::Source { .. } | Self::Sequence { .. } => None,
        }
    }
}

/// Readers keep legacy variants tolerant and the owned form strict.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum FailureIterationWire {
    Source { collection: Vec<String> },
    Sequence { sequence: SequenceExpr },
    OwnedSource(OwnedFailureSourceWire),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnedFailureSourceWire {
    source: OwnedSourceRef,
}

impl From<FailureIterationWire> for FailureIteration {
    fn from(wire: FailureIterationWire) -> Self {
        match wire {
            FailureIterationWire::Source { collection } => Self::Source { collection },
            FailureIterationWire::Sequence { sequence } => Self::Sequence { sequence },
            FailureIterationWire::OwnedSource(OwnedFailureSourceWire { source }) => {
                Self::OwnedSource { source }
            }
        }
    }
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
    /// Reusable scalar mappings referenced by [`crate::Node::UserFunctionCall`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub user_functions: BTreeMap<FunctionId, UserFunction>,
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
    /// A run-time path expression and the repeated driver collection that
    /// frames it. When present, the host loads one typed source instance for
    /// each produced path instead of preloading `path` once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic_path: Option<DynamicSourcePath>,
}

/// The driver of a per-item dynamic source.
///
/// A legacy path array keeps its primary-relative meaning and wire form. An
/// owned driver object names exactly one endpoint and an endpoint-relative
/// collection; readers that know only path arrays refuse it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicSourcePath {
    pub node: NodeId,
    pub iteration: SourceSelector,
}

impl DynamicSourcePath {
    /// A legacy driver iterating `iteration` from the primary-source context.
    pub fn legacy(node: NodeId, iteration: Vec<String>) -> Self {
        Self {
            node,
            iteration: SourceSelector::Legacy(iteration),
        }
    }

    /// An owned driver iterating one collection of exactly one endpoint.
    pub fn owned(node: NodeId, driver: OwnedSourceRef) -> Self {
        Self {
            node,
            iteration: SourceSelector::Owned(driver),
        }
    }

    /// The legacy driver path; `None` for an owned driver.
    pub fn legacy_driver(&self) -> Option<&[String]> {
        match &self.iteration {
            SourceSelector::Legacy(path) => Some(path),
            SourceSelector::Owned(_) => None,
        }
    }

    /// The owned driver collection; `None` for a legacy driver.
    pub fn owned_driver(&self) -> Option<&OwnedSourceRef> {
        match &self.iteration {
            SourceSelector::Legacy(_) => None,
            SourceSelector::Owned(driver) => Some(driver),
        }
    }
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
