use std::fmt;

use codegen::{
    ArtifactSet, CsvX12BoundaryError, CsvX12BoundaryPolicy, CsvX12BoundaryProfile, Program,
};
use ir::ScalarType;

use crate::{EmitError, file, literal};

const INPUT_SOURCE: &str =
    include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleCsvInput.cs");

/// A refusal from explicitly selected flat CSV input and singular X12 output.
#[derive(Debug)]
pub enum CsvX12EmitError {
    Policy(Box<CsvX12BoundaryError>),
    Ordinary(EmitError),
    ArtifactInvariant { path: &'static str },
}

impl fmt::Display for CsvX12EmitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Policy(error) => write!(formatter, "CSV/X12 boundary: {error}"),
            Self::Ordinary(error) => write!(formatter, "CSV/X12 typed emission: {error}"),
            Self::ArtifactInvariant { path } => {
                write!(
                    formatter,
                    "CSV/X12 emission requires the typed artifact {path}"
                )
            }
        }
    }
}

impl std::error::Error for CsvX12EmitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Policy(error) => Some(error.as_ref()),
            Self::Ordinary(error) => Some(error),
            Self::ArtifactInvariant { .. } => None,
        }
    }
}

impl From<EmitError> for CsvX12EmitError {
    fn from(error: EmitError) -> Self {
        Self::Ordinary(error)
    }
}

pub(crate) fn emit(
    program: &Program,
    policy: &CsvX12BoundaryPolicy,
) -> Result<ArtifactSet, CsvX12EmitError> {
    let profile = codegen::prepare_csv_x12_boundary(program, policy)
        .map_err(|error| CsvX12EmitError::Policy(Box::new(error)))?;
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
            "    <Compile Include=\"GeneratedMapping.cs\" />\n    <Compile Include=\"GeneratedMapping.CsvX12.cs\" />\n",
        ),
    ] {
        let generated = files
            .iter_mut()
            .find(|generated| generated.path.as_str() == path)
            .ok_or(CsvX12EmitError::ArtifactInvariant { path })?;
        let source = std::str::from_utf8(&generated.contents)
            .map_err(|_| CsvX12EmitError::ArtifactInvariant { path })?;
        if source.matches(before).count() != 1 {
            return Err(CsvX12EmitError::ArtifactInvariant { path });
        }
        generated.contents = source.replacen(before, after, 1).into_bytes();
    }
    files.push(file("GeneratedMapping.CsvX12.cs", render(&profile))?);
    files.push(file("Runtime/FerruleCsvInput.cs", INPUT_SOURCE)?);
    for (path, source) in crate::x12_output::X12_SOURCES {
        files.push(file(path, source)?);
    }
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(CsvX12EmitError::from)
}

fn render(profile: &CsvX12BoundaryProfile) -> String {
    let mut output = String::from(
        "namespace Ferrule.Generated;\n\npublic static partial class GeneratedMapping\n{\n    private static readonly global::Ferrule.Runtime.FerruleCsvReadField[] CsvInputFields =\n        new global::Ferrule.Runtime.FerruleCsvReadField[]\n        {\n",
    );
    for field in &profile.fields {
        output.push_str("            new(");
        output.push_str(&literal::string(&field.name));
        output.push_str(", global::Ferrule.Runtime.FerruleScalarType.");
        output.push_str(match field.ty {
            ScalarType::String => "String",
            ScalarType::Int => "Int64",
            ScalarType::Float => "Double",
            ScalarType::Bool => "Bool",
        });
        output.push_str("),\n");
    }
    output.push_str(
        "        };\n\n    private static readonly global::Ferrule.Runtime.FerruleCsvReadOptions CsvInputOptions =\n        new global::Ferrule.Runtime.FerruleCsvReadOptions\n        {\n",
    );
    output.push_str(&format!(
        "            Delimiter = {},\n            Quote = {},\n            QuoteDisabled = {},\n            HasHeaders = {},\n            PreserveEmptyStrings = {},\n        }};\n\n    private const string CsvX12TargetDescriptor = ",
        character(profile.source.delimiter),
        character(profile.source.quote),
        profile.source.quote_disabled,
        profile.source.has_headers,
        profile.source.preserve_empty_strings,
    ));
    output.push_str(&literal::string(&profile.target_descriptor));
    output.push_str(";\n");
    for bytes in [false, true] {
        let suffix = if bytes { "Bytes" } else { "" };
        let ty = if bytes { "byte[]" } else { "string" };
        output.push_str(&format!(
            "\n    public static global::Ferrule.Runtime.FerruleInstance ParseCsv{suffix}({ty} source)\n    {{\n        return global::Ferrule.Runtime.FerruleCsvInput.Parse{suffix}(source, CsvInputFields, CsvInputOptions);\n    }}\n",
        ));
        for contextual in [false, true] {
            let context_suffix = if contextual { "WithContext" } else { "" };
            output.push_str(&format!(
                "\n    public static {ty} ExecuteCsvToX12{suffix}{context_suffix}(\n        {ty} source",
            ));
            if contextual {
                output.push_str(
                    ",\n        global::Ferrule.Runtime.FerruleExecutionContext executionContext",
                );
            }
            output.push_str(
                ")\n    {\n        global::System.ArgumentNullException.ThrowIfNull(source);\n",
            );
            if contextual {
                output.push_str(
                    "        global::System.ArgumentNullException.ThrowIfNull(executionContext);\n",
                );
            }
            output.push_str(&format!("        var input = ParseCsv{suffix}(source);\n"));
            output.push_str(if contextual {
                "        var target = Execute(input, executionContext);\n"
            } else {
                "        var target = Execute(input);\n"
            });
            output.push_str(&format!(
                "        return global::Ferrule.Runtime.FerruleX12.SerializeEmbedded{suffix}(CsvX12TargetDescriptor, target{});\n    }}\n",
                if contextual { ", executionContext" } else { "" },
            ));
        }
    }
    output.push_str("}\n");
    output
}

fn character(value: Option<char>) -> String {
    value.map_or_else(
        || "null".into(),
        |value| format!("(char){}", u32::from(value)),
    )
}
