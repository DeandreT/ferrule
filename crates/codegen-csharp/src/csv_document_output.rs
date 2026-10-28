use crate::{EmitError, Options, composition::ClassPart};
use codegen::{ArtifactSet, CompanionSelection, CsvOutputPolicy, GenerationProfile, Program};

pub(crate) fn emit(
    program: &Program,
    policy: &CsvOutputPolicy,
    options: &Options,
) -> Result<ArtifactSet, EmitError> {
    codegen::validate_csv_document_output(program, policy)?;
    let mut library = crate::library_with_companions(program, CompanionSelection::TypedOnly)?;
    let mut members = super::csv_output::render_settings(program, policy);
    for (suffix, kind, serializer) in [
        ("", "Text", "Serialize"),
        ("Bytes", "Bytes", "SerializeBytes"),
    ] {
        for context in [false, true] {
            let argument = if context {
                ", global::Ferrule.Runtime.FerruleExecutionContext executionContext"
            } else {
                ""
            };
            let call = if context { ", executionContext" } else { "" };
            members.push_str(&format!(
                "    public static global::System.Collections.Generic.IReadOnlyList<global::Ferrule.Runtime.FerruleCsv{kind}Document> ExecuteCsvDocuments{suffix}(global::Ferrule.Runtime.FerruleInstance source{argument})\n    {{\n        var output = Execute(source{call});\n        return global::Ferrule.Runtime.FerruleCsvDocuments.{serializer}(output, CsvOutputFields, CsvOutputPolicy);\n    }}\n\n"
            ));
        }
    }
    library.add_companion(
        "GeneratedMapping.CsvDocuments.cs",
        ClassPart::members(members),
    );
    library.add_runtime(&[
        (
            "Runtime/FerruleCsv.cs",
            include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleCsv.cs"),
        ),
        (
            "Runtime/FerruleCsvDocuments.cs",
            include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleCsvDocuments.cs"),
        ),
    ]);
    library.into_artifacts_with(options, GenerationProfile::CsvDocumentOutput)
}
