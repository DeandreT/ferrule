use std::fmt;
use std::path::Path;

use codegen::{Expression, IterationSource, Program, TargetConstruction, TargetScope};
use codegen_runtime::JsonBoundaryError;
use ir::{Instance, SchemaNode, Value};
use mapping::Project;

use super::envelope::{Document, ordered_json_equal};

#[derive(Debug)]
pub(super) enum AdmissionError {
    ImportWarnings {
        count: usize,
    },
    Dependency,
    Validation,
    Lowering,
    Unsupported(&'static str),
    NativeJson {
        slot: usize,
    },
    Schema {
        slot: usize,
    },
    Boundary {
        slot: usize,
        error: JsonBoundaryError,
    },
    SourceRoundtrip {
        slot: usize,
    },
    TargetFixedPoint {
        slot: usize,
    },
    Execution,
    ContextControl,
    TransportedOutput,
    NamedOutputIdentity,
}

impl fmt::Display for AdmissionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ImportWarnings { count } => {
                write!(f, "generic corpus import has {count} warning(s)")
            }
            Self::NativeJson { slot } => write!(f, "native JSON boundary failed for slot {slot}"),
            Self::Schema { slot } => write!(f, "embedded schema boundary failed for slot {slot}"),
            Self::Boundary { slot, error } => {
                write!(f, "strict JSON boundary failed for slot {slot}: {error}")
            }
            Self::SourceRoundtrip { slot } => {
                write!(f, "source slot {slot} changed during JSON transport")
            }
            Self::TargetFixedPoint { slot } => {
                write!(f, "target slot {slot} changed during JSON normalization")
            }
            Self::Unsupported(reason) => {
                write!(f, "generic corpus transport is unsupported: {reason}")
            }
            _ => write!(f, "generic corpus admission rejected: {self:?}"),
        }
    }
}
impl std::error::Error for AdmissionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Boundary { error, .. } => Some(error),
            _ => None,
        }
    }
}

pub(super) struct Admitted {
    pub inputs: Vec<Document>,
    pub outputs: Vec<Document>,
}

pub(super) fn check_warnings(count: usize) -> Result<(), AdmissionError> {
    if count == 0 {
        Ok(())
    } else {
        Err(AdmissionError::ImportWarnings { count })
    }
}

fn scopes_supported(scope: &TargetScope) -> bool {
    !matches!(
        scope.construction,
        TargetConstruction::XmlMixedContent { .. }
    ) && scope.children.iter().all(scopes_supported)
        && match &scope.construction {
            TargetConstruction::DynamicGroup { children, .. } => {
                children.iter().all(|child| scopes_supported(&child.scope))
            }
            _ => true,
        }
        && match scope.iteration.as_ref().map(|iteration| iteration.input()) {
            Some(IterationSource::DynamicDocuments(_)) => false,
            Some(IterationSource::Concatenate(sequence)) => sequence.iter().all(scopes_supported),
            _ => true,
        }
}

pub(super) fn check_program(program: &Program) -> Result<(), AdmissionError> {
    if program
        .extra_sources
        .iter()
        .any(|source| source.dynamic.is_some())
    {
        return Err(AdmissionError::Unsupported("dynamic source"));
    }
    if !program.failure_rules.is_empty() {
        return Err(AdmissionError::Unsupported("failure rules"));
    }
    for expression in program.expressions.iter().chain(
        program
            .user_functions
            .iter()
            .flat_map(|function| &function.expressions),
    ) {
        match &expression.expression {
            Expression::RuntimeValue { .. }
            | Expression::RuntimeParameter { .. }
            | Expression::RuntimeParameterDefault { .. } => {
                return Err(AdmissionError::Unsupported("runtime context"));
            }
            Expression::SourceDocumentPath => {
                return Err(AdmissionError::Unsupported("source document path"));
            }
            Expression::XmlMixedContent { .. } => {
                return Err(AdmissionError::Unsupported("mixed XML expression"));
            }
            Expression::Call {
                function: codegen::ScalarFunction::CreateGuid,
                ..
            } => return Err(AdmissionError::Unsupported("nondeterministic function")),
            _ => {}
        }
    }
    if !scopes_supported(&program.root)
        || program
            .extra_targets
            .iter()
            .any(|target| !scopes_supported(&target.root))
    {
        return Err(AdmissionError::Unsupported(
            "document or mixed XML construction",
        ));
    }
    Ok(())
}

fn ordinary(value: &Instance) -> bool {
    match value {
        Instance::DocumentSet(_) => false,
        Instance::Scalar(Value::XmlNil(_)) => false,
        Instance::Scalar(_) => true,
        Instance::Group(fields) => fields.iter().all(|(_, value)| ordinary(value)),
        Instance::Repeated(items) | Instance::MappedSequence(items) => items.iter().all(ordinary),
    }
}

pub(super) fn source_document(
    schema: &SchemaNode,
    value: &Instance,
    slot: usize,
) -> Result<(String, Instance), AdmissionError> {
    if !ordinary(value) {
        return Err(AdmissionError::Unsupported("nil or document source"));
    }
    let document =
        format_json::to_string(schema, value).map_err(|_| AdmissionError::NativeJson { slot })?;
    let native = format_json::from_str(&document, schema)
        .map_err(|_| AdmissionError::NativeJson { slot })?;
    let codec = codegen::serialize_embedded_schema(schema, codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES)
        .map_err(|_| AdmissionError::Schema { slot })?;
    let boundary = |error| AdmissionError::Boundary { slot, error };
    let strict = codegen_runtime::parse_json(&codec, &document).map_err(boundary)?;
    let bytes = codegen_runtime::parse_json_bytes(&codec, document.as_bytes()).map_err(boundary)?;
    let round = codegen_runtime::serialize_json(&codec, &strict).map_err(boundary)?;
    let raw = codegen_runtime::serialize_json(&codec, value).map_err(boundary)?;
    if native != *value
        || strict != *value
        || bytes != strict
        || !ordered_json_equal(&document, &round)
        || !ordered_json_equal(&document, &raw)
    {
        return Err(AdmissionError::SourceRoundtrip { slot });
    }
    Ok((document, strict))
}

pub(super) fn target_document(
    schema: &SchemaNode,
    value: &Instance,
    slot: usize,
) -> Result<String, AdmissionError> {
    if !ordinary(value) {
        return Err(AdmissionError::Unsupported("nil or document target"));
    }
    let document =
        format_json::to_string(schema, value).map_err(|_| AdmissionError::NativeJson { slot })?;
    let codec = codegen::serialize_embedded_schema(schema, codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES)
        .map_err(|_| AdmissionError::Schema { slot })?;
    let boundary = |error| AdmissionError::Boundary { slot, error };
    let strict = codegen_runtime::parse_json(&codec, &document).map_err(boundary)?;
    let strict_bytes =
        codegen_runtime::parse_json_bytes(&codec, document.as_bytes()).map_err(boundary)?;
    let round = codegen_runtime::serialize_json(&codec, &strict).map_err(boundary)?;
    let raw = codegen_runtime::serialize_json(&codec, value).map_err(boundary)?;
    let raw_bytes = codegen_runtime::serialize_json_bytes(&codec, value).map_err(boundary)?;
    if strict != strict_bytes
        || !ordered_json_equal(&document, &round)
        || !ordered_json_equal(&document, &raw)
        || raw_bytes != raw.as_bytes()
    {
        return Err(AdmissionError::TargetFixedPoint { slot });
    }
    Ok(document)
}

pub(super) fn admit(
    project: &Project,
    source: &Instance,
    extras: &[(String, Instance)],
    mapping_path: &Path,
) -> Result<Admitted, AdmissionError> {
    if !project.runtime_dependencies().is_empty() {
        return Err(AdmissionError::Dependency);
    }
    if !engine::validate(project).is_empty() {
        return Err(AdmissionError::Validation);
    }
    let program = codegen::lower(project).map_err(|_| AdmissionError::Lowering)?;
    check_program(&program)?;
    if project.source_options.local_xml_file_set
        || project
            .extra_sources
            .iter()
            .any(|source| source.options.local_xml_file_set)
    {
        return Err(AdmissionError::Unsupported("file set"));
    }
    if extras.len() != project.extra_sources.len()
        || extras
            .iter()
            .zip(&project.extra_sources)
            .any(|((name, _), source)| name != &source.name)
    {
        return Err(AdmissionError::Unsupported("named input identity"));
    }
    let (primary, restored) = source_document(&project.source, source, 0)?;
    let mut inputs = vec![Document {
        name: String::new(),
        json: primary,
    }];
    let mut restored_extras = Vec::new();
    for (index, ((name, value), source)) in extras.iter().zip(&project.extra_sources).enumerate() {
        let (document, value) = source_document(&source.schema, value, index + 1)?;
        inputs.push(Document {
            name: name.clone(),
            json: document,
        });
        restored_extras.push((name.clone(), value));
    }
    let execution = engine::ExecutionContext::new(mapping_path);
    let native =
        engine::run_outputs_with_sources_and_context(project, source, extras.to_vec(), &execution)
            .map_err(|_| AdmissionError::Execution)?;
    let primary_control = engine::run_with_sources(project, source, extras.to_vec())
        .map_err(|_| AdmissionError::ContextControl)?;
    if native.primary != primary_control {
        return Err(AdmissionError::ContextControl);
    }
    if extras.is_empty() {
        let control =
            engine::run_outputs(project, source).map_err(|_| AdmissionError::ContextControl)?;
        if native.primary != control.primary || native.extras != control.extras {
            return Err(AdmissionError::ContextControl);
        }
    }
    let transported = engine::run_outputs_with_sources_and_context(
        project,
        &restored,
        restored_extras,
        &execution,
    )
    .map_err(|_| AdmissionError::TransportedOutput)?;
    if native.primary != transported.primary || native.extras != transported.extras {
        return Err(AdmissionError::TransportedOutput);
    }
    if native.extras.len() != project.extra_targets.len()
        || native
            .extras
            .iter()
            .zip(&project.extra_targets)
            .any(|(output, target)| output.name != target.name)
    {
        return Err(AdmissionError::NamedOutputIdentity);
    }
    let mut outputs = vec![Document {
        name: String::new(),
        json: target_document(&project.target, &native.primary, 0)?,
    }];
    for (index, (output, target)) in native.extras.iter().zip(&project.extra_targets).enumerate() {
        outputs.push(Document {
            name: output.name.clone(),
            json: target_document(&target.schema, &output.instance, index + 1)?,
        });
    }
    Ok(Admitted { inputs, outputs })
}
