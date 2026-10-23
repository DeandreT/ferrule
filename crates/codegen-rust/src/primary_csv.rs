use std::fmt;

use codegen::{ArtifactSet, CsvInputBoundaryError, CsvInputPolicy, DocumentProgram};
use ir::SchemaKind;

use crate::{EmitError, Options, rust_string};

#[derive(Debug)]
pub enum PrimaryCsvEmitError {
    Input(CsvInputBoundaryError),
    Emit(EmitError),
}
impl fmt::Display for PrimaryCsvEmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(error) => error.fmt(f),
            Self::Emit(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for PrimaryCsvEmitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Input(error) => error,
            Self::Emit(error) => error,
        })
    }
}

/// Add lazy CSV acquisition, not document mapping, to the filename companion.
pub fn emit_with_primary_csv_acquisition(
    document: &DocumentProgram,
    options: &Options,
    policy: &CsvInputPolicy,
) -> Result<ArtifactSet, PrimaryCsvEmitError> {
    codegen::validate_primary_csv_acquisition(document.program(), policy)
        .map_err(PrimaryCsvEmitError::Input)?;
    let artifacts =
        crate::emit_with_primary_filename(document, options).map_err(PrimaryCsvEmitError::Emit)?;
    append(artifacts, document, policy)
}

pub(crate) fn append(
    artifacts: ArtifactSet,
    document: &DocumentProgram,
    policy: &CsvInputPolicy,
) -> Result<ArtifactSet, PrimaryCsvEmitError> {
    append_schema(artifacts, &document.program().source, policy, false)
}

pub(crate) fn append_file_set(
    artifacts: ArtifactSet,
    row_schema: &ir::SchemaNode,
    policy: &CsvInputPolicy,
) -> Result<ArtifactSet, PrimaryCsvEmitError> {
    append_schema(artifacts, row_schema, policy, true)
}

fn append_schema(
    artifacts: ArtifactSet,
    schema: &ir::SchemaNode,
    policy: &CsvInputPolicy,
    file_set: bool,
) -> Result<ArtifactSet, PrimaryCsvEmitError> {
    let mut files = artifacts.into_files();
    let SchemaKind::Group { children, .. } = &schema.kind else {
        unreachable!("validated row schema")
    };
    let mut code = format!(
        "\nfn primary_csv_schema() -> &'static codegen_runtime::SchemaNode {{\n    static SCHEMA: std::sync::OnceLock<codegen_runtime::SchemaNode> = std::sync::OnceLock::new();\n    SCHEMA.get_or_init(|| codegen_runtime::SchemaNode::group({}, vec![\n",
        rust_string(&schema.name)
    );
    for field in children {
        let SchemaKind::Scalar { ty } = field.kind else {
            unreachable!("validated scalar field")
        };
        code.push_str(&format!("        codegen_runtime::SchemaNode::scalar({}, codegen_runtime::ScalarType::{ty:?}),\n",rust_string(&field.name)));
    }
    code.push_str("    ]))\n}\n");
    code.push_str(&format!("\nfn primary_csv_options() -> codegen_runtime::CsvReadOptions {{\n    codegen_runtime::CsvReadOptions {{ delimiter: {:?}, quote: {:?}, quote_disabled: {}, has_headers: {}, preserve_empty_strings: {}, repair_dependency: None }}\n}}\n",policy.delimiter,policy.quote,policy.quote_disabled,policy.has_headers,policy.preserve_empty_strings));
    if file_set {
        code.push_str("\n/// Acquire all physical files, including empty ones, before mapping.\npub fn acquire_primary_csv_file_set<E, I: Iterator<Item = Result<String, E>>>(\n    execution: &ExecutionContext<'_>, select: impl FnOnce(&str) -> Result<I, E>,\n    load: impl FnMut(&str) -> Result<codegen_runtime::CsvSourceData, E>,\n    decoding: codegen_runtime::CsvByteDecodingPolicy, maximum: usize,\n) -> Result<Instance, codegen_runtime::CsvAcquisitionError<codegen_runtime::RuntimeError, E>> {\n    codegen_runtime::acquire_csv_file_set(|| evaluate_primary_filename(execution), select, load,\n        primary_csv_schema(), &primary_csv_options(), decoding, maximum)\n}\n");
    } else {
        code.push_str("\n/// Acquire rows only. The host decides file selection and loading policy.\npub fn acquire_primary_csv<'a, E: 'a, I: Iterator<Item = Result<String, E>> + 'a>(\n    execution: &'a ExecutionContext<'a>,\n    select: impl FnOnce(&str) -> Result<I, E> + 'a,\n    load: impl FnMut(&str) -> Result<codegen_runtime::CsvSourceData, E> + 'a,\n    decoding: codegen_runtime::CsvByteDecodingPolicy,\n    maximum: usize,\n) -> Result<codegen_runtime::CsvSourceAcquisition<'a, codegen_runtime::RuntimeError, E>, codegen_runtime::CsvAcquisitionError<codegen_runtime::RuntimeError, E>> {\n    codegen_runtime::CsvSourceAcquisition::new(\n        move || evaluate_primary_filename(execution), select, load,\n        primary_csv_schema(), primary_csv_options(), decoding, maximum,\n    )\n}\n");
    }
    files
        .iter_mut()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .expect("ordinary Rust mapping artifact")
        .contents
        .extend_from_slice(code.as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(PrimaryCsvEmitError::Emit)
}
