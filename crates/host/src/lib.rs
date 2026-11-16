//! Read-only inspection and validation of native Ferrule projects.
//! Resource references are metadata. These APIs never open them or run a mapping.

#![forbid(unsafe_code)]

mod inspection;
mod validation;

pub use engine::{
    ValidationEndpoint, ValidationOwner, ValidationSchemaLocation, ValidationSchemaStep,
    ValidationScopeLocation, ValidationScopeStep,
};
pub use inspection::{
    Boundary, BoundaryId, BoundaryShape, Inspection, InspectionError, InspectionLimits,
    ResourceKind, ResourceReference, StructuralCounts, inspect,
};
pub use mapping::{FileCodecError, FunctionId, NodeId, Project, project_file::ProjectFileLayout};
pub use validation::{Diagnostic, ValidationError, ValidationLimits, ValidationReport, validate};

/// A decoded project and the layout of its source file.
#[derive(Debug)]
pub struct LoadedProject {
    project: Project,
    layout: ProjectFileLayout,
}

impl LoadedProject {
    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn layout(&self) -> ProjectFileLayout {
        self.layout
    }

    /// Take ownership of the decoded model, for example to start an edit session.
    pub fn into_project(self) -> Project {
        self.project
    }
}

/// Load the original file bytes through the native codec.
///
/// The byte limit applies before decoding, including for legacy unmarked files.
pub fn load(bytes: &[u8], maximum_bytes: usize) -> Result<LoadedProject, FileCodecError> {
    if bytes.len() > maximum_bytes {
        return Err(FileCodecError::TooLarge {
            bytes: bytes.len(),
            max: maximum_bytes,
        });
    }
    let (project, layout) = mapping::project_file::decode_bytes_with_layout(bytes)?;
    Ok(LoadedProject { project, layout })
}

/// A location within the exact project snapshot used to produce the result.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum NativeLocation {
    Document,
    JsonPointer(String),
    GraphNode {
        node: NodeId,
        function: Option<FunctionId>,
    },
}

impl NativeLocation {
    fn pointer(layout: ProjectFileLayout, relative: &str, maximum_bytes: usize) -> Self {
        let prefix = layout.document_pointer_prefix();
        if prefix.len() + relative.len() > maximum_bytes {
            return Self::Document;
        }
        Self::JsonPointer(format!("{prefix}{relative}"))
    }
}

/// Display text may be shortened to fit a host's UTF-8 byte budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayText {
    pub text: String,
    pub truncated: bool,
}

impl DisplayText {
    fn bounded(text: &str, maximum_bytes: usize) -> Self {
        let end = text.floor_char_boundary(maximum_bytes);
        Self {
            text: text[..end].into(),
            truncated: end < text.len(),
        }
    }
}
