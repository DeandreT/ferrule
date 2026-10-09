use std::fmt;

use codegen::{ArtifactSet, Program, X12BoundaryPolicy, X12BoundaryPolicyError};

use crate::{EmitError, file, literal};

/// Failures from explicitly selected generated raw X12 adapters.
#[derive(Debug)]
pub enum X12EmitError {
    Policy(X12BoundaryPolicyError),
    Ordinary(EmitError),
    ArtifactInvariant { path: &'static str },
}

impl fmt::Display for X12EmitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Policy(error) => write!(formatter, "X12 boundary: {error}"),
            Self::Ordinary(error) => write!(formatter, "X12 ordinary emission: {error}"),
            Self::ArtifactInvariant { path } => {
                write!(
                    formatter,
                    "X12 emission requires the ordinary artifact {path}"
                )
            }
        }
    }
}

impl std::error::Error for X12EmitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Policy(error) => Some(error),
            Self::Ordinary(error) => Some(error),
            Self::ArtifactInvariant { .. } => None,
        }
    }
}

impl From<EmitError> for X12EmitError {
    fn from(error: EmitError) -> Self {
        Self::Ordinary(error)
    }
}

pub(crate) fn emit(
    program: &Program,
    policy: &X12BoundaryPolicy,
) -> Result<ArtifactSet, X12EmitError> {
    let profile = codegen::prepare_x12_boundary(program, policy).map_err(X12EmitError::Policy)?;
    // Validate the complete optional boundary before ordinary artifacts exist.
    let mut files = crate::emit(program)?.into_files();
    for (path, before, after) in [
        (
            "GeneratedMapping.cs",
            "public static class GeneratedMapping\n{",
            "public static partial class GeneratedMapping\n{",
        ),
        (
            "Ferrule.Generated.csproj",
            "    <Compile Include=\"GeneratedMapping.cs\" />\n",
            "    <Compile Include=\"GeneratedMapping.cs\" />\n    <Compile Include=\"GeneratedMapping.X12.cs\" />\n",
        ),
    ] {
        let generated = files
            .iter_mut()
            .find(|generated| generated.path.as_str() == path)
            .ok_or(X12EmitError::ArtifactInvariant { path })?;
        let source = std::str::from_utf8(&generated.contents)
            .map_err(|_| X12EmitError::ArtifactInvariant { path })?;
        if source.matches(before).count() != 1 {
            return Err(X12EmitError::ArtifactInvariant { path });
        }
        generated.contents = source.replacen(before, after, 1).into_bytes();
    }
    files.push(file(
        "GeneratedMapping.X12.cs",
        render(
            &profile.source_descriptor,
            &profile.target_descriptor,
            profile.source_x12,
            profile.target_x12,
        ),
    )?);
    for (path, source) in X12_SOURCES {
        files.push(file(path, source)?);
    }
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(X12EmitError::from)
}

pub(crate) const X12_SOURCES: [(&str, &str); 3] = [
    (
        "Runtime/X12/FerruleX12.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/X12/FerruleX12.cs"),
    ),
    (
        "Runtime/X12/FerruleX12.Schema.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/X12/FerruleX12.Schema.cs"),
    ),
    (
        "Runtime/X12/FerruleX12Exception.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/X12/FerruleX12Exception.cs"),
    ),
];

fn render(source: &str, target: &str, source_x12: bool, target_x12: bool) -> String {
    let mut output = String::from(
        "namespace Ferrule.Generated;\n\npublic static partial class GeneratedMapping\n{\n    private const string X12SourceDescriptor = ",
    );
    output.push_str(&literal::string(source));
    output.push_str(";\n    private const string X12TargetDescriptor = ");
    output.push_str(&literal::string(target));
    output.push_str(";\n");
    if source_x12 {
        output.push_str(PARSE);
    }
    if target_x12 {
        output.push_str(SERIALIZE);
    }
    let (method, parse, serialize) = match (source_x12, target_x12) {
        (true, false) => ("ExecuteX12ToJson", "FerruleX12", "FerruleJson"),
        (false, true) => ("ExecuteJsonToX12", "FerruleJson", "FerruleX12"),
        (true, true) => ("ExecuteX12ToX12", "FerruleX12", "FerruleX12"),
        (false, false) => unreachable!("shared X12 policy requires at least one X12 boundary"),
    };
    for (byte, contextual) in [(false, false), (false, true), (true, false), (true, true)] {
        let ty = if byte { "byte[]" } else { "string" };
        let suffix = if byte { "Bytes" } else { "" };
        output.push_str(&format!(
            "\n    public static {ty} {method}{suffix}(\n        {ty} source"
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
        output.push_str(&format!(
            "        var input = global::Ferrule.Runtime.{parse}.ParseEmbedded{suffix}(X12SourceDescriptor, source);\n"
        ));
        output.push_str(if contextual {
            "        var target = Execute(input, executionContext);\n"
        } else {
            "        var target = Execute(input);\n"
        });
        output.push_str(&format!(
            "        return global::Ferrule.Runtime.{serialize}.SerializeEmbedded{suffix}(X12TargetDescriptor, target);\n    }}\n"
        ));
    }
    output.push_str("}\n");
    output
}

const PARSE: &str = r#"
    public static global::Ferrule.Runtime.FerruleInstance ParseX12(string source)
    {
        return global::Ferrule.Runtime.FerruleX12.ParseEmbedded(X12SourceDescriptor, source);
    }

    public static global::Ferrule.Runtime.FerruleInstance ParseX12Bytes(byte[] source)
    {
        return global::Ferrule.Runtime.FerruleX12.ParseEmbeddedBytes(X12SourceDescriptor, source);
    }
"#;

const SERIALIZE: &str = r#"
    public static string SerializeX12(global::Ferrule.Runtime.FerruleInstance target)
    {
        return global::Ferrule.Runtime.FerruleX12.SerializeEmbedded(X12TargetDescriptor, target);
    }

    public static byte[] SerializeX12Bytes(global::Ferrule.Runtime.FerruleInstance target)
    {
        return global::Ferrule.Runtime.FerruleX12.SerializeEmbeddedBytes(X12TargetDescriptor, target);
    }
"#;
