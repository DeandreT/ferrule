use std::fmt;

use mapping::NodeId;

/// Why lowering could not produce backend-neutral code-generation IR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diagnostic {
    Validation {
        location: String,
        message: String,
    },
    UnsupportedProject {
        feature: ProjectFeature,
        count: usize,
    },
    UnsupportedScope {
        /// Static target-field path. Empty identifies the primary root.
        target_path: Vec<String>,
        feature: ScopeFeature,
    },
    UnsupportedNode {
        node: NodeId,
        kind: UnsupportedNodeKind,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectFeature {
    ExtraSources,
    ExtraTargets,
    FailureRules,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeFeature {
    Iteration,
    Construction(ScopeConstructionKind),
    Filter,
    Grouping,
    Sorting,
    SequenceWindows,
    IterationOutput,
    DynamicBindings,
    DynamicChildren,
    DynamicFieldMerge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeConstructionKind {
    CopyCurrentSource,
    Scalar,
    XmlMixedContent,
    RecursiveFilter,
    PathHierarchy,
    AdjacencyTree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedNodeKind {
    FramedSourceField,
    NonFiniteFloatLiteral,
    SourceDocumentPath,
    Position,
    JoinField,
    JoinPosition,
    RuntimeValue,
    Call,
    If,
    ValueMap,
    Lookup,
    DynamicSourceField,
    XmlMixedContent,
    CollectionFind,
    SequenceExists,
    SequenceItemAt,
    Aggregate,
    JoinAggregate,
}

/// Complete deterministic diagnostic set for one failed lowering attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LowerError {
    diagnostics: Vec<Diagnostic>,
}

impl LowerError {
    pub(crate) fn new(diagnostics: Vec<Diagnostic>) -> Self {
        debug_assert!(!diagnostics.is_empty());
        Self { diagnostics }
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }
}

impl fmt::Display for LowerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "mapping cannot be lowered for code generation ({} diagnostic{})",
            self.diagnostics.len(),
            if self.diagnostics.len() == 1 { "" } else { "s" }
        )
    }
}

impl std::error::Error for LowerError {}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation { location, message } => write!(formatter, "{location}: {message}"),
            Self::UnsupportedProject { feature, count } => write!(
                formatter,
                "project: code generation does not support {feature} ({count} configured)"
            ),
            Self::UnsupportedScope {
                target_path,
                feature,
            } => write!(
                formatter,
                "target scope `{}`: code generation does not support {feature}",
                display_target_path(target_path)
            ),
            Self::UnsupportedNode { node, kind } => write!(
                formatter,
                "graph node {node}: code generation does not support {kind}"
            ),
        }
    }
}

fn display_target_path(path: &[String]) -> String {
    if path.is_empty() {
        "<root>".into()
    } else {
        path.join("/")
    }
}

impl fmt::Display for ProjectFeature {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ExtraSources => "extra sources; remove them or inline their values",
            Self::ExtraTargets => "extra targets; generate one primary target at a time",
            Self::FailureRules => "failure rules; remove them before generation",
        })
    }
}

impl fmt::Display for ScopeFeature {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Iteration => formatter.write_str("scope iteration"),
            Self::Construction(kind) => write!(formatter, "{kind} construction"),
            Self::Filter => formatter.write_str("scope filters"),
            Self::Grouping => formatter.write_str("scope grouping"),
            Self::Sorting => formatter.write_str("scope sorting"),
            Self::SequenceWindows => formatter.write_str("scope sequence windows"),
            Self::IterationOutput => formatter.write_str("non-default iteration output"),
            Self::DynamicBindings => formatter.write_str("dynamic target bindings"),
            Self::DynamicChildren => formatter.write_str("dynamic target child scopes"),
            Self::DynamicFieldMerge => formatter.write_str("dynamic-field merging"),
        }
    }
}

impl fmt::Display for ScopeConstructionKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::CopyCurrentSource => "copy-current-source",
            Self::Scalar => "scalar",
            Self::XmlMixedContent => "XML mixed-content",
            Self::RecursiveFilter => "recursive-filter",
            Self::PathHierarchy => "path-hierarchy",
            Self::AdjacencyTree => "adjacency-tree",
        })
    }
}

impl fmt::Display for UnsupportedNodeKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::FramedSourceField => "a frame-pinned source field",
            Self::NonFiniteFloatLiteral => "a non-finite float literal",
            Self::SourceDocumentPath => "current-document-path",
            Self::Position => "position",
            Self::JoinField => "a join field",
            Self::JoinPosition => "join position",
            Self::RuntimeValue => "a runtime value",
            Self::Call => "a function call",
            Self::If => "conditional evaluation",
            Self::ValueMap => "a value map",
            Self::Lookup => "a lookup",
            Self::DynamicSourceField => "a dynamic source field",
            Self::XmlMixedContent => "XML mixed content",
            Self::CollectionFind => "collection-find",
            Self::SequenceExists => "sequence-exists",
            Self::SequenceItemAt => "sequence item-at",
            Self::Aggregate => "an aggregate",
            Self::JoinAggregate => "a join aggregate",
        })
    }
}
