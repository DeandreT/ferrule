//! Lowered filename metadata alongside, not inside, the ordinary mapping program.

use std::fmt;

use mapping::{
    DocumentProject, GlobalFilename, NodeId, PrimaryDocumentFormat, PrimaryDocumentRoute,
    PrimaryFileSelection, PrimaryFilenameError, PrimaryRouteError,
};

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
    primary_selection: PrimaryFileSelection,
    primary_route: Result<PrimaryDocumentRoute, PrimaryRouteError>,
}

impl DocumentProgram {
    pub fn program(&self) -> &Program {
        &self.program
    }

    pub fn primary_filename(&self) -> &PrimaryFilenamePlan {
        &self.primary_filename
    }

    /// How the filename selects documents; generated document entry points
    /// pass it to the host rather than letting the host choose.
    pub fn primary_selection(&self) -> PrimaryFileSelection {
        self.primary_selection
    }

    /// The complete-document route decided from the saved format identities.
    /// Filename-only and acquisition companions do not need one.
    pub fn primary_route(&self) -> Result<PrimaryDocumentRoute, PrimaryRouteError> {
        self.primary_route.clone()
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
    let primary_route = document.primary_route();
    let project = document.project();
    let program = match primary_route {
        Ok(PrimaryDocumentRoute {
            format: PrimaryDocumentFormat::Xml,
            ..
        }) => crate::lower::lower_primary_xml_documents(project),
        _ => crate::lower(project),
    }
    .map_err(DocumentLowerError::Mapping)?;
    Ok(DocumentProgram {
        program,
        primary_filename,
        primary_selection: document.primary_source().selection,
        primary_route,
    })
}
