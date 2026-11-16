//! Deterministic Rust source generation for backend-neutral ferrule programs,
//! including typed instance and bounded schema-shaped JSON entry points.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use codegen::{
    AggregateFunction, AggregateValue, ArtifactPath, ArtifactPathError, ArtifactSet,
    ArtifactSetError, Binding, BoundaryEmissionPlan, CandidateIteration, CompanionSelection,
    Expression, GeneratedFile, GeneratedSequence, GroupingPlan, InnerJoin, IterationKind,
    IterationOutput, IterationPlan, IterationSource, Program, ProgramValidationError, RuntimeValue,
    ScalarTargetDomain, SequenceWindow, SortFilterOrder, TargetConstruction, TargetScope,
    UserFunctionProgram, serialize_embedded_schema, validate_program,
};
use codegen::{CsvOutputError, CsvOutputPolicy, validate_csv_output};
use ir::{ScalarType, Value};
use mapping::{FunctionId, FunctionParameterId, NodeId};

mod expressions;
use expressions::*;
mod iteration;
use iteration::*;
mod scopes;
use scopes::*;
mod literals;
use literals::*;

mod compatibility;
mod csv_api;
mod mapped_csv_document_output;
pub use mapped_csv_document_output::{
    MappedCsvDocumentEmitError, emit_with_mapped_csv_document_output,
};
mod csv_document;
pub use compatibility::Generator;
pub use csv_document::{CsvDocumentEmitError, emit_with_csv_document_adapters};
mod failure;
mod identity;
pub use identity::{
    MAX_PACKAGE_NAME_BYTES, PackageName, RUNTIME_IDENTITY, RuntimeBindingError, runtime_identity,
    runtime_identity_at, verify_runtime_path,
};
mod json5_api;
mod json_api;
mod owned_basic;
mod owned_json_api;
mod primary_csv;
mod primary_csv_document;
mod primary_edi_document;
mod primary_filename;
mod primary_fixed_width_document;
mod primary_mapped_csv_document;
pub use primary_edi_document::emit_with_primary_edi_documents;
mod primary_json_document;
pub use primary_fixed_width_document::emit_with_primary_fixed_width_documents;
mod primary_xml_document;
pub use primary_json_document::emit_with_primary_json_documents;
mod scalar_sequence;
mod selected_json_api;
mod source_contract;

pub use json5_api::{Json5EmitError, emit_with_json5};
pub use primary_csv::{PrimaryCsvEmitError, emit_with_primary_csv_acquisition};
pub use primary_csv_document::emit_with_primary_csv_documents;
pub use primary_filename::emit_with_primary_filename;
pub use primary_mapped_csv_document::{
    PrimaryMappedCsvDocumentEmitError, emit_with_primary_mapped_csv_documents,
};
pub use primary_xml_document::{PrimaryDocumentEmitError, emit_with_primary_xml_documents};
mod x12_api;
mod xml_api;
pub use x12_api::{X12EmitError, emit_with_x12};

/// How a generated Cargo project locates ferrule's small Rust runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeDependency {
    Path(String),
    Version(String),
    Git {
        repository: String,
        revision: String,
    },
}

/// Rust project generation settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub package_name: String,
    pub runtime_dependency: RuntimeDependency,
}

/// Failure to render a complete Rust project.
#[derive(Debug)]
pub enum EmitError {
    UnsupportedDecimalValue { node: NodeId },
    UnsupportedFilterMapV1 { item: NodeId },
    CsvOutput(CsvOutputError),
    InvalidProgram(ProgramValidationError),
    InvalidPackageName(String),
    SchemaSerialization(String),
    EmbeddedSchema(codegen::EmbeddedSchemaError),
    ArtifactPath(ArtifactPathError),
    ArtifactSet(ArtifactSetError),
    Manifest(codegen::ManifestError),
}

impl fmt::Display for EmitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedDecimalValue { node } => write!(
                formatter,
                "decimal literal at node {node} has no admitted generated Rust profile"
            ),
            Self::UnsupportedFilterMapV1 { item } => write!(
                formatter,
                "generated Rust filter_map_v1 item {item} is not implemented"
            ),
            Self::CsvOutput(error) => error.fmt(formatter),
            Self::InvalidProgram(error) => error.fmt(formatter),
            Self::InvalidPackageName(name) => match PackageName::new(name) {
                Err(error) => error.fmt(formatter),
                Ok(_) => write!(formatter, "invalid generated Rust package name {name:?}"),
            },
            Self::SchemaSerialization(message) => {
                write!(formatter, "cannot serialize embedded schema: {message}")
            }
            Self::EmbeddedSchema(error) => error.fmt(formatter),
            Self::ArtifactPath(error) => error.fmt(formatter),
            Self::ArtifactSet(error) => error.fmt(formatter),
            Self::Manifest(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for EmitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CsvOutput(error) => Some(error),
            Self::InvalidProgram(error) => Some(error),
            Self::EmbeddedSchema(error) => Some(error),
            Self::ArtifactPath(error) => Some(error),
            Self::ArtifactSet(error) => Some(error),
            Self::Manifest(error) => Some(error),
            Self::UnsupportedDecimalValue { .. }
            | Self::UnsupportedFilterMapV1 { .. }
            | Self::InvalidPackageName(_)
            | Self::SchemaSerialization(_) => None,
        }
    }
}

impl From<CsvOutputError> for EmitError {
    fn from(error: CsvOutputError) -> Self {
        Self::CsvOutput(error)
    }
}

impl From<ProgramValidationError> for EmitError {
    fn from(error: ProgramValidationError) -> Self {
        Self::InvalidProgram(error)
    }
}

impl From<codegen::EmbeddedSchemaError> for EmitError {
    fn from(error: codegen::EmbeddedSchemaError) -> Self {
        Self::EmbeddedSchema(error)
    }
}

impl From<ArtifactPathError> for EmitError {
    fn from(error: ArtifactPathError) -> Self {
        Self::ArtifactPath(error)
    }
}

impl From<ArtifactSetError> for EmitError {
    fn from(error: ArtifactSetError) -> Self {
        Self::ArtifactSet(error)
    }
}

/// Emits one buildable Rust library project with every companion API.
pub fn emit(program: &Program, options: &Options) -> Result<ArtifactSet, EmitError> {
    emit_with_companions(program, options, CompanionSelection::Complete)
}

/// Emits one buildable Rust library project with the selected companions.
///
/// Program validation runs once and its proved boundary plan drives every
/// renderer. [`CompanionSelection::TypedOnly`] renders only the typed instance
/// entry points and admits no JSON or XML companion descriptor; refusals that
/// precede rendering are those of [`emit`].
pub fn emit_with_companions(
    program: &Program,
    options: &Options,
    companions: CompanionSelection,
) -> Result<ArtifactSet, EmitError> {
    let plan = codegen::prepare_boundary_emission(program, companions)?;
    if !valid_package_name(&options.package_name) {
        return Err(EmitError::InvalidPackageName(options.package_name.clone()));
    }
    codegen::validate_owned_emission(program)?;
    let manifest = render_manifest(options);
    let source = render_library(program, &plan)?;
    ArtifactSet::new([
        GeneratedFile::new(ArtifactPath::new("Cargo.toml")?, manifest),
        GeneratedFile::new(ArtifactPath::new("src/lib.rs")?, source),
    ])
    .map_err(EmitError::from)
}

/// Emit the Basic owned-source family with a mandatory per-run native policy.
/// The ordinary emitter and its closed ownership gate remain unchanged.
pub fn emit_with_basic_owned_sources(
    program: &Program,
    options: &Options,
) -> Result<ArtifactSet, EmitError> {
    validate_program(program)?;
    if !valid_package_name(&options.package_name) {
        return Err(EmitError::InvalidPackageName(options.package_name.clone()));
    }
    codegen::validate_basic_owned_emission(program)?;
    let manifest = render_manifest(options);
    let mut source = owned_basic::render_public_basic_owned_source(program)?;
    source.push_str(&owned_json_api::render(program)?);
    ArtifactSet::new([
        GeneratedFile::new(ArtifactPath::new("Cargo.toml")?, manifest),
        GeneratedFile::new(ArtifactPath::new("src/lib.rs")?, source),
    ])
    .map_err(EmitError::from)
}

/// Emit the Basic owned library for `capabilities` under a mandatory per-run
/// native policy. Complete Basic admission precedes the package-name check.
fn emit_with_owned_capabilities(
    program: &Program,
    options: &Options,
    capabilities: codegen::OwnedCapabilities,
) -> Result<ArtifactSet, EmitError> {
    codegen::validate_basic_owned_emission_with_capabilities(program, capabilities)?;
    if !valid_package_name(&options.package_name) {
        return Err(EmitError::InvalidPackageName(options.package_name.clone()));
    }
    let manifest = render_manifest(options);
    let mut source =
        owned_basic::render_public_basic_owned_capability_source(program, capabilities)?;
    if capabilities.xml_document_sets {
        source.push_str(&xml_api::render_owned_document_sets(program)?);
    }
    ArtifactSet::new([
        GeneratedFile::new(ArtifactPath::new("Cargo.toml")?, manifest),
        GeneratedFile::new(ArtifactPath::new("src/lib.rs")?, source),
    ])
    .map_err(EmitError::from)
}

macro_rules! owned_presets {
    ($($preset:ident => $emit:ident;)*) => {$(
        #[doc = concat!(
            "Emit the Basic owned library for the [`codegen::OwnedPreset::",
            stringify!($preset),
            "`] preset with a mandatory per-run native policy. The ordinary emitter and its closed ownership gate are unchanged."
        )]
        pub fn $emit(program: &Program, options: &Options) -> Result<ArtifactSet, EmitError> {
            emit_with_owned_capabilities(program, options, codegen::OwnedPreset::$preset.capabilities())
        }
    )*};
}

owned_presets! {
    Filters => emit_with_basic_owned_filters;
    Sort => emit_with_basic_owned_sort;
    Windows => emit_with_basic_owned_windows;
    RowFailures => emit_with_basic_owned_row_failures;
    GroupBy => emit_with_basic_owned_group_by;
    SequenceItemAt => emit_with_basic_owned_sequence_item_at;
    GroupAdjacentBy => emit_with_basic_owned_group_adjacent_by;
    GroupStartingWith => emit_with_basic_owned_group_starting_with;
    GroupEndingWith => emit_with_basic_owned_group_ending_with;
    GroupIntoBlocks => emit_with_basic_owned_group_into_blocks;
    FailureSources => emit_with_basic_owned_failure_sources;
    PathHierarchy => emit_with_basic_owned_path_hierarchy;
    AdjacencyTree => emit_with_basic_owned_adjacency_tree;
    RecursiveCollect => emit_with_basic_owned_recursive_collect;
    RecursiveFilter => emit_with_basic_owned_recursive_filter;
    LookupFind => emit_with_basic_owned_lookup_find;
    DynamicField => emit_with_basic_owned_dynamic_field;
    DynamicLoading => emit_with_basic_owned_dynamic_loading;
    CollectionAggregates => emit_with_basic_owned_collection_aggregates;
    Joins => emit_with_basic_owned_joins;
    SequenceExists => emit_with_basic_owned_sequence_exists;
    SequenceAggregate => emit_with_basic_owned_sequence_aggregate;
    XmlSerialize => emit_with_basic_owned_xml_serialize;
    XmlMixedContent => emit_with_basic_owned_xml_mixed_content;
    XmlMixedConstruction => emit_with_basic_owned_xml_mixed_construction;
    DocumentPaths => emit_with_basic_owned_document_paths;
    DynamicDocumentPaths => emit_with_basic_owned_dynamic_document_paths;
    XmlDocumentSets => emit_with_basic_owned_xml_document_sets;
}

/// Emit the ordinary library with explicitly selected bounded CSV adapters.
/// Flat row/schema and literal dialect eligibility are checked before artifacts
/// exist. The ordinary mapper, JSON/XML adapters, and manifest remain intact.
pub fn emit_with_csv_output(
    program: &Program,
    options: &Options,
    policy: &CsvOutputPolicy,
) -> Result<ArtifactSet, EmitError> {
    validate_csv_output(program, policy)?;
    let mut files = emit(program, options)?.into_files();
    let adapters = csv_api::render(program, policy)?;
    for file in &mut files {
        if file.path.as_str() == "src/lib.rs" {
            file.contents.extend_from_slice(adapters.as_bytes());
        }
    }
    ArtifactSet::new(files).map_err(EmitError::from)
}

fn valid_package_name(name: &str) -> bool {
    PackageName::new(name).is_ok()
}

/// Emit typed input and bounded, ordered CSV document-list output APIs.
/// The singular CSV and implicit JSON/XML boundaries are not selected.
pub fn emit_with_csv_document_output(
    program: &Program,
    options: &Options,
    policy: &CsvOutputPolicy,
) -> Result<ArtifactSet, EmitError> {
    codegen::validate_csv_document_output(program, policy)?;
    let mut files =
        emit_with_companions(program, options, CompanionSelection::TypedOnly)?.into_files();
    let adapters = csv_api::render_documents(program, policy)?;
    for file in &mut files {
        if file.path.as_str() == "src/lib.rs" {
            file.contents.extend_from_slice(adapters.as_bytes());
        }
    }
    ArtifactSet::new(files).map_err(EmitError::from)
}

fn render_manifest(options: &Options) -> String {
    let dependency = match &options.runtime_dependency {
        RuntimeDependency::Path(path) => format!("path = {}", toml_string(path)),
        RuntimeDependency::Version(version) => format!("version = {}", toml_string(version)),
        RuntimeDependency::Git {
            repository,
            revision,
        } => format!(
            "git = {}, rev = {}",
            toml_string(repository),
            toml_string(revision)
        ),
    };
    format!(
        "[package]\nname = {}\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n\n[dependencies]\ncodegen-runtime = {{ {dependency} }}\n\n[workspace]\n",
        toml_string(&options.package_name)
    )
}

fn render_selected_target_api(program: &Program, filter_map: bool) -> String {
    let mut output = String::from(
        "#[derive(Clone, Debug, PartialEq)]\npub enum SelectedTargetOutput {\n    Primary(Instance),\n    Named(NamedOutput),\n}\n\n#[derive(Clone, Copy)]\nenum ResolvedTarget {\n    Primary,\n",
    );
    for index in 0..program.extra_targets.len() {
        output.push_str(&format!("    Named{index},\n"));
    }
    output.push_str("}\n\nfn resolve_target(selection: TargetSelection<'_>) -> Result<ResolvedTarget, RuntimeError> {\n    match selection {\n        TargetSelection::Primary => Ok(ResolvedTarget::Primary),\n        TargetSelection::Named(name) => match name {\n");
    for (index, target) in program.extra_targets.iter().enumerate() {
        output.push_str(&format!(
            "            {} => Ok(ResolvedTarget::Named{index}),\n",
            rust_string(&target.name)
        ));
    }
    output.push_str("            _ => Err(RuntimeError::UnknownTarget { name: name.to_string() }),\n        },\n    }\n}\n\n");
    output.push_str(
        "pub fn execute_selected_target(\n    source: &Instance,\n    selection: TargetSelection<'_>,\n) -> Result<SelectedTargetOutput, RuntimeError> {\n    execute_selected_target_with_host(source, &[], None, None, selection)\n}\n\npub fn execute_selected_target_with_host<'a>(\n    source: &'a Instance,\n    inputs: &[NamedInput<'a>],\n    execution: Option<&ExecutionContext<'a>>,\n    loader: Option<&'a dyn DynamicSourceLoader>,\n    selection: TargetSelection<'_>,\n) -> Result<SelectedTargetOutput, RuntimeError> {\n    let selected = resolve_target(selection)?;\n    let inputs = validate_named_inputs(inputs)?;\n    let context = match execution {\n        Some(execution) => ScopeContext::with_named_inputs_and_execution_context(source, &inputs, execution),\n        None => ScopeContext::with_named_inputs(source, &inputs),\n    };\n    let context = match loader { Some(loader) => context.with_dynamic_source_loader(loader), None => context };\n    execute_selected_target_from_context(&context, selected)\n}\n\npub fn execute_selected_target_with_filter_map_controls<'a>(\n    source: &'a Instance,\n    inputs: &[NamedInput<'a>],\n    execution: Option<&ExecutionContext<'a>>,\n    loader: Option<&'a dyn DynamicSourceLoader>,\n    selection: TargetSelection<'_>,\n    limits: FilterMapLimits,\n    cancellation: Option<&'a dyn FilterMapCancellation>,\n) -> Result<FilterMapExecution<SelectedTargetOutput>, RuntimeError> {\n    let selected = resolve_target(selection)?;\n    let inputs = validate_named_inputs(inputs)?;\n    let run = FilterMapRun::new(limits, cancellation);\n    let context = match execution {\n        Some(execution) => ScopeContext::with_named_inputs_and_execution_context(source, &inputs, execution),\n        None => ScopeContext::with_named_inputs(source, &inputs),\n    };\n    let context = context.with_filter_map_run(&run);\n    let context = match loader { Some(loader) => context.with_dynamic_source_loader(loader), None => context };\n    let outcome = execute_selected_target_from_context(&context, selected);\n    Ok(FilterMapExecution { outcome, counters: run.counters() })\n}\n\nfn execute_selected_target_from_context(\n    context: &ScopeContext<'_>,\n    selected: ResolvedTarget,\n) -> Result<SelectedTargetOutput, RuntimeError> {\n",
    );
    source_contract::render_admission(program, &mut output);
    if filter_map {
        output.push_str("    let run_context = context.with_filter_map_defaults();\n    let context = &run_context;\n");
    }
    output.push_str("    evaluate_failure_rules(context)?;\n    match selected {\n        ResolvedTarget::Primary => Ok(SelectedTargetOutput::Primary(scope_root(context)?)),\n");
    for (index, target) in program.extra_targets.iter().enumerate() {
        output.push_str(&format!("        ResolvedTarget::Named{index} => Ok(SelectedTargetOutput::Named(NamedOutput {{\n            name: {},\n            instance: scope_extra_{index}(context)?,\n        }})),\n", rust_string(&target.name)));
    }
    output.push_str("    }\n}\n\n");
    output
}

/// Renders the ordinary library under the complete companion selection.
#[cfg(test)]
fn render_source(program: &Program) -> Result<String, EmitError> {
    render_library(
        program,
        &codegen::prepare_boundary_emission(program, CompanionSelection::Complete)?,
    )
}

fn render_library(program: &Program, plan: &BoundaryEmissionPlan<'_>) -> Result<String, EmitError> {
    let filter_map = !program.filter_map_v1_sequences().is_empty();
    let mut source = String::from(
        "// Generated by ferrule. Do not edit.\n\
         #[allow(unused_imports)]\n\
         use codegen_runtime::{\n\
             AdjacencyTreeFields, AggregateFunction, DynamicSourceItems, ExecutionContext, GeneratedItems, GroupedItems, InnerJoinKey,\n\
             InnerJoinStage, Instance, JsonBoundaryError, RecursiveCollectPaths,\n\
             RuntimeError, RuntimeValue,\n\
             ScalarType, ScopeContext,\n\
             FilterMapCancellation, FilterMapCapture, FilterMapDescriptor, FilterMapExecution,\n\
             FilterMapFunction, FilterMapInput, FilterMapLimits, FilterMapRun, filter_map_sequence,\n\
             SequenceWindow, SortDirection, Value, adapt_target_value, adapt_union_target_value, aggregate,\n\
             adapt_user_function_value, apply_sequence_windows, call,\n\
             collection_find_selected, field, generate_sequence,\n\
             dynamic_document, dynamic_property_name, group, insert_dynamic_field,\n\
             item_count, merge_dynamic_fragments, repeated,\n\
             recursive_collect, recursive_filter, recursive_sequence_parameter, require_bool, scalar,\n\
             parse_json, parse_json_bytes, serialize_json, serialize_json_bytes, serialize_xml, sort_candidates, tokenize, tokenize_by_length, tokenize_regex, value_map,\n\
             adjacency_tree, path_hierarchy,\n\
             preserve_xml_mixed_content, xml_mixed_content, XmlMixedContentElement,\n\
             XmlMixedContentReplacement,\n\
         };\n\n",
    );
    source.push_str(
        "pub use codegen_runtime::NamedInput;\n\
         pub use codegen_runtime::TargetSelection;\n\
         pub use codegen_runtime::{DynamicJsonSourceLoader, DynamicSourceLoader};\n\n",
    );
    source.push_str(
        "#[derive(Clone, Debug, PartialEq)]\n\
         pub struct NamedOutput {\n\
             pub name: &'static str,\n\
             pub instance: Instance,\n\
         }\n\n\
         #[derive(Clone, Debug, PartialEq)]\n\
         pub struct ExecutionOutputs {\n\
             pub primary: Instance,\n\
             pub extras: Vec<NamedOutput>,\n\
         }\n\n",
    );
    source.push_str(
        "pub fn execute(source: &Instance) -> Result<Instance, RuntimeError> {\n\
             Ok(execute_outputs(source)?.primary)\n\
         }\n\n\
         pub fn execute_with_context(\n\
             source: &Instance,\n\
             execution: &ExecutionContext<'_>,\n\
         ) -> Result<Instance, RuntimeError> {\n\
             Ok(execute_outputs_with_context(source, execution)?.primary)\n\
         }\n\n\
         pub fn execute_with_sources(\n\
             source: &Instance,\n\
             inputs: &[NamedInput<'_>],\n\
         ) -> Result<Instance, RuntimeError> {\n\
             Ok(execute_outputs_with_sources(source, inputs)?.primary)\n\
         }\n\n\
         pub fn execute_with_sources_and_context(\n\
             source: &Instance,\n\
             inputs: &[NamedInput<'_>],\n\
             execution: &ExecutionContext<'_>,\n\
         ) -> Result<Instance, RuntimeError> {\n\
             Ok(execute_outputs_with_sources_and_context(source, inputs, execution)?.primary)\n\
         }\n\n\
         pub fn execute_with_dynamic_source_loader(\n\
             source: &Instance,\n\
             loader: &dyn DynamicSourceLoader,\n\
         ) -> Result<Instance, RuntimeError> {\n\
             Ok(execute_outputs_with_dynamic_source_loader(source, loader)?.primary)\n\
         }\n\n\
         pub fn execute_with_sources_and_dynamic_source_loader(\n\
             source: &Instance,\n\
             inputs: &[NamedInput<'_>],\n\
             loader: &dyn DynamicSourceLoader,\n\
         ) -> Result<Instance, RuntimeError> {\n\
             Ok(execute_outputs_with_sources_and_dynamic_source_loader(source, inputs, loader)?.primary)\n\
         }\n\n\
         pub fn execute_with_sources_context_and_dynamic_source_loader(\n\
             source: &Instance,\n\
             inputs: &[NamedInput<'_>],\n\
             execution: &ExecutionContext<'_>,\n\
             loader: &dyn DynamicSourceLoader,\n\
         ) -> Result<Instance, RuntimeError> {\n\
             Ok(execute_outputs_with_sources_context_and_dynamic_source_loader(source, inputs, execution, loader)?.primary)\n\
         }\n\n\
         pub fn execute_outputs(source: &Instance) -> Result<ExecutionOutputs, RuntimeError> {\n\
             execute_outputs_with_sources(source, &[])\n\
         }\n\n\
         pub fn execute_outputs_with_context(\n\
             source: &Instance,\n\
             execution: &ExecutionContext<'_>,\n\
         ) -> Result<ExecutionOutputs, RuntimeError> {\n\
             execute_outputs_with_sources_and_context(source, &[], execution)\n\
         }\n\n\
         pub fn execute_outputs_with_sources(\n\
             source: &Instance,\n\
             inputs: &[NamedInput<'_>],\n\
         ) -> Result<ExecutionOutputs, RuntimeError> {\n\
             let inputs = validate_named_inputs(inputs)?;\n\
             execute_outputs_from_context(&ScopeContext::with_named_inputs(source, &inputs))\n\
         }\n\n\
         pub fn execute_outputs_with_sources_and_context(\n\
             source: &Instance,\n\
             inputs: &[NamedInput<'_>],\n\
             execution: &ExecutionContext<'_>,\n\
         ) -> Result<ExecutionOutputs, RuntimeError> {\n\
             let inputs = validate_named_inputs(inputs)?;\n\
             execute_outputs_from_context(&ScopeContext::with_named_inputs_and_execution_context(\n\
                 source,\n\
                 &inputs,\n\
                 execution,\n\
             ))\n\
         }\n\n\
         pub fn execute_outputs_with_dynamic_source_loader(\n\
             source: &Instance,\n\
             loader: &dyn DynamicSourceLoader,\n\
         ) -> Result<ExecutionOutputs, RuntimeError> {\n\
             execute_outputs_with_sources_and_dynamic_source_loader(source, &[], loader)\n\
         }\n\n\
         pub fn execute_outputs_with_sources_and_dynamic_source_loader(\n\
             source: &Instance,\n\
             inputs: &[NamedInput<'_>],\n\
             loader: &dyn DynamicSourceLoader,\n\
         ) -> Result<ExecutionOutputs, RuntimeError> {\n\
             let inputs = validate_named_inputs(inputs)?;\n\
             execute_outputs_from_context(\n\
                 &ScopeContext::with_named_inputs(source, &inputs)\n\
                     .with_dynamic_source_loader(loader),\n\
             )\n\
         }\n\n\
         pub fn execute_outputs_with_sources_context_and_dynamic_source_loader(\n\
             source: &Instance,\n\
             inputs: &[NamedInput<'_>],\n\
             execution: &ExecutionContext<'_>,\n\
             loader: &dyn DynamicSourceLoader,\n\
         ) -> Result<ExecutionOutputs, RuntimeError> {\n\
             let inputs = validate_named_inputs(inputs)?;\n\
             execute_outputs_from_context(\n\
                 &ScopeContext::with_named_inputs_and_execution_context(source, &inputs, execution)\n\
                     .with_dynamic_source_loader(loader),\n\
             )\n\
         }\n\n",
    );
    source.push_str(&render_selected_target_api(program, filter_map));
    if plan.json_companions() {
        source.push_str(&json_api::render(program)?);
        source.push_str(&selected_json_api::render(program));
    }
    if let Some(xml) = plan.xml() {
        source.push_str(&xml_api::render_plan(program, xml)?);
    }

    let input_names = program
        .extra_sources
        .iter()
        .filter(|input| input.dynamic.is_none())
        .map(|input| rust_string(&input.name))
        .collect::<Vec<_>>()
        .join(", ");
    source.push_str(&format!(
        "pub const EXTRA_SOURCE_NAMES: &[&str] = &[{input_names}];\n\n"
    ));
    source.push_str(
        "fn validate_named_inputs<'a>(\n    inputs: &[NamedInput<'a>],\n) -> Result<Vec<NamedInput<'a>>, RuntimeError> {\n",
    );
    let static_sources = program
        .extra_sources
        .iter()
        .filter(|source| source.dynamic.is_none())
        .collect::<Vec<_>>();
    if program.execution_contract.is_some() {
        source.push_str(
            "    codegen_runtime::validate_source_contract_bindings(inputs.len(), 0)?;\n",
        );
    }
    if static_sources.is_empty() {
        source.push_str(
            "    if let Some(input) = inputs.first() {\n        return Err(RuntimeError::UnexpectedNamedSource {\n            name: input.name.to_string(),\n        });\n    }\n    Ok(Vec::new())\n}\n\n",
        );
    } else {
        source.push_str(&format!(
            "    let mut matched: Vec<Option<NamedInput<'a>>> = vec![None; {}];\n",
            static_sources.len()
        ));
        source.push_str("    for input in inputs {\n        let index = match input.name {\n");
        for (index, input) in static_sources.iter().enumerate() {
            source.push_str(&format!(
                "            {} => {index},\n",
                rust_string(&input.name)
            ));
        }
        source.push_str(
            "            _ => return Err(RuntimeError::UnexpectedNamedSource { name: input.name.to_string() }),\n        };\n        if matched[index].is_some() {\n            return Err(RuntimeError::DuplicateNamedSource {\n                name: EXTRA_SOURCE_NAMES[index],\n            });\n        }\n        matched[index] = Some(*input);\n    }\n    let mut ordered = Vec::with_capacity(EXTRA_SOURCE_NAMES.len());\n    for (index, name) in EXTRA_SOURCE_NAMES.iter().copied().enumerate() {\n        let Some(input) = matched[index].take() else {\n            return Err(RuntimeError::MissingNamedSource { name });\n        };\n        ordered.push(input);\n    }\n    Ok(ordered)\n}\n\n",
        );
    }

    source.push_str(
        "pub fn execute_outputs_with_filter_map_controls<'a>(\n\
             source: &'a Instance,\n\
             inputs: &'a [NamedInput<'a>],\n\
             execution: Option<&ExecutionContext<'a>>,\n\
             loader: Option<&'a dyn DynamicSourceLoader>,\n\
             limits: FilterMapLimits,\n\
             cancellation: Option<&'a dyn FilterMapCancellation>,\n\
         ) -> Result<FilterMapExecution<ExecutionOutputs>, RuntimeError> {\n\
             let inputs = validate_named_inputs(inputs)?;\n\
             let run = FilterMapRun::new(limits, cancellation);\n\
             let context = match execution {\n\
                 Some(execution) => ScopeContext::with_named_inputs_and_execution_context(source, &inputs, execution),\n\
                 None => ScopeContext::with_named_inputs(source, &inputs),\n\
             };\n\
             let context = context.with_filter_map_run(&run);\n\
             let context = match loader { Some(loader) => context.with_dynamic_source_loader(loader), None => context };\n\
             let outcome = execute_outputs_from_context(&context);\n\
             Ok(FilterMapExecution { outcome, counters: run.counters() })\n\
         }\n\n",
    );
    source.push_str(
        "fn execute_outputs_from_context(\n    context: &ScopeContext<'_>,\n) -> Result<ExecutionOutputs, RuntimeError> {\n",
    );
    source_contract::render_admission(program, &mut source);
    if filter_map {
        source.push_str("    let run_context = context.with_filter_map_defaults();\n    let context = &run_context;\n");
    }
    source.push_str(
        "    evaluate_failure_rules(context)?;\n    let primary = scope_root(context)?;\n",
    );
    if program.extra_targets.is_empty() {
        source.push_str("    let extras = Vec::new();\n");
    } else {
        source.push_str(&format!(
            "    let mut extras = Vec::with_capacity({});\n",
            program.extra_targets.len()
        ));
        for (index, target) in program.extra_targets.iter().enumerate() {
            source.push_str(&format!(
                "    extras.push(NamedOutput {{\n        name: {},\n        instance: scope_extra_{index}(context)?,\n    }});\n",
                rust_string(&target.name)
            ));
        }
    }
    source.push_str("    Ok(ExecutionOutputs { primary, extras })\n}\n\n");

    let functions = program
        .user_functions
        .iter()
        .map(|function| (function.id, function))
        .collect::<BTreeMap<_, _>>();
    for function in &program.user_functions {
        source.push_str(&render_user_function(function, &functions, filter_map)?);
    }
    let mut scopes = Vec::new();
    collect_scopes(&program.root, "scope_root".to_string(), &mut scopes);
    for (index, target) in program.extra_targets.iter().enumerate() {
        collect_scopes(&target.root, format!("scope_extra_{index}"), &mut scopes);
    }
    let retained_expressions = expression_usage(program, &scopes);
    let item_ids: BTreeSet<_> = scopes
        .iter()
        .filter_map(|(_, scope, _, _)| {
            scope
                .iteration
                .as_ref()
                .and_then(IterationPlan::generated_sequence)
                .map(GeneratedSequence::item)
        })
        .chain(
            program
                .expressions
                .iter()
                .filter_map(|node| match &node.expression {
                    Expression::SequenceExists { sequence, .. }
                    | Expression::SequenceItemAt { sequence, .. }
                    | Expression::SequenceAggregate { sequence, .. } => Some(sequence.item()),
                    _ => None,
                }),
        )
        .chain(
            program
                .failure_rules
                .iter()
                .filter_map(|rule| match &rule.iteration {
                    codegen::FailureIteration::Generated(sequence) => Some(sequence.item()),
                    codegen::FailureIteration::Source(_)
                    | codegen::FailureIteration::OwnedSource(_) => None,
                }),
        )
        .collect();
    for node in &program.expressions {
        // Render even an omitted expression, preserving all emission validation
        // errors and their original expression order.
        let rendered = render_expression(
            node.id,
            &node.expression,
            "expression_",
            None,
            &functions,
            filter_map,
            None,
        )?;
        if !retained_expressions.contains(&node.id) {
            continue;
        }
        if item_ids.contains(&node.id) {
            // Portable validation retains item identity even when no expression
            // reads its value. Only these owned metadata functions may be unused.
            source.push_str("#[allow(dead_code)]\n");
        }
        source.push_str(&rendered);
    }
    source.push_str(&failure::render(program)?);

    for (name, scope, child_names, segment_names) in scopes {
        source.push_str(&render_scope(
            program,
            &name,
            scope,
            &child_names,
            &segment_names,
        )?);
    }
    source_contract::render(program, &mut source)?;
    Ok(source)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod direct_program_tests;

// Private checked prototype; public emission and prepared entries stay exact.
#[cfg(test)]
pub(crate) fn render_checked_private_owned_basic_source(
    program: &Program,
) -> Result<String, EmitError> {
    owned_basic::render_checked_basic_owned_source(program)
}
