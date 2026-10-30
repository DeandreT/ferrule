use codegen::{ArtifactSet, DocumentProgram, JsonProjectionBoundaryError};
use mapping::JsonSourceShape;

use crate::{EmitError, Options, rust_string};

#[derive(Debug)]
pub enum PrimaryJsonEmitError {
    Boundary(JsonProjectionBoundaryError),
    Emit(EmitError),
    Writer(codegen::MapForceX12WriterError),
    Visitor(codegen::DocumentVisitorEmitError<EmitError>),
    Parameter(codegen::JsonParameterBoundaryError),
}
impl std::fmt::Display for PrimaryJsonEmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Boundary(error) => error.fmt(f),
            Self::Emit(error) => error.fmt(f),
            Self::Writer(error) => error.fmt(f),
            Self::Visitor(error) => error.fmt(f),
            Self::Parameter(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for PrimaryJsonEmitError {}

pub fn emit_with_mapforce_x12_writer(
    document: &DocumentProgram,
    options: &Options,
    shape: &JsonSourceShape,
    target_options: &mapping::FormatOptions,
    minimums: &[mapping::MapForceEdiMinimum],
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let descriptor =
        codegen::prepare_mapforce_x12_writer(document.program(), target_options, minimums)
            .map_err(PrimaryJsonEmitError::Writer)?;
    emit_with_writer(document, options, shape, descriptor)
}

/// Emit grouped documents with incremental delivery and an individual X12 writer.
/// Filename clocks precede each body; writer clocks remain separate host calls.
pub fn emit_grouped_with_mapforce_x12_writer(
    document: &DocumentProgram,
    options: &Options,
    shape: &JsonSourceShape,
    target_options: &mapping::FormatOptions,
    minimums: &[mapping::MapForceEdiMinimum],
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let descriptor =
        codegen::prepare_mapforce_x12_document_writer(document, target_options, minimums)
            .map_err(PrimaryJsonEmitError::Writer)?;
    let projection = codegen::prepare_json_source_projection(document, shape)
        .map_err(PrimaryJsonEmitError::Boundary)?;
    let base = crate::document_visitor::emit_with_static_error_document_clocks(
        document.program(),
        options,
    )
    .map_err(PrimaryJsonEmitError::Visitor)?;
    let base =
        crate::primary_filename::append(base, document).map_err(PrimaryJsonEmitError::Emit)?;
    let projected = append_projection(base, projection, document.json_grouping_preparation())?;
    append_writer(projected, descriptor)
}

/// Add explicit parameter admission around a stateless physical grouped body.
/// The host visits prepared invocations and binds each key before acquisition.
pub fn emit_invoked_grouped_with_mapforce_x12_writer(
    document: &DocumentProgram,
    options: &Options,
    invocation: &mapping::JsonParameterInvocationPlan,
    shape: &JsonSourceShape,
    target_options: &mapping::FormatOptions,
    minimums: &[mapping::MapForceEdiMinimum],
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let descriptor = codegen::prepare_json_parameter_invocation(document, invocation)
        .map_err(PrimaryJsonEmitError::Parameter)?;
    let base =
        emit_grouped_with_mapforce_x12_writer(document, options, shape, target_options, minimums)?;
    append_parameter_invocation(base, descriptor, invocation)
}

pub fn emit_object_invoked_grouped_with_mapforce_x12_writer(
    document: &DocumentProgram,
    options: &Options,
    invocation: &mapping::JsonParameterInvocationPlan,
    binding: &codegen::JsonParameterObjectBinding,
    shape: &JsonSourceShape,
    target_options: &mapping::FormatOptions,
    minimums: &[mapping::MapForceEdiMinimum],
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let writer = codegen::prepare_mapforce_x12_parameter_document_writer(
        document,
        invocation,
        binding,
        target_options,
        minimums,
    )
    .map_err(PrimaryJsonEmitError::Writer)?;
    let base = emit_object_invoked_with_primary_json_projection(
        document, options, invocation, binding, shape,
    )?;
    append_writer(base, writer)
}

fn append_parameter_object(
    base: ArtifactSet,
    invocation: &mapping::JsonParameterInvocationPlan,
    binding: &codegen::JsonParameterObjectBinding,
    grouped: bool,
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let shape = serde_json::to_string(&invocation.projection)
        .map_err(JsonProjectionBoundaryError::Descriptor)
        .map_err(PrimaryJsonEmitError::Boundary)?;
    let preparation = if grouped {
        ".and_then(|source| source.with_grouping_preparation(primary_json_grouping()))"
    } else {
        ""
    };
    let source = format!(
        r#"
pub fn parameter_source_name() -> &'static str {{ {name} }}

pub fn project_parameter_source(
    invocation: codegen_runtime::JsonParameterInvocation<'_>,
    limits: codegen_runtime::JsonProjectionLimits,
) -> Result<Option<Instance>, codegen_runtime::JsonProjectionError> {{
    static SHAPE: std::sync::OnceLock<codegen_runtime::JsonSourceShape> = std::sync::OnceLock::new();
    let shape = SHAPE.get_or_init(|| codegen_runtime::parse_json_projection_shape({shape}).expect("generation-validated parameter projection"));
    codegen_runtime::project_json_source_value(shape, invocation.parameter_object(), limits)
}}

pub fn parameter_filename_members<'a>(invocation: codegen_runtime::JsonParameterInvocation<'a>) -> impl Iterator<Item = &'a str> {{
    invocation.parameter_object().members_named({member}).filter_map(|value| value.as_str())
}}

pub fn acquire_parameter_json<'a, E: 'a, I: Iterator<Item = Result<String, E>> + 'a>(
    invocation: codegen_runtime::JsonParameterInvocation<'a>,
    mut select: impl FnMut(&str) -> Result<I, E> + 'a,
    load: impl FnMut(&str) -> Result<codegen_runtime::JsonSourceDocument, E> + 'a,
    limits: codegen_runtime::JsonProjectionLimits,
    maximum: usize,
) -> Result<codegen_runtime::JsonSourceAcquisition<'a, std::convert::Infallible, E>, codegen_runtime::JsonAcquisitionError<std::convert::Infallible, E>> {{
    let requests = parameter_filename_members(invocation).flat_map(move |name| {{
        let values: Box<dyn Iterator<Item = Result<String, E>> + 'a> = match select(name) {{
            Ok(values) => Box::new(values),
            Err(error) => Box::new(std::iter::once(Err(error))),
        }};
        values
    }});
    codegen_runtime::JsonSourceAcquisition::from_requests(requests, load, primary_json_projection(), limits, maximum){preparation}
}}
"#,
        name = rust_string(&binding.source_name),
        shape = rust_string(&shape),
        member = rust_string(&binding.filename_member),
    );
    let mut files = base.into_files();
    files
        .iter_mut()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .expect("Rust library artifact")
        .contents
        .extend_from_slice(source.as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(PrimaryJsonEmitError::Emit)
}

pub fn emit_object_invoked_with_primary_json_projection(
    document: &DocumentProgram,
    options: &Options,
    invocation: &mapping::JsonParameterInvocationPlan,
    binding: &codegen::JsonParameterObjectBinding,
    shape: &JsonSourceShape,
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let descriptor =
        codegen::prepare_json_parameter_object_invocation(document, invocation, binding)
            .map_err(PrimaryJsonEmitError::Parameter)?;
    let projection = codegen::prepare_json_source_projection(document, shape)
        .map_err(PrimaryJsonEmitError::Boundary)?;
    let base = crate::document_visitor::emit_with_static_error_document_clocks(
        document.program(),
        options,
    )
    .map_err(PrimaryJsonEmitError::Visitor)?;
    let base =
        crate::primary_filename::append(base, document).map_err(PrimaryJsonEmitError::Emit)?;
    let base = append_projection(base, projection, document.json_grouping_preparation())?;
    let base = append_parameter_invocation(base, descriptor, invocation)?;
    append_parameter_object(
        base,
        invocation,
        binding,
        document.json_grouping_preparation().is_some(),
    )
}

pub fn emit_invoked_with_primary_json_projection(
    document: &DocumentProgram,
    options: &Options,
    invocation: &mapping::JsonParameterInvocationPlan,
    shape: &JsonSourceShape,
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let descriptor = codegen::prepare_json_parameter_invocation(document, invocation)
        .map_err(PrimaryJsonEmitError::Parameter)?;
    let projection = codegen::prepare_json_source_projection(document, shape)
        .map_err(PrimaryJsonEmitError::Boundary)?;
    let base = crate::document_visitor::emit_with_static_error_document_clocks(
        document.program(),
        options,
    )
    .map_err(PrimaryJsonEmitError::Visitor)?;
    let base =
        crate::primary_filename::append(base, document).map_err(PrimaryJsonEmitError::Emit)?;
    let base = append_projection(base, projection, document.json_grouping_preparation())?;
    append_parameter_invocation(base, descriptor, invocation)
}

fn append_parameter_invocation(
    base: ArtifactSet,
    descriptor: String,
    invocation: &mapping::JsonParameterInvocationPlan,
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let mut files = base.into_files();
    let source = format!(
        "\npub fn prepare_parameter_invocations(input: &codegen_runtime::Value, json_limits: codegen_runtime::JsonSourceLimits, limits: codegen_runtime::JsonParameterInvocationLimits) -> Result<codegen_runtime::PreparedJsonParameterInvocations, codegen_runtime::JsonParameterInvocationError> {{\n    static PLAN: std::sync::OnceLock<codegen_runtime::JsonParameterInvocationPlan> = std::sync::OnceLock::new();\n    let plan = PLAN.get_or_init(|| codegen_runtime::parse_json_parameter_invocation_plan({}).expect(\"generation-validated parameter plan\"));\n    codegen_runtime::PreparedJsonParameterInvocations::prepare(plan, input, json_limits, limits)\n}}\npub fn parameter_input_name() -> &'static str {{ {} }}\n",
        rust_string(&descriptor),
        rust_string(&invocation.parameter),
    );
    files
        .iter_mut()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .expect("ordinary Rust artifact")
        .contents
        .extend_from_slice(source.as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(PrimaryJsonEmitError::Emit)
}

fn emit_with_writer(
    document: &DocumentProgram,
    options: &Options,
    shape: &JsonSourceShape,
    descriptor: String,
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let projected = emit_with_primary_json_projection(document, options, shape)?;
    append_writer(projected, descriptor)
}

fn append_writer(
    artifacts: ArtifactSet,
    descriptor: String,
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let mut files = artifacts.into_files();
    let mut source = format!(
        "\npub fn serialize_mapforce_x12_document(value: &Instance, execution: &ExecutionContext<'_>) -> Result<Vec<u8>, codegen_runtime::X12BoundaryError> {{\n    codegen_runtime::serialize_mapforce_x12_document({0}, value, execution)\n}}\npub fn write_mapforce_x12_document(value: &Instance, execution: &ExecutionContext<'_>, output: &mut dyn std::io::Write) -> Result<(), codegen_runtime::MapForceX12WriteError> {{\n    codegen_runtime::write_mapforce_x12_document({0}, value, execution, output)\n}}\n",
        rust_string(&descriptor)
    );
    source.push_str(&format!("\npub fn serialize_mapforce_x12_document_with_clock(value: &Instance, execution: &ExecutionContext<'_>, clock: &mut dyn FnMut(codegen_runtime::MapForceX12ClockField) -> Result<String, String>) -> Result<Vec<u8>, codegen_runtime::X12BoundaryError> {{\n    codegen_runtime::serialize_mapforce_x12_document_with_clock({0}, value, execution, clock)\n}}\npub fn write_mapforce_x12_document_with_clock(value: &Instance, execution: &ExecutionContext<'_>, clock: &mut dyn FnMut(codegen_runtime::MapForceX12ClockField) -> Result<String, String>, output: &mut dyn std::io::Write) -> Result<(), codegen_runtime::MapForceX12WriteError> {{\n    codegen_runtime::write_mapforce_x12_document_with_clock({0}, value, execution, clock, output)\n}}\n",rust_string(&descriptor)));
    files
        .iter_mut()
        .find(|f| f.path.as_str() == "src/lib.rs")
        .expect("ordinary Rust artifact")
        .contents
        .extend_from_slice(source.as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(PrimaryJsonEmitError::Emit)
}

/// Add lazy primary filename acquisition and typed ordered JSON projection.
/// The host owns transport, parsing and cleanup; this does not publish output.
pub fn emit_with_primary_json_projection(
    document: &DocumentProgram,
    options: &Options,
    shape: &JsonSourceShape,
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let descriptor = codegen::prepare_json_source_projection(document, shape)
        .map_err(PrimaryJsonEmitError::Boundary)?;
    let base =
        crate::emit_with_primary_filename(document, options).map_err(PrimaryJsonEmitError::Emit)?;
    append_projection(base, descriptor, document.json_grouping_preparation())
}

fn append_projection(
    base: ArtifactSet,
    descriptor: String,
    preparation: Option<&mapping::JsonGroupingPreparation>,
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let mut files = base.into_files();
    let preparation_call = if preparation.is_some() {
        ".and_then(|source| source.with_grouping_preparation(primary_json_grouping()))"
    } else {
        ""
    };
    let mut code = format!(
        "\nfn primary_json_projection() -> &'static codegen_runtime::JsonSourceShape {{\n    static SHAPE: std::sync::OnceLock<codegen_runtime::JsonSourceShape> = std::sync::OnceLock::new();\n    SHAPE.get_or_init(|| codegen_runtime::parse_json_projection_shape({}).expect(\"generation-validated JSON projection\"))\n}}\n\n/// Lazily acquire one typed document at a time. Host callbacks own I/O and parsing.\npub fn acquire_primary_json<'a, E: 'a, I: Iterator<Item = Result<String, E>> + 'a>(\n    execution: &'a ExecutionContext<'a>,\n    select: impl FnOnce(&str) -> Result<I, E> + 'a,\n    load: impl FnMut(&str) -> Result<codegen_runtime::JsonSourceDocument, E> + 'a,\n    limits: codegen_runtime::JsonProjectionLimits, maximum: usize,\n) -> Result<codegen_runtime::JsonSourceAcquisition<'a, codegen_runtime::RuntimeError, E>, codegen_runtime::JsonAcquisitionError<codegen_runtime::RuntimeError, E>> {{\n    codegen_runtime::JsonSourceAcquisition::new(move || evaluate_primary_filename(execution), select, load, primary_json_projection(), limits, maximum)\n}}\n",
        rust_string(&descriptor)
    );
    if let Some(preparation) = preparation {
        let encoded = serde_json::to_string(preparation)
            .map_err(JsonProjectionBoundaryError::Descriptor)
            .map_err(PrimaryJsonEmitError::Boundary)?;
        code = code.replacen(
            "primary_json_projection(), limits, maximum)\n",
            &format!("primary_json_projection(), limits, maximum){preparation_call}\n"),
            1,
        );
        code.push_str(&format!("\nfn primary_json_grouping() -> &'static codegen_runtime::JsonGroupingPreparation {{\n    static PLAN: std::sync::OnceLock<codegen_runtime::JsonGroupingPreparation> = std::sync::OnceLock::new();\n    PLAN.get_or_init(|| codegen_runtime::parse_json_grouping_preparation({}).expect(\"generation-validated grouping preparation\"))\n}}\npub fn prepare_primary_json_document(document: Instance, limits: codegen_runtime::JsonProjectionLimits) -> Result<Instance, codegen_runtime::JsonGroupingError> {{\n    codegen_runtime::prepare_json_grouping_document(primary_json_grouping(), primary_json_projection(), document, limits)\n}}\n",rust_string(&encoded)));
    }
    code.push_str("\n/// Explicit wire-compatible byte decoding. Lone UTF-16 escapes become U+FFFD;\n/// separately qualify string operations and encoding. Strict parsing is unchanged.\npub fn parse_primary_json_source(identity: &str, bytes: &[u8]) -> Result<codegen_runtime::JsonSourceDocument, codegen_runtime::NativeJsonSourceError> {\n    codegen_runtime::parse_native_json_source(identity, bytes, primary_json_projection(), codegen_runtime::JsonSourceLimits::default())\n}\n");
    code.push_str("\n/// Sample the body clock only at reached current-dateTime nodes.\n/// Source acquisition and filename clocks remain host-owned separate calls.\npub fn execute_primary_json_with_body_clock<'a>(source: &'a Instance, execution: &ExecutionContext<'a>, clock: &'a dyn codegen_runtime::DocumentBodyClock) -> Result<Instance, RuntimeError> {\n    let context = ScopeContext::with_execution_context(source, execution).with_document_body_clock(Some(clock));\n    Ok(execute_outputs_from_context(&context)?.primary)\n}\n");
    files
        .iter_mut()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .expect("ordinary Rust artifact")
        .contents
        .extend_from_slice(code.as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(PrimaryJsonEmitError::Emit)
}
