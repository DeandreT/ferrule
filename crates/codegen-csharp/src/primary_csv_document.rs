use codegen::{ArtifactSet, CsvDocumentBoundaryPolicy, DocumentProgram};
use mapping::{EmptyFileSelection, PrimaryFileSelection};

use crate::CsvDocumentEmitError;
use crate::composition::ClassPart;

/// Add one complete primary CSV document entry point to the CSV document
/// adapters: the embedded filename plan and selection mode go to a host that
/// selects and loads; every selected document is parsed before the ordinary
/// mapper runs once; the caller publishes the returned bytes.
pub fn emit_with_primary_csv_documents(
    document: &DocumentProgram,
    policy: &CsvDocumentBoundaryPolicy,
) -> Result<ArtifactSet, CsvDocumentEmitError> {
    let mut library = crate::csv_document::library(document.program(), policy)?;
    crate::primary_filename::add_evaluator(&mut library, document);
    let selection = match document.primary_selection() {
        PrimaryFileSelection::Direct => {
            "global::Ferrule.Runtime.FerrulePrimaryFileSelection.Direct".to_string()
        }
        PrimaryFileSelection::FileNameWildcard { empty } => format!(
            "global::Ferrule.Runtime.FerrulePrimaryFileSelection.FileNameWildcard(global::Ferrule.Runtime.FerruleEmptyFileSelection.{})",
            match empty {
                EmptyFileSelection::Refuse => "Refuse",
                EmptyFileSelection::MapNoDocuments => "MapNoDocuments",
            }
        ),
    };
    let code = format!(
        "    /// <summary>The plan's selection mode; hosts receive it rather than choosing one.</summary>\n    public static readonly global::Ferrule.Runtime.FerrulePrimaryFileSelection PrimaryFileSelection = {selection};\n\n    /// <summary>\n    /// Evaluate the primary filename, let the host select and load, parse every selected\n    /// document, then map once over all rows and return one complete CSV document. The first\n    /// acquisition failure stops before later loads and before mapping. Publishing the bytes\n    /// belongs to the caller.\n    /// </summary>\n    public static byte[] ExecutePrimaryCsvDocumentBytes(global::Ferrule.Runtime.FerruleExecutionContext execution,\n        global::Ferrule.Runtime.IFerrulePrimarySourceHost host, global::Ferrule.Runtime.FerruleCsvByteDecodingPolicy decoding, int maximum)\n    {{\n        global::System.ArgumentNullException.ThrowIfNull(execution);\n        global::System.ArgumentNullException.ThrowIfNull(host);\n        var input = global::Ferrule.Runtime.FerrulePrimaryDocuments.AcquireCsvRows(() => EvaluatePrimaryFilename(execution),\n            PrimaryFileSelection, host, CsvInputFields, CsvInputOptions, decoding, maximum);\n        return ExecuteCsvBytes(input, execution);\n    }}\n"
    );
    library.add_companion(
        "GeneratedMapping.PrimaryCsvDocument.cs",
        ClassPart::members(code),
    );
    library.add_runtime(&[
        (
            "Runtime/FerruleCsvAcquisition.cs",
            include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleCsvAcquisition.cs"),
        ),
        (
            "Runtime/FerrulePrimaryDocuments.cs",
            include_str!("../../../runtime/csharp/Ferrule.Runtime/FerrulePrimaryDocuments.cs"),
        ),
        (
            "Runtime/FerruleLocalPrimaryFiles.cs",
            include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleLocalPrimaryFiles.cs"),
        ),
    ]);
    library.into_artifacts().map_err(CsvDocumentEmitError::Emit)
}
