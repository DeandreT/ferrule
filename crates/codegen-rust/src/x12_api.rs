use std::error::Error;
use std::fmt;

use codegen::{ArtifactSet, ArtifactSetError, Program, X12BoundaryPolicy, X12BoundaryPolicyError};

use crate::{EmitError, Options, emit, rust_string};

#[derive(Debug)]
pub enum X12EmitError {
    Policy(X12BoundaryPolicyError),
    UnsupportedNativeSetting {
        side: &'static str,
        setting: &'static str,
    },
    Ordinary(EmitError),
    ArtifactSet(ArtifactSetError),
    MissingLibrary,
}

impl fmt::Display for X12EmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Policy(error) => error.fmt(f),
            Self::UnsupportedNativeSetting { side, setting } => {
                write!(f, "generated Rust X12 {side} does not support {setting}")
            }
            Self::Ordinary(error) => error.fmt(f),
            Self::ArtifactSet(error) => error.fmt(f),
            Self::MissingLibrary => f.write_str("ordinary emitter did not return src/lib.rs"),
        }
    }
}
impl Error for X12EmitError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Policy(error) => Some(error),
            Self::Ordinary(error) => Some(error),
            Self::ArtifactSet(error) => Some(error),
            _ => None,
        }
    }
}

/// Emit explicit primary JSON/X12 text and UTF-8 document companions.
/// Shared admission preserves the complete selected schema and physical policy.
/// Named boundaries, document iteration and XML adapters remain strict refusals.
pub fn emit_with_x12(
    program: &Program,
    options: &Options,
    policy: &X12BoundaryPolicy,
) -> Result<ArtifactSet, X12EmitError> {
    let profile = codegen::prepare_x12_boundary(program, policy).map_err(X12EmitError::Policy)?;
    for (side, selected) in [("source", &policy.source), ("target", &policy.target)] {
        if selected
            .as_ref()
            .and_then(|options| options.separators)
            .is_some_and(|syntax| syntax.segment == '\n')
        {
            return Err(X12EmitError::UnsupportedNativeSetting {
                side,
                setting: "LF segment separators in the native codec",
            });
        }
    }
    let mut files = emit(program, options)
        .map_err(X12EmitError::Ordinary)?
        .into_files();
    let library = files
        .iter_mut()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .ok_or(X12EmitError::MissingLibrary)?;
    let mut source = format!(
        "\nconst X12_SOURCE_DESCRIPTOR: &str = {};\nconst X12_TARGET_DESCRIPTOR: &str = {};\n",
        rust_string(&profile.source_descriptor),
        rust_string(&profile.target_descriptor)
    );
    if profile.source_x12 {
        source.push_str("\npub fn parse_x12(source: &str) -> Result<Instance, codegen_runtime::X12BoundaryError> { codegen_runtime::parse_x12(X12_SOURCE_DESCRIPTOR, source) }\npub fn parse_x12_bytes(source: &[u8]) -> Result<Instance, codegen_runtime::X12BoundaryError> { codegen_runtime::parse_x12_bytes(X12_SOURCE_DESCRIPTOR, source) }\n");
    }
    if profile.target_x12 {
        source.push_str("\npub fn serialize_x12(target: &Instance) -> Result<String, codegen_runtime::X12BoundaryError> { codegen_runtime::serialize_x12(X12_TARGET_DESCRIPTOR, target, None) }\npub fn serialize_x12_with_context(target: &Instance, execution: &ExecutionContext<'_>) -> Result<String, codegen_runtime::X12BoundaryError> { codegen_runtime::serialize_x12(X12_TARGET_DESCRIPTOR, target, Some(execution)) }\npub fn serialize_x12_bytes(target: &Instance) -> Result<Vec<u8>, codegen_runtime::X12BoundaryError> { codegen_runtime::serialize_x12_bytes(X12_TARGET_DESCRIPTOR, target, None) }\npub fn serialize_x12_bytes_with_context(target: &Instance, execution: &ExecutionContext<'_>) -> Result<Vec<u8>, codegen_runtime::X12BoundaryError> { codegen_runtime::serialize_x12_bytes(X12_TARGET_DESCRIPTOR, target, Some(execution)) }\n");
    }
    let direction = match (profile.source_x12, profile.target_x12) {
        (true, true) => "x12_to_x12",
        (true, false) => "x12_to_json",
        _ => "json_to_x12",
    };
    for bytes in [false, true] {
        for context in [false, true] {
            let name = format!(
                "execute_{direction}{}{}",
                if bytes { "_bytes" } else { "" },
                if context { "_with_context" } else { "" }
            );
            let arg = if context {
                ", execution: &ExecutionContext<'_>"
            } else {
                ""
            };
            let suffix = if bytes { "_bytes" } else { "" };
            let parse = if profile.source_x12 {
                format!("codegen_runtime::parse_x12{suffix}(X12_SOURCE_DESCRIPTOR, source)?")
            } else {
                format!(
                    "codegen_runtime::parse_json{suffix}(X12_SOURCE_DESCRIPTOR, source).map_err(codegen_runtime::X12BoundaryError::Json)?"
                )
            };
            let execute = if context {
                "execute_with_context(&input, execution)"
            } else {
                "execute(&input)"
            };
            let serialize = if profile.target_x12 {
                format!(
                    "codegen_runtime::serialize_x12{suffix}(X12_TARGET_DESCRIPTOR, &output, {})",
                    if context { "Some(execution)" } else { "None" }
                )
            } else {
                format!(
                    "codegen_runtime::serialize_json{suffix}(X12_TARGET_DESCRIPTOR, &output).map_err(codegen_runtime::X12BoundaryError::Json)"
                )
            };
            source.push_str(&format!("\npub fn {name}(source: {}{arg}) -> Result<{}, codegen_runtime::X12BoundaryError> {{\n    let input = {parse};\n    let output = {execute}.map_err(codegen_runtime::X12BoundaryError::Mapping)?;\n    {serialize}\n}}\n", if bytes { "&[u8]" } else { "&str" }, if bytes { "Vec<u8>" } else { "String" }));
        }
    }
    library.contents.extend_from_slice(source.as_bytes());
    ArtifactSet::new(files).map_err(X12EmitError::ArtifactSet)
}
