use std::fmt;

use codegen::{ArtifactSet, Json5BoundaryPolicyError, Program};

use crate::{EmitError, file, literal, runtime};

/// Errors from explicitly selected JSON5 emission. Ordinary emission is unchanged.
#[derive(Debug)]
pub enum Json5EmitError {
    Policy(Json5BoundaryPolicyError),
    Ordinary(EmitError),
    ArtifactInvariant { path: &'static str },
}

impl fmt::Display for Json5EmitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Policy(error) => write!(formatter, "JSON5 boundary: {error}"),
            Self::Ordinary(error) => write!(formatter, "JSON5 ordinary emission: {error}"),
            Self::ArtifactInvariant { path } => write!(
                formatter,
                "JSON5 emission requires the ordinary artifact {path}"
            ),
        }
    }
}
impl std::error::Error for Json5EmitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Policy(error) => Some(error),
            Self::Ordinary(error) => Some(error),
            Self::ArtifactInvariant { .. } => None,
        }
    }
}
impl From<EmitError> for Json5EmitError {
    fn from(error: EmitError) -> Self {
        Self::Ordinary(error)
    }
}

pub(crate) fn emit(program: &Program) -> Result<ArtifactSet, Json5EmitError> {
    // Both descriptors and the complete shared program profile precede artifacts.
    let profile = codegen::prepare_json5_boundary(program).map_err(Json5EmitError::Policy)?;
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
            "    <Compile Include=\"GeneratedMapping.cs\" />\n    <Compile Include=\"GeneratedMapping.Json5.cs\" />\n",
        ),
    ] {
        let generated = files
            .iter_mut()
            .find(|generated| generated.path.as_str() == path)
            .ok_or(Json5EmitError::ArtifactInvariant { path })?;
        let source = std::str::from_utf8(&generated.contents)
            .map_err(|_| Json5EmitError::ArtifactInvariant { path })?;
        if source.matches(before).count() != 1 {
            return Err(Json5EmitError::ArtifactInvariant { path });
        }
        generated.contents = source.replacen(before, after, 1).into_bytes();
    }
    files.push(file(
        "GeneratedMapping.Json5.cs",
        render(&profile.source_descriptor, &profile.target_descriptor),
    )?);
    for (path, source) in runtime::JSON5_SOURCES {
        files.push(file(path, source)?);
    }
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(Json5EmitError::from)
}

fn render(source: &str, target: &str) -> String {
    let mut output = String::from(
        "namespace Ferrule.Generated;\n\npublic static partial class GeneratedMapping\n{\n    private const string Json5SourceDescriptor = ",
    );
    output.push_str(&literal::string(source));
    output.push_str(";\n    private const string Json5TargetDescriptor = ");
    output.push_str(&literal::string(target));
    output.push_str(";\n");
    output.push_str(ADAPTERS);
    output
}

const ADAPTERS: &str = r#"
    public static string ExecuteJson5(string source)
    {
        return global::Ferrule.Runtime.FerruleJson5.Execute(
            Json5SourceDescriptor, Json5TargetDescriptor, source, Execute);
    }

    public static string ExecuteJson5(
        string source, global::Ferrule.Runtime.FerruleExecutionContext executionContext)
    {
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(executionContext);
        return global::Ferrule.Runtime.FerruleJson5.Execute(
            Json5SourceDescriptor, Json5TargetDescriptor, source,
            input => Execute(input, executionContext));
    }

    public static byte[] ExecuteJson5Bytes(byte[] source)
    {
        return global::Ferrule.Runtime.FerruleJson5.ExecuteBytes(
            Json5SourceDescriptor, Json5TargetDescriptor, source, Execute);
    }

    public static byte[] ExecuteJson5Bytes(
        byte[] source, global::Ferrule.Runtime.FerruleExecutionContext executionContext)
    {
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(executionContext);
        return global::Ferrule.Runtime.FerruleJson5.ExecuteBytes(
            Json5SourceDescriptor, Json5TargetDescriptor, source,
            input => Execute(input, executionContext));
    }
}
"#;
