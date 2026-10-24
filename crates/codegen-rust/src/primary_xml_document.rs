use std::fmt;

use codegen::{ArtifactSet, DocumentProgram, PrimaryDocumentBoundaryError, XmlDocumentInputs};
use mapping::PrimaryDocumentShape;

use crate::{EmitError, Options};

#[derive(Debug)]
pub enum PrimaryDocumentEmitError {
    Boundary(PrimaryDocumentBoundaryError),
    Emit(EmitError),
}

impl fmt::Display for PrimaryDocumentEmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Boundary(error) => error.fmt(f),
            Self::Emit(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for PrimaryDocumentEmitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Boundary(error) => error,
            Self::Emit(error) => error,
        })
    }
}

/// The `PRIMARY_DOCUMENT_SHAPE` item for a document-format route.
pub(crate) fn shape_constant(shape: PrimaryDocumentShape) -> String {
    let shape = match shape {
        PrimaryDocumentShape::Rows => "Rows",
        PrimaryDocumentShape::Document => "Document",
        PrimaryDocumentShape::DocumentSet => "DocumentSet",
    };
    format!(
        "\n/// How the selected documents form the one mapping source.\npub const PRIMARY_DOCUMENT_SHAPE: codegen_runtime::PrimaryDocumentShape = codegen_runtime::PrimaryDocumentShape::{shape};\n"
    )
}

/// Add one complete primary XML document entry point to the ordinary library
/// and its single-document XML adapter: the embedded filename plan and
/// selection mode go to a host that selects and loads; every selected document
/// is parsed into the plan's one document or document set before the ordinary
/// mapper runs once; the caller publishes the returned bytes.
pub fn emit_with_primary_xml_documents(
    document: &DocumentProgram,
    options: &Options,
) -> Result<ArtifactSet, PrimaryDocumentEmitError> {
    let inputs = codegen::validate_primary_xml_documents(document)
        .map_err(PrimaryDocumentEmitError::Boundary)?;
    let shape = document
        .primary_route()
        .map_err(|error| {
            PrimaryDocumentEmitError::Boundary(PrimaryDocumentBoundaryError::Route(error))
        })?
        .shape;
    let mut files = crate::emit(document.program(), options)
        .map_err(PrimaryDocumentEmitError::Emit)?
        .into_files();
    let parse = match inputs {
        XmlDocumentInputs::RootViewPrimary => {
            let policy = &codegen::prepare_boundary_emission(
                document.program(),
                codegen::CompanionSelection::Complete,
            )
            .map_err(|error| {
                PrimaryDocumentEmitError::Boundary(PrimaryDocumentBoundaryError::Program(error))
            })?;
            let input = &policy
                .xml()
                .ok_or(PrimaryDocumentEmitError::Boundary(
                    PrimaryDocumentBoundaryError::XmlAdapter,
                ))?
                .policy()
                .input;
            format!(
                "codegen_runtime::parse_xml_bytes(SOURCE_XML_SCHEMA, &data.bytes, {}, {})",
                input.allow_inactive_root_type_members, input.root_view_policy
            )
        }
        _ => "codegen_runtime::parse_structured_xml_bytes(SOURCE_XML_SCHEMA, &data.bytes)"
            .to_string(),
    };
    let mut code = crate::primary_filename::evaluator(document);
    code.push_str(&crate::primary_filename::selection_constant(document));
    code.push_str(&shape_constant(shape));
    code.push_str(&format!("\n/// Evaluate the primary filename, let `host` select and load, parse every\n/// selected XML document, then map once over the plan's document or document\n/// set and return one complete XML document. The first acquisition failure\n/// stops before later loads and before mapping. Publishing the bytes belongs\n/// to the caller.\npub fn execute_primary_xml_document_bytes<H: codegen_runtime::PrimarySourceHost>(\n    execution: &ExecutionContext<'_>,\n    host: &H,\n    maximum: usize,\n) -> Result<Vec<u8>, codegen_runtime::PrimaryXmlDocumentError<H::Error>> {{\n    let documents = codegen_runtime::acquire_primary_documents(\n        || evaluate_primary_filename(execution),\n        PRIMARY_FILE_SELECTION,\n        host,\n        maximum,\n        |_, data| {parse},\n    )\n    .map_err(codegen_runtime::PrimaryXmlDocumentError::Acquisition)?;\n    let source = codegen_runtime::assemble_primary_documents(PRIMARY_DOCUMENT_SHAPE, documents)\n        .map_err(codegen_runtime::PrimaryXmlDocumentError::Assembly)?;\n    let mapped = execute_outputs_with_context(&source, execution)\n        .map_err(codegen_runtime::XmlBoundaryError::from)\n        .map_err(codegen_runtime::XmlOutputSetError::from)\n        .map_err(codegen_runtime::PrimaryXmlDocumentError::Output)?;\n    serialize_xml_bytes_outputs(mapped)\n        .map(|outputs| outputs.primary)\n        .map_err(codegen_runtime::PrimaryXmlDocumentError::Output)\n}}\n"));
    files
        .iter_mut()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .expect("ordinary Rust mapping artifact")
        .contents
        .extend_from_slice(code.as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(PrimaryDocumentEmitError::Emit)
}
