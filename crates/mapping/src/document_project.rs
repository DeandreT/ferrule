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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrimarySourcePlan {
    pub filename: NodeId,
}

/// A document plan cannot be deserialized as an ordinary Project. Its embedded
/// project is read-only so ordinary graph pruning cannot discard the filename.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentProject {
    format: DocumentProjectFormat,
    project: Project,
    primary_source: PrimarySourcePlan,
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
}

impl fmt::Display for PrimaryFilenameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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
        }
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
        let node = self.primary_source.filename;
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

    /// Preserve acquisition dependencies as well as all ordinary mapping roots.
    /// An unsupported expression must not disappear merely because it is refused.
    pub fn prune_unreachable_nodes(&mut self) {
        self.project
            .prune_unreachable_nodes_with_roots([self.primary_source.filename]);
    }
}
