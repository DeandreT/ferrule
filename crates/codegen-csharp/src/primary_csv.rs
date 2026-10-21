use crate::{EmitError, file, literal};
use codegen::{ArtifactSet, CsvInputBoundaryError, CsvInputPolicy, DocumentProgram};
use ir::{ScalarType, SchemaKind};
use std::fmt;

#[derive(Debug)]
pub enum PrimaryCsvEmitError {
    Input(CsvInputBoundaryError),
    Emit(EmitError),
    ArtifactInvariant,
}
impl fmt::Display for PrimaryCsvEmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(error) => error.fmt(f),
            Self::Emit(error) => error.fmt(f),
            Self::ArtifactInvariant => f.write_str("missing primary CSV compile entry"),
        }
    }
}
impl std::error::Error for PrimaryCsvEmitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Input(error) => Some(error),
            Self::Emit(error) => Some(error),
            Self::ArtifactInvariant => None,
        }
    }
}

pub fn emit_with_primary_csv_acquisition(
    document: &DocumentProgram,
    policy: &CsvInputPolicy,
) -> Result<ArtifactSet, PrimaryCsvEmitError> {
    codegen::validate_primary_csv_acquisition(document.program(), policy)
        .map_err(PrimaryCsvEmitError::Input)?;
    let mut files = crate::emit_with_primary_filename(document)
        .map_err(PrimaryCsvEmitError::Emit)?
        .into_files();
    let project = files
        .iter_mut()
        .find(|file| file.path.as_str() == "Ferrule.Generated.csproj")
        .ok_or(PrimaryCsvEmitError::ArtifactInvariant)?;
    let text = std::str::from_utf8(&project.contents)
        .map_err(|_| PrimaryCsvEmitError::ArtifactInvariant)?;
    let entry = "    <Compile Include=\"GeneratedMapping.cs\" />\n";
    if text.matches(entry).count() != 1 {
        return Err(PrimaryCsvEmitError::ArtifactInvariant);
    }
    project.contents = text
        .replacen(
            entry,
            &format!("{entry}    <Compile Include=\"GeneratedMapping.PrimaryCsv.cs\" />\n"),
            1,
        )
        .into_bytes();
    let SchemaKind::Group { children, .. } = &document.program().source.kind else {
        unreachable!("validated CSV source")
    };
    let mut code = String::from(
        "namespace Ferrule.Generated;\n\npublic static partial class GeneratedMapping\n{\n    private static readonly global::Ferrule.Runtime.FerruleCsvReadField[] PrimaryCsvFields = [\n",
    );
    for field in children {
        let SchemaKind::Scalar { ty } = field.kind else {
            unreachable!("validated scalar field")
        };
        let ty = match ty {
            ScalarType::String => "String",
            ScalarType::Int => "Int64",
            ScalarType::Float => "Double",
            ScalarType::Bool => "Bool",
        };
        code.push_str(&format!(
            "        new({}, global::Ferrule.Runtime.FerruleScalarType.{ty}),\n",
            literal::string(&field.name)
        ));
    }
    let character = |value: Option<char>| {
        value.map_or_else(
            || "null".to_string(),
            |value| format!("(char){}", u32::from(value)),
        )
    };
    code.push_str(&format!("    ];\n    private static readonly global::Ferrule.Runtime.FerruleCsvReadOptions PrimaryCsvOptions = new() {{ Delimiter = {}, Quote = {}, QuoteDisabled = {}, HasHeaders = {}, PreserveEmptyStrings = {} }};\n",character(policy.delimiter),character(policy.quote),policy.quote_disabled,policy.has_headers,policy.preserve_empty_strings));
    code.push_str("\n    /// <summary>Acquire rows only; file selection and loading policy belong to the host.</summary>\n    public static global::Ferrule.Runtime.FerruleCsvAcquisition AcquirePrimaryCsv(\n        global::Ferrule.Runtime.FerruleExecutionContext execution,\n        global::System.Func<string, global::System.Collections.Generic.IEnumerable<string>> select,\n        global::System.Func<string, global::Ferrule.Runtime.FerruleCsvSourceData> load,\n        global::Ferrule.Runtime.FerruleCsvByteDecodingPolicy decoding, int maximum)\n    {\n        global::System.ArgumentNullException.ThrowIfNull(execution);\n        return new(() => EvaluatePrimaryFilename(execution), select, load, PrimaryCsvFields, PrimaryCsvOptions, decoding, maximum);\n    }\n}\n");
    files.push(file("GeneratedMapping.PrimaryCsv.cs", code).map_err(PrimaryCsvEmitError::Emit)?);
    for (path, contents) in [
        (
            "Runtime/FerruleCsvInput.cs",
            include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleCsvInput.cs"),
        ),
        (
            "Runtime/FerruleCsvSource.cs",
            include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleCsvSource.cs"),
        ),
        (
            "Runtime/FerruleCsvAcquisition.cs",
            include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleCsvAcquisition.cs"),
        ),
    ] {
        files.push(file(path, contents.to_string()).map_err(PrimaryCsvEmitError::Emit)?);
    }
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(PrimaryCsvEmitError::Emit)
}
