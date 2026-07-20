use std::fmt;

use mapping::NodeId;

/// Why lowering could not produce backend-neutral code-generation IR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diagnostic {
    Validation {
        location: String,
        message: String,
    },
    UnsupportedDynamicSource {
        source: String,
        path_expression: NodeId,
        iteration: Vec<String>,
    },
    UnsupportedFailureRule {
        /// One-based declaration index.
        rule: usize,
        feature: FailureRuleFeature,
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
    UnsupportedFunction {
        node: NodeId,
        function: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeFeature {
    Iteration,
    GeneratedSequence(UnsupportedSequenceKind),
    Construction(ScopeConstructionKind),
    Grouping,
    DynamicBindings,
    DynamicChildren,
    DynamicFieldMerge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedSequenceKind {
    TokenizeRegex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureRuleFeature {
    GeneratedSequence(UnsupportedSequenceKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeConstructionKind {
    XmlMixedContent,
    RecursiveFilter,
    PathHierarchy,
    AdjacencyTree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedNodeKind {
    SourceDocumentPath,
    JoinField,
    JoinPosition,
    DynamicSourceField,
    XmlMixedContent,
    CollectionFind,
    SequenceExists,
    SequenceItemAt,
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
            Self::UnsupportedDynamicSource {
                source,
                path_expression,
                iteration,
            } => write!(
                formatter,
                "extra source `{source}`: code generation does not support dynamic path expression {path_expression} over `{}`",
                display_source_path(iteration)
            ),
            Self::UnsupportedFailureRule { rule, feature } => write!(
                formatter,
                "failure rule {rule}: code generation does not support {feature}"
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
            Self::UnsupportedFunction { node, function } => write!(
                formatter,
                "graph node {node}: code generation does not support function `{function}`"
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

fn display_source_path(path: &[String]) -> String {
    if path.is_empty() {
        "<root>".into()
    } else {
        path.join("/")
    }
}

impl fmt::Display for FailureRuleFeature {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GeneratedSequence(kind) => write!(formatter, "{kind} generated sequence"),
        }
    }
}

impl fmt::Display for ScopeFeature {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Iteration => formatter.write_str("scope iteration"),
            Self::GeneratedSequence(kind) => {
                write!(formatter, "{kind} generated-sequence iteration")
            }
            Self::Construction(kind) => write!(formatter, "{kind} construction"),
            Self::Grouping => formatter.write_str("scope grouping"),
            Self::DynamicBindings => formatter.write_str("dynamic target bindings"),
            Self::DynamicChildren => formatter.write_str("dynamic target child scopes"),
            Self::DynamicFieldMerge => formatter.write_str("dynamic-field merging"),
        }
    }
}

impl fmt::Display for UnsupportedSequenceKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TokenizeRegex => "regular-expression tokenize",
        })
    }
}

impl fmt::Display for ScopeConstructionKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
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
            Self::SourceDocumentPath => "current-document-path",
            Self::JoinField => "a join field",
            Self::JoinPosition => "join position",
            Self::DynamicSourceField => "a dynamic source field",
            Self::XmlMixedContent => "XML mixed content",
            Self::CollectionFind => "collection-find",
            Self::SequenceExists => "sequence-exists",
            Self::SequenceItemAt => "sequence item-at",
            Self::JoinAggregate => "a join aggregate",
        })
    }
}
