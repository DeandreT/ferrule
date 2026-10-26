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
    source_options: mapping::FormatOptions,
    json_source_projection: Option<mapping::JsonSourceShape>,
    native_x12_nullable_binding: Option<String>,
}

impl DocumentProgram {
    pub fn program(&self) -> &Program {
        &self.program
    }

    pub fn primary_filename(&self) -> &PrimaryFilenamePlan {
        &self.primary_filename
    }

    pub fn source_options(&self) -> &mapping::FormatOptions {
        &self.source_options
    }

    pub fn json_source_projection(&self) -> Option<&mapping::JsonSourceShape> {
        self.json_source_projection.as_ref()
    }

    /// Exact source descriptor admitted by the explicit native lowering entry.
    /// Ordinary document lowering never supplies this capability marker.
    pub fn native_x12_nullable_binding(&self) -> Option<&str> {
        self.native_x12_nullable_binding.as_deref()
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
    lower_document_using(document, crate::lower, None)
}

/// Explicit selected-message lowering, not an extension of ordinary or Basic
/// ownership admission. The resulting document is bound to this exact native
/// source descriptor for subsequent native-only emission.
pub fn lower_native_x12_nullable_document(
    document: &DocumentProject,
    source: &crate::NativeX12Source<'_>,
) -> Result<DocumentProgram, DocumentLowerError> {
    lower_document_using(
        document,
        |project| crate::lower::lower_with_native_x12_nullable_scalars(project, source),
        Some(source),
    )
}

fn lower_document_using(
    document: &DocumentProject,
    lower: impl Fn(&mapping::Project) -> Result<Program, LowerError>,
    native: Option<&crate::NativeX12Source<'_>>,
) -> Result<DocumentProgram, DocumentLowerError> {
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
    let program = lower(document.project()).map_err(DocumentLowerError::Mapping)?;
    let native_x12_nullable_binding = native
        .map(|source| crate::prepare_native_x12_source(&program, source))
        .transpose()
        .map_err(|error| {
            DocumentLowerError::Mapping(LowerError::new(vec![crate::Diagnostic::Validation {
                location: "native X12 selected-message profile".into(),
                message: error.to_string(),
            }]))
        })?;
    Ok(DocumentProgram {
        program,
        primary_filename,
        source_options: document.project().source_options.clone(),
        json_source_projection: document.json_source_projection().cloned(),
        native_x12_nullable_binding,
    })
}
