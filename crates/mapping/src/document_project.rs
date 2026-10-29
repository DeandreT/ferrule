//! Explicit document acquisition metadata, separate from a legacy Project.
//!
//! This representation does not open an interpreter or generated-host capability.
//! In particular, retaining a filename expression is not permission to replace it
//! with the project's saved preview path or with caller-supplied document text.

use std::fmt;

use ir::{ScalarType, Value};
use serde::{Deserialize, Serialize};

use crate::{MAX_RUNTIME_PARAMETER_NAME_BYTES, Node, NodeId, Project};

/// Identifies the distinct document-plan artifact rather than a legacy project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentProjectFormat {
    #[serde(rename = "ferrule_document_project_v1")]
    V1,
}

/// The primary filename expression uses the embedded project's graph identities.
///
/// `selection` is omitted from the saved form when it is `Direct`, so plans
/// without it keep their original bytes. A saved wildcard selection is refused
/// by readers that predate it rather than being read as a direct filename.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrimarySourcePlan {
    pub filename: NodeId,
    #[serde(default, skip_serializing_if = "PrimaryFileSelection::is_direct")]
    pub selection: PrimaryFileSelection,
}

impl PrimarySourcePlan {
    /// A filename that names exactly one document.
    pub fn new(filename: NodeId) -> Self {
        Self {
            filename,
            selection: PrimaryFileSelection::Direct,
        }
    }

    pub fn with_selection(mut self, selection: PrimaryFileSelection) -> Self {
        self.selection = selection;
        self
    }
}

/// How the evaluated filename text selects documents. This is part of the plan,
/// not a host preference; hosts decide only where and how selected files load.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PrimaryFileSelection {
    /// The filename names one document. `*` and `?` are ordinary characters.
    #[default]
    Direct,
    /// `*` and `?` in the final path component select every matching file in
    /// one directory, in Unicode scalar order of their names. Directory
    /// components never contain wildcards.
    FileNameWildcard { empty: EmptyFileSelection },
}

impl PrimaryFileSelection {
    pub fn is_direct(&self) -> bool {
        matches!(self, Self::Direct)
    }
}

/// What a wildcard that matches no file means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmptyFileSelection {
    /// A typed selection failure; the mapping does not run.
    Refuse,
    /// The mapping runs once over zero documents.
    MapNoDocuments,
}

/// A document plan cannot be deserialized as an ordinary Project. Its embedded
/// project is read-only so ordinary graph pruning cannot discard the filename.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentProject {
    format: DocumentProjectFormat,
    project: Project,
    primary_source: PrimarySourcePlan,
    /// Explicit single-document output transport, not a mapped business field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output_filename: Option<NodeId>,
}

/// A source-independent filename. The parameter's retained preview is not a
/// normal-run value; acquisition still requires the execution host's binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalFilename<'a> {
    Literal(&'a str),
    Parameter(&'a str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimaryFilenameError {
    MissingNode { node: NodeId },
    NonString { node: NodeId },
    InvalidParameterName { node: NodeId },
    UnsupportedExpression { node: NodeId },
    InvalidEndpointText { node: NodeId },
}

impl fmt::Display for PrimaryFilenameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEndpointText { node } => write!(
                f,
                "filename endpoint node {node} requires nonempty text without wildcards or control characters"
            ),
            Self::MissingNode { node } => write!(f, "primary filename node {node} does not exist"),
            Self::NonString { node } => write!(f, "primary filename node {node} must be a string"),
            Self::InvalidParameterName { node } => {
                write!(
                    f,
                    "primary filename node {node} has an invalid parameter name"
                )
            }
            Self::UnsupportedExpression { node } => write!(
                f,
                "primary filename node {node} requires an unsupported acquisition expression"
            ),
        }
    }
}

impl std::error::Error for PrimaryFilenameError {}

impl DocumentProject {
    /// Retains the plan, including expressions that do not yet have an
    /// acquisition implementation. Callers must validate before using a profile.
    pub fn new(project: Project, primary_source: PrimarySourcePlan) -> Self {
        Self {
            format: DocumentProjectFormat::V1,
            project,
            primary_source,
            output_filename: None,
        }
    }

    pub fn with_output_filename(mut self, filename: NodeId) -> Self {
        self.output_filename = Some(filename);
        self
    }

    pub fn output_filename(&self) -> Option<NodeId> {
        self.output_filename
    }

    pub fn global_output_filename(
        &self,
    ) -> Result<Option<GlobalFilename<'_>>, PrimaryFilenameError> {
        self.output_filename
            .map(|node| self.filename(node))
            .transpose()
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn primary_source(&self) -> &PrimarySourcePlan {
        &self.primary_source
    }

    /// Classifies the initial literal/global-parameter profile without executing
    /// a graph node, reading a preview value, or opening a source document.
    pub fn global_filename(&self) -> Result<GlobalFilename<'_>, PrimaryFilenameError> {
        self.filename(self.primary_source.filename)
    }

    fn filename(&self, node: NodeId) -> Result<GlobalFilename<'_>, PrimaryFilenameError> {
        match self.project.graph.nodes.get(&node) {
            None => Err(PrimaryFilenameError::MissingNode { node }),
            Some(Node::Const {
                value: Value::String(value),
            }) => Ok(GlobalFilename::Literal(value)),
            Some(Node::Const { .. }) => Err(PrimaryFilenameError::NonString { node }),
            Some(Node::RuntimeParameter { name, ty, .. }) => {
                if *ty != ScalarType::String {
                    return Err(PrimaryFilenameError::NonString { node });
                }
                if name.is_empty()
                    || name.contains('\0')
                    || name.len() > MAX_RUNTIME_PARAMETER_NAME_BYTES
                {
                    return Err(PrimaryFilenameError::InvalidParameterName { node });
                }
                Ok(GlobalFilename::Parameter(name))
            }
            Some(_) => Err(PrimaryFilenameError::UnsupportedExpression { node }),
        }
    }

    /// The explicit single-document route never infers wildcard selection.
    /// All other path interpretation belongs to the execution host.
    pub fn validate_endpoint_text(node: NodeId, text: &str) -> Result<(), PrimaryFilenameError> {
        if text.is_empty() || text.contains(['*', '?']) || text.chars().any(char::is_control) {
            Err(PrimaryFilenameError::InvalidEndpointText { node })
        } else {
            Ok(())
        }
    }

    /// Preserve acquisition dependencies as well as all ordinary mapping roots.
    /// An unsupported expression must not disappear merely because it is refused.
    pub fn prune_unreachable_nodes(&mut self) {
        self.project.prune_unreachable_nodes_with_roots(
            std::iter::once(self.primary_source.filename).chain(self.output_filename),
        );
    }
}
