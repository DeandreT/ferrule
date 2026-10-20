use std::fmt;

use codegen::{ArtifactSet, CsvDocumentBoundaryError, CsvDocumentBoundaryPolicy, Program};
use ir::SchemaKind;

use crate::{EmitError, Options, rust_string};

#[derive(Debug)]
pub enum CsvDocumentEmitError {
    Boundary(Box<CsvDocumentBoundaryError>),
    Emit(EmitError),
}

impl fmt::Display for CsvDocumentEmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Boundary(e) => e.fmt(f),
            Self::Emit(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for CsvDocumentEmitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Boundary(e) => e.as_ref(),
            Self::Emit(e) => e,
        })
    }
}

/// Add complete CSV text/byte entry points using the ordinary generated mapper.
pub fn emit_with_csv_document_adapters(
    program: &Program,
    options: &Options,
    policy: &CsvDocumentBoundaryPolicy,
) -> Result<ArtifactSet, CsvDocumentEmitError> {
    codegen::validate_csv_document_boundary(program, policy)
        .map_err(|e| CsvDocumentEmitError::Boundary(Box::new(e)))?;
    let mut files = crate::emit_with_csv_output(program, options, &policy.target)
        .map_err(CsvDocumentEmitError::Emit)?
        .into_files();
    let SchemaKind::Group { children, .. } = &program.source.kind else {
        unreachable!("admitted CSV source")
    };
    let mut source = String::from("\nfn csv_input_schema() -> codegen_runtime::SchemaNode {\n");
    source.push_str(&format!(
        "    codegen_runtime::SchemaNode::group({}, vec![\n",
        rust_string(&program.source.name)
    ));
    for field in children {
        let SchemaKind::Scalar { ty } = field.kind else {
            unreachable!("admitted CSV field")
        };
        source.push_str(&format!(
            "        codegen_runtime::SchemaNode::scalar({}, ScalarType::{ty:?}),\n",
            rust_string(&field.name)
        ));
    }
    let p = &policy.source;
    source.push_str(&format!("    ])\n}}\n\nfn csv_input_options() -> codegen_runtime::CsvReadOptions {{\n    codegen_runtime::CsvReadOptions {{\n        delimiter: {:?},\n        quote: {:?},\n        quote_disabled: {},\n        has_headers: {},\n        preserve_empty_strings: {},\n        repair_dependency: None,\n    }}\n}}\n", p.delimiter, p.quote, p.quote_disabled, p.has_headers, p.preserve_empty_strings));
    for bytes in [false, true] {
        let suffix = if bytes { "_bytes" } else { "" };
        let input = if bytes { "&[u8]" } else { "&str" };
        let output = if bytes { "Vec<u8>" } else { "String" };
        source.push_str(&format!("\npub fn parse_csv{suffix}(source: {input}) -> Result<Instance, codegen_runtime::CsvReadError> {{\n    codegen_runtime::parse_csv_rows{suffix}(source, &csv_input_schema(), &csv_input_options()).map(Instance::Repeated)\n}}\n"));
        for contextual in [false, true] {
            let context = if contextual { "_with_context" } else { "" };
            let arg = if contextual {
                ", execution: &ExecutionContext<'_>"
            } else {
                ""
            };
            let pass = if contextual { ", execution" } else { "" };
            source.push_str(&format!("\n/// Parse the complete input, map, then write one complete CSV document.\npub fn execute_csv_document{suffix}{context}(source: {input}{arg}) -> Result<{output}, codegen_runtime::CsvDocumentError> {{\n    let input = parse_csv{suffix}(source)?;\n    execute_csv{suffix}{context}(&input{pass}).map_err(Into::into)\n}}\n"));
        }
    }
    source.push_str("\npub fn parse_csv_bytes_with_decoding_policy(source: &[u8], policy: codegen_runtime::CsvByteDecodingPolicy) -> Result<Instance, codegen_runtime::CsvReadError> {\n    codegen_runtime::parse_csv_rows_bytes_with_decoding_policy(source, &csv_input_schema(), &csv_input_options(), policy).map(Instance::Repeated)\n}\n");
    for contextual in [false, true] {
        let suffix = if contextual { "_with_context" } else { "" };
        let argument = if contextual {
            ", execution: &ExecutionContext<'_>"
        } else {
            ""
        };
        let pass = if contextual { ", execution" } else { "" };
        source.push_str(&format!("\npub fn execute_csv_document_bytes_with_decoding_policy{suffix}(source: &[u8], policy: codegen_runtime::CsvByteDecodingPolicy{argument}) -> Result<Vec<u8>, codegen_runtime::CsvDocumentError> {{\n    let input = parse_csv_bytes_with_decoding_policy(source, policy)?;\n    execute_csv_bytes{suffix}(&input{pass}).map_err(Into::into)\n}}\n"));
    }
    files
        .iter_mut()
        .find(|f| f.path.as_str() == "src/lib.rs")
        .expect("ordinary Rust artifact")
        .contents
        .extend_from_slice(source.as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(CsvDocumentEmitError::Emit)
}
