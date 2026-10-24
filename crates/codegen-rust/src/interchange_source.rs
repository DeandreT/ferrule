use crate::{EmitError, Options};
use codegen::{ArtifactSet, MapForceX12SourceError, Program};
use ir::SchemaNode;
use mapping::FormatOptions;

#[derive(Debug)]
pub enum MapForceSourceEmitError {
    Boundary(MapForceX12SourceError),
    Writer(codegen::MapForceJsonWriterError),
    Emit(EmitError),
    Artifact,
    Xml(codegen::DeferredXmlPlanError),
}
impl std::fmt::Display for MapForceSourceEmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Boundary(error) => error.fmt(f),
            Self::Writer(error) => error.fmt(f),
            Self::Emit(error) => error.fmt(f),
            Self::Artifact => f.write_str("missing ordinary Rust mapping artifact"),
            Self::Xml(error) => error.fmt(f),
        }
    }
}

/// Bind native X12 source projection and strict JSON output; acquisition,
/// filename evaluation, clock phases and publication remain the host's work.
pub fn emit_with_mapforce_x12_json(
    program: &Program,
    options: &Options,
    schema: &SchemaNode,
    source_options: &FormatOptions,
    message: &[String],
) -> Result<ArtifactSet, MapForceSourceEmitError> {
    let descriptor =
        codegen::prepare_mapforce_json_writer(program).map_err(MapForceSourceEmitError::Writer)?;
    let mut files =
        emit_with_mapforce_x12_source(program, options, schema, source_options, message)?
            .into_files();
    let mapping = files
        .iter_mut()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .ok_or(MapForceSourceEmitError::Artifact)?;
    mapping.contents.extend_from_slice(format!("\npub fn serialize_mapforce_json(value: &codegen_runtime::Instance) -> Result<String, codegen_runtime::MapForceJson5Error> {{\n    codegen_runtime::serialize_mapforce_json_ascii_keys({descriptor:?}, value)\n}}\n").as_bytes());
    mapping.contents.extend_from_slice(format!("\npub fn write_mapforce_json(value: &codegen_runtime::Instance, output: &mut dyn std::io::Write) -> Result<(), codegen_runtime::MapForceJsonWriteError> {{\n    codegen_runtime::write_mapforce_json_ascii_keys({descriptor:?}, value, output)\n}}\n").as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(MapForceSourceEmitError::Emit)
}
impl std::error::Error for MapForceSourceEmitError {}

pub fn emit_with_native_deferred_x12_xml(
    document: &codegen::DocumentProgram,
    options: &Options,
    source: &codegen::NativeX12Source<'_>,
    filename_target: &str,
    driver: &[String],
) -> Result<ArtifactSet, MapForceSourceEmitError> {
    let plan = codegen::prepare_deferred_x12_xml(document.program(), filename_target, driver)
        .map_err(MapForceSourceEmitError::Xml)?;
    let descriptor = codegen::prepare_native_x12_source(document.program(), source)
        .map_err(MapForceSourceEmitError::Boundary)?;
    let artifacts = emit_with_native_x12_source(document.program(), options, source)?;
    let mut files = crate::primary_filename::append(artifacts, document)
        .map_err(MapForceSourceEmitError::Emit)?
        .into_files();
    let mapping = files
        .iter_mut()
        .find(|f| f.path.as_str() == "src/lib.rs")
        .ok_or(MapForceSourceEmitError::Artifact)?;
    let driver = plan
        .driver
        .iter()
        .map(|p| format!("{p:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    mapping.contents.extend_from_slice(format!(r#"
pub fn prepare_primary_x12_xml_message<'a>(context:&ScopeContext<'a>,body_clock:Option<&'a dyn codegen_runtime::DocumentBodyClock>,filename_clock:Option<&'a dyn codegen_runtime::DocumentFilenameClock>)->Result<Option<codegen_runtime::DeferredXmlDocument<'a>>,RuntimeError> {{
    if matches!(context.resolve_source_field(&[{driver}])?,Value::Null) {{return Ok(None);}}
    let body_context=context.clone().with_document_body_clock(body_clock);
    let early=scope_extra_1(&body_context)?;
    let filename=scope_extra_0(&context.clone().with_document_filename_clock(filename_clock))?;
    let path=filename.field("Path").and_then(Instance::as_scalar).cloned().unwrap_or(Value::Null);
    let member=codegen_runtime::dynamic_document({},path,early)?;
    Ok(Some(codegen_runtime::DeferredXmlDocument::new({:?},member,move||scope_extra_2(&body_context))))
}}
pub fn acquire_native_primary_x12<'a,E:'a,I:Iterator<Item=Result<String,E>>+'a>(execution:&ExecutionContext<'a>,select:impl FnOnce(&str)->Result<I,E>+'a,load:impl FnMut(&str)->Result<codegen_runtime::ParsedX12Source,E>+'a,maximum:usize)->Result<codegen_runtime::X12SourceAcquisition<'a,RuntimeError,E>,codegen_runtime::X12AcquisitionError<RuntimeError,E>> {{
    let execution=*execution;
    codegen_runtime::X12SourceAcquisition::new({descriptor:?},move||evaluate_primary_filename(&execution),select,load,maximum)
}}
"#,plan.filename_node,plan.descriptor).as_bytes());
    mapping.contents.extend_from_slice(format!(r#"
/// Serialize a materialized element-only XML body. The schema location is
/// explicit literal metadata; this does not open or accept an output file.
pub fn serialize_mapforce_xml(value: &Instance, schema_location: Option<&str>) -> Result<Vec<u8>, codegen_runtime::MapForceXmlError> {{
    codegen_runtime::serialize_mapforce_xml_document({:?}, value, schema_location)
}}
/// Prepare XML before opening the host-owned destination stream.
pub fn prepare_mapforce_xml(value: &Instance, schema_location: Option<&str>) -> Result<codegen_runtime::PreparedMapForceXmlDocument, codegen_runtime::MapForceXmlError> {{
    codegen_runtime::prepare_mapforce_xml_document({:?}, value, schema_location)
}}
"#, plan.descriptor, plan.descriptor).as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(MapForceSourceEmitError::Emit)
}

pub fn emit_with_native_x12_source(
    program: &Program,
    options: &Options,
    source: &codegen::NativeX12Source<'_>,
) -> Result<ArtifactSet, MapForceSourceEmitError> {
    let descriptor = codegen::prepare_native_x12_source(program, source)
        .map_err(MapForceSourceEmitError::Boundary)?;
    let mut files = emit_with_mapforce_x12_source(
        program,
        options,
        source.schema,
        source.options,
        source.message,
    )?
    .into_files();
    let mapping = files
        .iter_mut()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .ok_or(MapForceSourceEmitError::Artifact)?;
    mapping.contents.extend_from_slice(format!(r#"
pub fn parse_native_primary_x12_source(bytes: &[u8]) -> Result<codegen_runtime::Instance, codegen_runtime::X12BoundaryError> {{ codegen_runtime::parse_mapforce_x12_source({descriptor:?}, bytes) }}
pub fn parse_native_primary_x12_document(identity: &str, bytes: &[u8]) -> Result<codegen_runtime::ParsedX12Source, codegen_runtime::X12BoundaryError> {{ codegen_runtime::parse_mapforce_x12_source_document({descriptor:?}, identity, bytes) }}
"#).as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(MapForceSourceEmitError::Emit)
}

/// Retain the original MFD input decoder. The older primary-X12 methods remain
/// explicit UTF-8-fallback companions; these native methods use the bound policy.
pub fn emit_with_native_primary_eager_x12_json(
    document: &codegen::DocumentProgram,
    options: &Options,
    source: &codegen::NativeX12Source<'_>,
    filename_target: &str,
    driver: &[String],
) -> Result<ArtifactSet, MapForceSourceEmitError> {
    let descriptor = codegen::prepare_native_x12_source(document.program(), source)
        .map_err(MapForceSourceEmitError::Boundary)?;
    let mut files = emit_with_primary_eager_x12_json(
        document,
        options,
        source.schema,
        source.options,
        source.message,
        filename_target,
        driver,
    )?
    .into_files();
    let mapping = files
        .iter_mut()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .ok_or(MapForceSourceEmitError::Artifact)?;
    mapping.contents.extend_from_slice(format!(r#"
pub fn parse_native_primary_x12_source(bytes: &[u8]) -> Result<codegen_runtime::Instance, codegen_runtime::X12BoundaryError> {{
    codegen_runtime::parse_mapforce_x12_source({descriptor:?}, bytes)
}}
pub fn parse_native_primary_x12_document(identity: &str, bytes: &[u8]) -> Result<codegen_runtime::ParsedX12Source, codegen_runtime::X12BoundaryError> {{
    codegen_runtime::parse_mapforce_x12_source_document({descriptor:?}, identity, bytes)
}}
pub fn acquire_native_primary_x12<'a, E: 'a, I: Iterator<Item = Result<String, E>> + 'a>(
    execution: &ExecutionContext<'a>, select: impl FnOnce(&str) -> Result<I, E> + 'a,
    load: impl FnMut(&str) -> Result<codegen_runtime::ParsedX12Source, E> + 'a, maximum: usize,
) -> Result<codegen_runtime::X12SourceAcquisition<'a, RuntimeError, E>, codegen_runtime::X12AcquisitionError<RuntimeError, E>> {{
    let execution = *execution;
    codegen_runtime::X12SourceAcquisition::new({descriptor:?}, move || evaluate_primary_filename(&execution), select, load, maximum)
}}
"#).as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(MapForceSourceEmitError::Emit)
}

/// Compose the original source parameter with lazy host-owned selection/load
/// and the eager per-message JSON preparation profile.
pub fn emit_with_primary_eager_x12_json(
    document: &codegen::DocumentProgram,
    options: &Options,
    schema: &SchemaNode,
    source_options: &FormatOptions,
    message: &[String],
    filename_target: &str,
    driver: &[String],
) -> Result<ArtifactSet, MapForceSourceEmitError> {
    let descriptor =
        codegen::prepare_mapforce_x12_source(document.program(), schema, source_options, message)
            .map_err(MapForceSourceEmitError::Boundary)?;
    let artifacts = emit_with_eager_x12_json_message(
        document.program(),
        options,
        schema,
        source_options,
        message,
        filename_target,
        driver,
    )?;
    let mut files = crate::primary_filename::append(artifacts, document)
        .map_err(MapForceSourceEmitError::Emit)?
        .into_files();
    let mapping = files
        .iter_mut()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .ok_or(MapForceSourceEmitError::Artifact)?;
    mapping.contents.extend_from_slice(format!(r#"
pub fn parse_primary_x12_document(identity: &str, bytes: &[u8]) -> Result<codegen_runtime::ParsedX12Source, codegen_runtime::X12BoundaryError> {{
    codegen_runtime::parse_mapforce_x12_source_document({descriptor:?}, identity, bytes)
}}
/// Loaders must finish parsing and input cleanup before returning a document.
pub fn acquire_primary_x12<'a, E: 'a, I: Iterator<Item = Result<String, E>> + 'a>(
    execution: &ExecutionContext<'a>,
    select: impl FnOnce(&str) -> Result<I, E> + 'a,
    load: impl FnMut(&str) -> Result<codegen_runtime::ParsedX12Source, E> + 'a,
    maximum: usize,
) -> Result<codegen_runtime::X12SourceAcquisition<'a, RuntimeError, E>, codegen_runtime::X12AcquisitionError<RuntimeError, E>> {{
    let execution = *execution;
    codegen_runtime::X12SourceAcquisition::new({descriptor:?}, move || evaluate_primary_filename(&execution), select, load, maximum)
}}
"#).as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(MapForceSourceEmitError::Emit)
}

/// Prepare one selected message with eager JSON body evaluation, followed by
/// filename evaluation. This does not read, serialize, open or publish a file.
pub fn emit_with_eager_x12_json_message(
    program: &Program,
    options: &Options,
    schema: &SchemaNode,
    source_options: &FormatOptions,
    message: &[String],
    filename_target: &str,
    driver: &[String],
) -> Result<ArtifactSet, MapForceSourceEmitError> {
    let plan = codegen::prepare_eager_x12_json_message(program, filename_target, driver)
        .map_err(MapForceSourceEmitError::Writer)?;
    let mut files = emit_with_mapforce_x12_json(program, options, schema, source_options, message)?
        .into_files();
    let mapping = files
        .iter_mut()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .ok_or(MapForceSourceEmitError::Artifact)?;
    let driver = plan
        .driver
        .iter()
        .map(|part| format!("{part:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    mapping.contents.extend_from_slice(format!(r#"
/// The caller retains one invocation context while visiting selected messages.
pub fn prepare_primary_x12_json_message<'a>(context: &ScopeContext<'a>, body_clock: Option<&'a dyn codegen_runtime::DocumentBodyClock>, filename_clock: Option<&'a dyn codegen_runtime::DocumentFilenameClock>) -> Result<Option<codegen_runtime::DocumentMember>, RuntimeError> {{
    if matches!(context.resolve_source_field(&[{driver}])?, Value::Null) {{ return Ok(None); }}
    let body_context=context.clone().with_document_body_clock(body_clock);
    let body=scope_root(&body_context)?;
    let filename_context=context.clone().with_document_filename_clock(filename_clock);
    let filename=scope_extra_0(&filename_context)?;
    let path=filename.field("Path").and_then(Instance::as_scalar).cloned().unwrap_or(Value::Null);
    Ok(Some(codegen_runtime::dynamic_document({},path,body)?))
}}
"#,plan.filename_node).as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(MapForceSourceEmitError::Emit)
}

/// Add a bound source reader; hosts still own file acquisition, message iteration
/// and all filename/body/publication phases.
pub fn emit_with_mapforce_x12_source(
    program: &Program,
    options: &Options,
    schema: &SchemaNode,
    source_options: &FormatOptions,
    message: &[String],
) -> Result<ArtifactSet, MapForceSourceEmitError> {
    let descriptor = codegen::prepare_mapforce_x12_source(program, schema, source_options, message)
        .map_err(MapForceSourceEmitError::Boundary)?;
    let mut files = crate::emit(program, options)
        .map_err(MapForceSourceEmitError::Emit)?
        .into_files();
    let mapping = files
        .iter_mut()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .ok_or(MapForceSourceEmitError::Artifact)?;
    mapping.contents.extend_from_slice(format!("\n/// Parse the complete physical X12 source, not a single-message body input.\npub fn parse_primary_x12_source(bytes: &[u8]) -> Result<codegen_runtime::Instance, codegen_runtime::X12BoundaryError> {{\n    codegen_runtime::parse_mapforce_x12_source({descriptor:?}, bytes)\n}}\n").as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(MapForceSourceEmitError::Emit)
}
