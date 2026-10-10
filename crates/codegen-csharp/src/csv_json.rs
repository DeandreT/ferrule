use std::fmt;

use codegen::{
    ArtifactSet, CsvJsonBoundaryError, CsvJsonBoundaryPolicy, CsvJsonBoundaryProfile, Program,
};
use ir::ScalarType;

use crate::{EmitError, file, literal};

const INPUT_SOURCE: &str =
    include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleCsvInput.cs");

/// Failures from explicitly selected flat CSV input and own JSON output.
#[derive(Debug)]
pub enum CsvJsonEmitError {
    Policy(Box<CsvJsonBoundaryError>),
    Ordinary(EmitError),
    ArtifactInvariant { path: &'static str },
}

impl fmt::Display for CsvJsonEmitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Policy(error) => write!(formatter, "CSV/JSON boundary: {error}"),
            Self::Ordinary(error) => write!(formatter, "CSV/JSON typed emission: {error}"),
            Self::ArtifactInvariant { path } => write!(
                formatter,
                "CSV/JSON emission requires the typed artifact {path}"
            ),
        }
    }
}

impl std::error::Error for CsvJsonEmitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Policy(error) => Some(error.as_ref()),
            Self::Ordinary(error) => Some(error),
            Self::ArtifactInvariant { .. } => None,
        }
    }
}

impl From<EmitError> for CsvJsonEmitError {
    fn from(error: EmitError) -> Self {
        Self::Ordinary(error)
    }
}

pub(crate) fn emit(
    program: &Program,
    policy: &CsvJsonBoundaryPolicy,
) -> Result<ArtifactSet, CsvJsonEmitError> {
    let profile = codegen::prepare_csv_json_boundary(program, policy)
        .map_err(|error| CsvJsonEmitError::Policy(Box::new(error)))?;
    let mut files = crate::emit_typed(program)?.into_files();
    for (path, before, after) in [
        (
            "GeneratedMapping.cs",
            "public static class GeneratedMapping\n{",
            "public static partial class GeneratedMapping\n{",
        ),
        (
            "Ferrule.Generated.csproj",
            "    <Compile Include=\"GeneratedMapping.cs\" />\n",
            "    <Compile Include=\"GeneratedMapping.cs\" />\n    <Compile Include=\"GeneratedMapping.CsvJson.cs\" />\n",
        ),
    ] {
        let generated = files
            .iter_mut()
            .find(|generated| generated.path.as_str() == path)
            .ok_or(CsvJsonEmitError::ArtifactInvariant { path })?;
        let source = std::str::from_utf8(&generated.contents)
            .map_err(|_| CsvJsonEmitError::ArtifactInvariant { path })?;
        if source.matches(before).count() != 1 {
            return Err(CsvJsonEmitError::ArtifactInvariant { path });
        }
        generated.contents = source.replacen(before, after, 1).into_bytes();
    }
    files.push(file("GeneratedMapping.CsvJson.cs", render(&profile))?);
    files.push(file("Runtime/FerruleCsvInput.cs", INPUT_SOURCE)?);
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(CsvJsonEmitError::from)
}

fn character(value: Option<char>) -> String {
    value.map_or_else(
        || "null".into(),
        |value| format!("(char){}", u32::from(value)),
    )
}

fn render(profile: &CsvJsonBoundaryProfile) -> String {
    let mut output = String::from("namespace Ferrule.Generated;\n\n");
    output.push_str(crate::document_output_types::OUTPUT_TYPES);
    output.push_str("public static partial class GeneratedMapping\n{\n    private static readonly global::Ferrule.Runtime.FerruleCsvReadField[] CsvInputFields =\n        new global::Ferrule.Runtime.FerruleCsvReadField[]\n        {\n");
    for field in &profile.fields {
        let ty = match field.ty {
            ScalarType::String => "String",
            ScalarType::Int => "Int64",
            ScalarType::Float => "Double",
            ScalarType::Bool => "Bool",
        };
        output.push_str(&format!(
            "            new({}, global::Ferrule.Runtime.FerruleScalarType.{ty}),\n",
            literal::string(&field.name)
        ));
    }
    output.push_str("        };\n\n    private static readonly global::Ferrule.Runtime.FerruleCsvReadOptions CsvInputOptions =\n        new global::Ferrule.Runtime.FerruleCsvReadOptions\n        {\n");
    output.push_str(&format!("            Delimiter = {},\n            Quote = {},\n            QuoteDisabled = {},\n            HasHeaders = {},\n            PreserveEmptyStrings = {},\n        }};\n\n    private const string CsvJsonTargetDescriptor = {};\n",
        character(profile.source.delimiter), character(profile.source.quote), profile.source.quote_disabled,
        profile.source.has_headers, profile.source.preserve_empty_strings, literal::string(&profile.target_descriptor)));
    if !profile.extra_target_descriptors.is_empty() {
        output.push_str(
            "    private static readonly string[] ExtraCsvJsonTargetDescriptors =\n    [\n",
        );
        for target in &profile.extra_target_descriptors {
            output.push_str(&format!(
                "        {},\n",
                literal::string(&target.descriptor)
            ));
        }
        output.push_str("    ];\n");
    }
    for bytes in [false, true] {
        let suffix = if bytes { "Bytes" } else { "" };
        let document = if bytes { "byte[]" } else { "string" };
        let result = if bytes {
            "SelectedDocumentBytesTargetOutput"
        } else {
            "SelectedDocumentTargetOutput"
        };
        output.push_str(&format!("\n    public static global::Ferrule.Runtime.FerruleInstance ParseCsv{suffix}({document} source)\n    {{\n        return global::Ferrule.Runtime.FerruleCsvInput.Parse{suffix}(source, CsvInputFields, CsvInputOptions);\n    }}\n"));
        for contextual in [false, true] {
            let context_suffix = if contextual { "WithHost" } else { "" };
            output.push_str(&format!("\n    public static {result} ExecuteCsvJson{suffix}SelectedTarget{context_suffix}(\n        {document} source,\n        global::Ferrule.Runtime.FerruleTargetSelection selection"));
            if contextual {
                output.push_str(
                    ",\n        global::Ferrule.Runtime.FerruleExecutionContext executionContext",
                );
            }
            output.push_str(" )\n    {\n        global::System.ArgumentNullException.ThrowIfNull(source);\n        global::System.ArgumentNullException.ThrowIfNull(selection);\n        var selected = ResolveTarget(selection);\n");
            if contextual {
                output.push_str(
                    "        global::System.ArgumentNullException.ThrowIfNull(executionContext);\n",
                );
            }
            output.push_str(&format!("        var input = ParseCsv{suffix}(source);\n        var target = ExecuteSelectedTargetWithHost(input, selection, global::System.Array.Empty<NamedInput>(){});\n        return SerializeCsvJson{suffix}SelectedTarget(target, selected);\n    }}\n", if contextual { ", executionContext" } else { "" }));
        }
        render_serializer(profile, bytes, &mut output);
    }
    output.push_str("}\n");
    output
}

fn render_serializer(profile: &CsvJsonBoundaryProfile, bytes: bool, output: &mut String) {
    let suffix = if bytes { "Bytes" } else { "" };
    let result = if bytes {
        "SelectedDocumentBytesTargetOutput"
    } else {
        "SelectedDocumentTargetOutput"
    };
    let named = if bytes {
        "NamedDocumentBytesOutput"
    } else {
        "NamedDocumentOutput"
    };
    output.push_str(&format!("\n    private static {result} SerializeCsvJson{suffix}SelectedTarget(SelectedTargetOutput outcome, int selected)\n    {{\n        switch (selected)\n        {{\n            case 0 when outcome is SelectedTargetOutput.Primary primary:\n                return new {result}.Primary(DocumentBoundaryFormat.Json, global::Ferrule.Runtime.FerruleJson.SerializeEmbedded{suffix}(CsvJsonTargetDescriptor, primary.Instance));\n"));
    for (index, target) in profile.extra_target_descriptors.iter().enumerate() {
        let selected = index + 1;
        output.push_str(&format!("            case {selected} when outcome is SelectedTargetOutput.Named named_{selected} &&\n                global::System.String.Equals(named_{selected}.Output.Name, {}, global::System.StringComparison.Ordinal):\n                return new {result}.Named(new {named}(named_{selected}.Output.Name, DocumentBoundaryFormat.Json, global::Ferrule.Runtime.FerruleJson.SerializeEmbedded{suffix}(ExtraCsvJsonTargetDescriptors[{index}], named_{selected}.Output.Instance)));\n", literal::string(&target.name)));
    }
    output.push_str("            default:\n                throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                    global::Ferrule.Runtime.FerruleRuntimeError.JsonBoundary,\n                    \"generated mapping returned a target that does not match the resolved selection\");\n        }\n    }\n");
}
