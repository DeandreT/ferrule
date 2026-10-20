//! Lowered filename metadata alongside, not inside, the ordinary mapping program.

use std::fmt;

use mapping::{DocumentProject, GlobalFilename, NodeId, PrimaryFilenameError};

use crate::{LowerError, Program};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrimaryFilenamePlan {
    Literal { node: NodeId, text: String },
    Parameter { node: NodeId, name: String },
}

/// Retains the primary filename through lowering. This is not an acquisition
/// policy or a complete document execution profile.
#[derive(Debug, Clone)]
pub struct DocumentProgram {
    program: Program,
    primary_filename: PrimaryFilenamePlan,
}

impl DocumentProgram {
    pub fn program(&self) -> &Program {
        &self.program
    }

    pub fn primary_filename(&self) -> &PrimaryFilenamePlan {
        &self.primary_filename
    }
}

#[derive(Debug)]
pub enum DocumentLowerError {
    Filename(PrimaryFilenameError),
    Mapping(LowerError),
}

impl fmt::Display for DocumentLowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Filename(error) => error.fmt(f),
            Self::Mapping(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for DocumentLowerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Filename(error) => error,
            Self::Mapping(error) => error,
        })
    }
}

/// Retain a classified filename and apply the unchanged ordinary mapping gates.
/// Unsupported filename expressions fail before lowering the mapping body.
pub fn lower_document(document: &DocumentProject) -> Result<DocumentProgram, DocumentLowerError> {
    let node = document.primary_source().filename;
    let primary_filename = match document
        .global_filename()
        .map_err(DocumentLowerError::Filename)?
    {
        GlobalFilename::Literal(text) => PrimaryFilenamePlan::Literal {
            node,
            text: text.into(),
        },
        GlobalFilename::Parameter(name) => PrimaryFilenamePlan::Parameter {
            node,
            name: name.into(),
        },
    };
    let program = crate::lower(document.project()).map_err(DocumentLowerError::Mapping)?;
    Ok(DocumentProgram {
        program,
        primary_filename,
    })
}
