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
    edi: Option<Box<EdiDocumentPolicy>>,
    fixed_width: Option<FixedWidthDocumentPolicy>,
    program: Program,
    primary_filename: PrimaryFilenamePlan,
    primary_selection: PrimaryFileSelection,
    primary_route: Result<PrimaryDocumentRoute, PrimaryRouteError>,
}

impl DocumentProgram {
    pub fn edi_policy(&self) -> Option<&EdiDocumentPolicy> {
        self.edi.as_deref()
    }
    pub fn fixed_width_policy(&self) -> Option<&FixedWidthDocumentPolicy> {
        self.fixed_width.as_ref()
    }
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

#[derive(Debug, Clone)]
pub struct EdiDocumentPolicy {
    pub kind: mapping::EdiBoundaryKind,
    pub source: mapping::FormatOptions,
    pub target: mapping::FormatOptions,
}

#[derive(Debug, Clone)]
pub struct FixedWidthDocumentPolicy {
    pub source: mapping::FixedWidthLayout,
    pub target: mapping::FixedWidthLayout,
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
        Ok(PrimaryDocumentRoute {
            format: PrimaryDocumentFormat::Json | PrimaryDocumentFormat::Edi(_),
            ..
        }) => crate::lower::lower_primary_json_documents(document),
        _ => crate::lower(project),
    }
    .map_err(DocumentLowerError::Mapping)?;
    Ok(DocumentProgram {
        edi: match &primary_route {
            Ok(PrimaryDocumentRoute {
                format: PrimaryDocumentFormat::Edi(kind),
                ..
            }) => Some(Box::new(EdiDocumentPolicy {
                kind: *kind,
                source: project.source_options.clone(),
                target: project.target_options.clone(),
            })),
            _ => None,
        },
        fixed_width: if primary_route
            .as_ref()
            .is_ok_and(|route| route.format == PrimaryDocumentFormat::FixedWidth)
        {
            project
                .source_options
                .fixed_width
                .clone()
                .zip(project.target_options.fixed_width.clone())
                .map(|(source, target)| FixedWidthDocumentPolicy { source, target })
        } else {
            None
        },
        program,
        primary_filename,
        primary_selection: document.primary_source().selection,
        primary_route,
    })
}
