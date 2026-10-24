use std::fmt;

use codegen::{
    ArtifactSet, DocumentProgram, GenerationProfile, PrimaryDocumentBoundaryError,
    XmlDocumentInputs,
};
use mapping::PrimaryDocumentShape;

use crate::composition::ClassPart;
use crate::primary_csv_document::{PRIMARY_DOCUMENT_RUNTIME, selection_literal};
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

/// The C# literal of the plan's document shape.
pub(crate) fn shape_literal(shape: PrimaryDocumentShape) -> &'static str {
    match shape {
        PrimaryDocumentShape::Rows => "global::Ferrule.Runtime.FerruleDocumentShape.Rows",
        PrimaryDocumentShape::Document => "global::Ferrule.Runtime.FerruleDocumentShape.Document",
        PrimaryDocumentShape::DocumentSet => {
            "global::Ferrule.Runtime.FerruleDocumentShape.DocumentSet"
        }
    }
}

/// The CSV runtime the shared acquisition reuses for its request checks.
pub(crate) const PRIMARY_DOCUMENT_CSV_RUNTIME: [(&str, &str); 2] = [
    (
        "Runtime/FerruleCsvInput.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleCsvInput.cs"),
    ),
    (
        "Runtime/FerruleCsvSource.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleCsvSource.cs"),
    ),
];

/// Add one complete primary XML document entry point to the ordinary library
/// and its single-document XML adapter: the embedded filename plan and
/// selection mode go to a host that selects and loads; every selected document
/// is parsed into the plan's one document or document set before the ordinary
/// mapper runs once; the caller publishes the returned bytes.
pub fn emit_with_primary_xml_documents(
    document: &DocumentProgram,
) -> Result<ArtifactSet, PrimaryDocumentEmitError> {
    emit(document, &Options::default())
}

pub(crate) fn emit(
    document: &DocumentProgram,
    options: &Options,
) -> Result<ArtifactSet, PrimaryDocumentEmitError> {
    let boundary = PrimaryDocumentEmitError::Boundary;
    let inputs = codegen::validate_primary_xml_documents(document).map_err(boundary)?;
    let shape = document
        .primary_route()
        .map_err(|error| boundary(PrimaryDocumentBoundaryError::Route(error)))?
        .shape;
    let parse = if inputs == XmlDocumentInputs::RootViewPrimary {
        let plan = codegen::prepare_boundary_emission(
            document.program(),
            codegen::CompanionSelection::Complete,
        )
        .map_err(|error| boundary(PrimaryDocumentBoundaryError::Program(error)))?;
        let input = &plan
            .xml()
            .ok_or(boundary(PrimaryDocumentBoundaryError::XmlAdapter))?
            .policy()
            .input;
        format!(
            "global::Ferrule.Runtime.FerruleXml.ParseEmbeddedBytes(SourceXmlSchema, data.Bytes, {}, {})",
            input.allow_inactive_root_type_members, input.root_view_policy
        )
    } else {
        "global::Ferrule.Runtime.FerruleXml.ParseStructuredEmbeddedBytes(SourceXmlSchema, data.Bytes)"
            .to_string()
    };
    let mut library = crate::library(document.program()).map_err(PrimaryDocumentEmitError::Emit)?;
    crate::primary_filename::add_evaluator(&mut library, document);
    let code = format!(
        "    /// <summary>The plan's selection mode; hosts receive it rather than choosing one.</summary>\n    public static readonly global::Ferrule.Runtime.FerrulePrimaryFileSelection PrimaryFileSelection = {selection};\n\n    /// <summary>How the selected documents form the one mapping source.</summary>\n    public const global::Ferrule.Runtime.FerruleDocumentShape PrimaryDocumentShape = {shape};\n\n    /// <summary>\n    /// Evaluate the primary filename, let the host select and load, parse every selected XML\n    /// document, then map once over the plan's document or document set and return one\n    /// complete XML document. The first acquisition failure stops before later loads and\n    /// before mapping. Publishing the bytes belongs to the caller.\n    /// </summary>\n    public static byte[] ExecutePrimaryXmlDocumentBytes(global::Ferrule.Runtime.FerruleExecutionContext execution,\n        global::Ferrule.Runtime.IFerrulePrimarySourceHost host, int maximum)\n    {{\n        global::System.ArgumentNullException.ThrowIfNull(execution);\n        global::System.ArgumentNullException.ThrowIfNull(host);\n        var source = global::Ferrule.Runtime.FerrulePrimaryDocuments.AcquireDocuments(() => EvaluatePrimaryFilename(execution),\n            PrimaryFileSelection, host, PrimaryDocumentShape, maximum, data => {parse});\n        ExecutionOutputs mapped;\n        try {{ mapped = ExecuteOutputs(source, execution); }}\n        catch (global::Ferrule.Runtime.FerruleRuntimeException error)\n        {{\n            throw new global::Ferrule.Runtime.FerruleXmlOutputSetException(null, new global::Ferrule.Runtime.FerruleXmlBoundaryException(\n                global::Ferrule.Runtime.FerruleXmlBoundaryErrorKind.Mapping, error.Message, error));\n        }}\n        return SerializeXmlBytesOutputs(mapped).Primary;\n    }}\n",
        selection = selection_literal(document),
        shape = shape_literal(shape),
    );
    library.add_companion(
        "GeneratedMapping.PrimaryXmlDocument.cs",
        ClassPart::members(code),
    );
    library.add_runtime(&PRIMARY_DOCUMENT_CSV_RUNTIME);
    library.add_runtime(&PRIMARY_DOCUMENT_RUNTIME);
    library
        .into_artifacts_with(options, GenerationProfile::PrimaryXmlDocument)
        .map_err(PrimaryDocumentEmitError::Emit)
}
