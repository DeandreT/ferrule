//! Interprets a mapping graph against a source instance to produce a target instance.

use std::path::Path;
use std::sync::Arc;

#[cfg(test)]
use ir::ScalarType;
use ir::{Instance, Value};
#[cfg(test)]
use mapping::{Graph, IterationOutput, Node, Scope, ScopeConstruction};
use mapping::{JoinId, NodeId, Project, RuntimeValue};
use thiserror::Error;

mod adjacency_tree;
mod aggregate;
mod context;
mod dynamic_target;
mod eval_expr;
mod eval_scope;
mod grouping;
mod iteration_output;
mod join;
mod path_hierarchy;
mod recursive_filter;
mod resolve;
mod sequence;
mod source_iteration;
mod validate;

#[cfg(test)]
use aggregate::{aggregate, value_ordering};
use context::runtime_field;
use eval_scope::eval_scope;

pub use validate::{ValidationIssue, validate};

/// One additional named target value produced by a project run.
#[derive(Debug, Clone)]
pub struct NamedOutput {
    pub name: String,
    pub instance: Instance,
}

/// Every target value produced by one project run.
#[derive(Debug, Clone)]
pub struct ExecutionOutputs {
    pub primary: Instance,
    pub extras: Vec<NamedOutput>,
}

#[derive(Debug, Error, PartialEq)]
pub enum EngineError {
    #[error("mapping graph has no node with id {0}")]
    MissingNode(NodeId),
    #[error("cycle detected while evaluating node {0}")]
    Cycle(NodeId),
    #[error("no source field found at path `{0}`")]
    MissingSourceField(String),
    #[error("node {node}: expected a bool, got {found}")]
    NotABool { node: NodeId, found: &'static str },
    #[error("node {node}: expected an item count, got {found}")]
    NotAnItemCount { node: NodeId, found: &'static str },
    #[error("node {node}: group block size must be greater than zero")]
    InvalidBlockSize { node: NodeId },
    #[error("a scope cannot combine multiple grouping modes")]
    ConflictingGroupingModes,
    #[error("node {node}: value-map lookup missed and there's no default")]
    ValueMapMiss { node: NodeId },
    #[error("execution context does not provide {0:?}")]
    MissingRuntimeValue(RuntimeValue),
    #[error("dynamic source `{source_name}` requires a host source loader")]
    MissingDynamicSourceLoader { source_name: String },
    #[error("dynamic source `{source_name}` path expression produced {found}, expected a string")]
    DynamicSourcePath {
        source_name: String,
        found: &'static str,
    },
    #[error("node {node}: dynamic target path produced {found}, expected a string")]
    DynamicTargetPath { node: NodeId, found: &'static str },
    #[error("node {node}: dynamic target path cannot be empty")]
    EmptyDynamicTargetPath { node: NodeId },
    #[error("loading dynamic source `{source_name}` from `{path}` failed: {message}")]
    DynamicSourceLoad {
        source_name: String,
        path: String,
        message: String,
    },
    #[error("a scope with `filter` but no `source` filtered out its only item")]
    FilteredNonRepeatingScope,
    #[error("node {node}: dynamic target property name must be a string, got {found}")]
    DynamicPropertyName { node: NodeId, found: &'static str },
    #[error("dynamic target object contains duplicate or fixed-colliding property `{0}`")]
    DuplicateDynamicProperty(String),
    #[error("a dynamic object merge can contain only object property fragments")]
    InvalidDynamicPropertyFragment,
    #[error("first-item output requires an iterating scope")]
    FirstOutputWithoutIteration,
    #[error("dynamic object merging requires repeated iteration output")]
    ConflictingIterationOutput,
    #[error("mapped-sequence output cannot populate a computed target property")]
    MappedSequenceDynamicTarget,
    #[error("copy-current-source construction requires a group item, got {found}")]
    CopyCurrentSourceRequiresGroup { found: &'static str },
    #[error("concatenated scope segment produced {found} instead of a group or repeated groups")]
    InvalidConcatenatedScopeItem { found: &'static str },
    #[error("generate-sequence requested {requested} items; maximum is {max}")]
    GeneratedSequenceTooLarge { requested: u128, max: u128 },
    #[error("recursive sequence exceeds the {limit}-group depth limit")]
    RecursiveSequenceDepth { limit: usize },
    #[error("recursive sequence produced more than {max} items")]
    RecursiveSequenceTooLarge { max: u128 },
    #[error("tokenize-regexp pattern is {bytes} bytes; maximum is {max}")]
    TokenizeRegexPatternTooLarge { bytes: usize, max: usize },
    #[error("tokenize-regexp flags `{flags}` contain an unsupported flag")]
    InvalidTokenizeRegexFlags { flags: String },
    #[error("tokenize-regexp pattern is invalid: {message}")]
    InvalidTokenizeRegex { message: String },
    #[error("tokenize-regexp pattern matches a zero-width string")]
    ZeroWidthTokenizeRegex,
    #[error("tokenize-regexp produced more than {max} items")]
    TokenizeRegexTooLarge { max: u128 },
    #[error("recursive filter exceeds the {limit}-group depth limit")]
    RecursiveFilterDepth { limit: usize },
    #[error("recursive filter requires a group item, got {found}")]
    RecursiveFilterRequiresGroup { found: &'static str },
    #[error("recursive filter field `{field}` requires a repeated collection, got {found}")]
    RecursiveFilterRequiresCollection { field: String, found: &'static str },
    #[error("path hierarchy input requires string values, got {found}")]
    PathHierarchyValueType { found: &'static str },
    #[error("path hierarchy exceeds the {limit}-directory depth limit")]
    PathHierarchyDepth { limit: usize },
    #[error("path hierarchy produced more than {max} directory and file items")]
    PathHierarchyTooLarge { max: usize },
    #[error("path hierarchy requires exactly one root directory, got {count}")]
    PathHierarchyRootCount { count: usize },
    #[error("adjacency-tree collection `{0}` is missing")]
    MissingAdjacencyCollection(String),
    #[error("adjacency tree produced more than {max} items")]
    AdjacencyTreeTooLarge { max: u128 },
    #[error("adjacency-tree key `{0}` occurs more than once")]
    DuplicateAdjacencyKey(String),
    #[error("adjacency-tree root requires a string or absent value, got {found}")]
    InvalidAdjacencyRoot { found: &'static str },
    #[error("adjacency tree requires exactly one selected root row, got {count}")]
    AdjacencyRootCount { count: usize },
    #[error("adjacency tree exceeds the {limit}-group depth limit")]
    AdjacencyTreeDepth { limit: usize },
    #[error("adjacency tree contains a cycle at key `{0}`")]
    AdjacencyCycle(String),
    #[error("adjacency-tree {role} field `{path}` requires a string or absent value, got {found}")]
    InvalidAdjacencyField {
        role: &'static str,
        path: String,
        found: &'static str,
    },
    #[error("join {} is not active in the current scope", .join.get())]
    MissingJoinContext { join: JoinId },
    #[error("inner-join iteration cannot be combined with grouping controls")]
    JoinGroupingUnsupported,
    #[error("{function:?} aggregate overflowed the integer range")]
    AggregateIntegerOverflow { function: mapping::AggregateOp },
    #[error("{function:?} aggregate encountered or produced a non-finite number")]
    AggregateNonFinite { function: mapping::AggregateOp },
    #[error(transparent)]
    Function(#[from] functions::FunctionError),
}

/// Runs `project`'s scope tree against `source`, producing one target
/// instance.
pub fn run(project: &Project, source: &Instance) -> Result<Instance, EngineError> {
    Ok(run_outputs_internal(project, source, Vec::new(), None)?.primary)
}

/// Runs every target declared by `project`.
pub fn run_outputs(project: &Project, source: &Instance) -> Result<ExecutionOutputs, EngineError> {
    run_outputs_internal(project, source, Vec::new(), None)
}

/// Host values available to runtime graph nodes.
#[derive(Clone, Copy)]
pub struct ExecutionContext<'a> {
    mapping_file_path: &'a Path,
    main_mapping_file_path: &'a Path,
    current_datetime: Option<&'a str>,
    dynamic_source_loader: Option<&'a dyn DynamicSourceLoader>,
}

/// Host boundary for typed secondary sources whose path is computed during
/// mapping execution. Implementations should cache by source name and path.
pub trait DynamicSourceLoader {
    fn load(&self, source: &str, path: &str) -> Result<Arc<Instance>, String>;
}
impl<'a> ExecutionContext<'a> {
    /// Uses one path for both the active and top-level mapping.
    pub fn new(mapping_file_path: &'a Path) -> Self {
        Self {
            mapping_file_path,
            main_mapping_file_path: mapping_file_path,
            current_datetime: None,
            dynamic_source_loader: None,
        }
    }

    /// Distinguishes a reusable mapping's path from its top-level caller.
    pub fn with_main_mapping_file_path(
        mapping_file_path: &'a Path,
        main_mapping_file_path: &'a Path,
    ) -> Self {
        Self {
            mapping_file_path,
            main_mapping_file_path,
            current_datetime: None,
            dynamic_source_loader: None,
        }
    }

    /// Supplies one stable XML `dateTime` lexical value for the run.
    pub fn with_current_datetime(mut self, current_datetime: &'a str) -> Self {
        self.current_datetime = Some(current_datetime);
        self
    }

    /// Supplies lazy typed-source loading for dynamic secondary inputs.
    pub fn with_dynamic_source_loader(mut self, loader: &'a dyn DynamicSourceLoader) -> Self {
        self.dynamic_source_loader = Some(loader);
        self
    }

    fn value(self, value: RuntimeValue) -> Option<Value> {
        match value {
            RuntimeValue::MappingFilePath => Some(Value::String(
                self.mapping_file_path.to_string_lossy().into_owned(),
            )),
            RuntimeValue::MainMappingFilePath => Some(Value::String(
                self.main_mapping_file_path.to_string_lossy().into_owned(),
            )),
            RuntimeValue::CurrentDateTime => self
                .current_datetime
                .map(|value| Value::String(value.to_string())),
        }
    }
}

/// Like [`run`], with host-provided runtime values.
pub fn run_with_context(
    project: &Project,
    source: &Instance,
    execution: &ExecutionContext<'_>,
) -> Result<Instance, EngineError> {
    Ok(run_outputs_internal(project, source, Vec::new(), Some(execution))?.primary)
}

/// Like [`run`], with named secondary sources. They form the outermost
/// context frame, so scope source paths and field paths reach them by name
/// through the usual outward fallback -- while anything the primary source
/// (or an inner scope item) defines still wins.
pub fn run_with_sources(
    project: &Project,
    source: &Instance,
    extras: Vec<(String, Instance)>,
) -> Result<Instance, EngineError> {
    Ok(run_outputs_internal(project, source, extras, None)?.primary)
}

/// Like [`run_with_sources`], with host-provided runtime values.
pub fn run_with_sources_and_context(
    project: &Project,
    source: &Instance,
    extras: Vec<(String, Instance)>,
    execution: &ExecutionContext<'_>,
) -> Result<Instance, EngineError> {
    Ok(run_outputs_internal(project, source, extras, Some(execution))?.primary)
}

/// Like [`run_outputs`], with named secondary sources and host values.
pub fn run_outputs_with_sources_and_context(
    project: &Project,
    source: &Instance,
    extras: Vec<(String, Instance)>,
    execution: &ExecutionContext<'_>,
) -> Result<ExecutionOutputs, EngineError> {
    run_outputs_internal(project, source, extras, Some(execution))
}

fn run_outputs_internal(
    project: &Project,
    source: &Instance,
    extras: Vec<(String, Instance)>,
    execution: Option<&ExecutionContext<'_>>,
) -> Result<ExecutionOutputs, EngineError> {
    let runtime_frame = Instance::Group(
        execution
            .into_iter()
            .flat_map(|execution| {
                [
                    RuntimeValue::MappingFilePath,
                    RuntimeValue::MainMappingFilePath,
                    RuntimeValue::CurrentDateTime,
                ]
                .into_iter()
                .filter_map(|value| {
                    execution.value(value).map(|instance| {
                        (runtime_field(value).to_string(), Instance::Scalar(instance))
                    })
                })
            })
            .collect(),
    );
    let extras_frame = Instance::Group(extras);
    let context = [&runtime_frame, &extras_frame, source];
    let primary = eval_scope(
        &project.graph,
        &project.root,
        Some(&project.target),
        &context,
        &[],
        &project.extra_sources,
        execution.and_then(|execution| execution.dynamic_source_loader),
    )?;
    let mut targets = Vec::with_capacity(project.extra_targets.len());
    for target in &project.extra_targets {
        targets.push(NamedOutput {
            name: target.name.clone(),
            instance: eval_scope(
                &project.graph,
                &target.root,
                Some(&target.schema),
                &context,
                &[],
                &project.extra_sources,
                execution.and_then(|execution| execution.dynamic_source_loader),
            )?,
        });
    }
    Ok(ExecutionOutputs {
        primary,
        extras: targets,
    })
}

#[cfg(test)]
#[path = "tests/adjacency_tree.rs"]
mod adjacency_tree_tests;
#[cfg(test)]
#[path = "tests/aggregate.rs"]
mod aggregate_tests;
#[cfg(test)]
#[path = "tests/collection.rs"]
mod collection_tests;
#[cfg(test)]
#[path = "tests/core.rs"]
mod core_tests;
#[cfg(test)]
#[path = "tests/dynamic_document_output.rs"]
mod dynamic_document_output_tests;
#[cfg(test)]
#[path = "tests/dynamic_source.rs"]
mod dynamic_source_tests;
#[cfg(test)]
#[path = "tests/dynamic_target.rs"]
mod dynamic_target_tests;
#[cfg(test)]
#[path = "tests/group_blocks.rs"]
mod group_blocks_tests;
#[cfg(test)]
#[path = "tests/group_starting.rs"]
mod group_starting_tests;
#[cfg(test)]
#[path = "tests/iteration_output.rs"]
mod iteration_output_tests;
#[cfg(test)]
#[path = "tests/join.rs"]
mod join_tests;
#[cfg(test)]
#[path = "tests/path_hierarchy.rs"]
mod path_hierarchy_tests;
#[cfg(test)]
#[path = "tests/recursive_filter.rs"]
mod recursive_filter_tests;
#[cfg(test)]
#[path = "tests/repeated_scalar.rs"]
mod repeated_scalar_tests;
#[cfg(test)]
#[path = "tests/sequence_exists.rs"]
mod sequence_exists_tests;
#[cfg(test)]
#[path = "tests/sequence_item_at.rs"]
mod sequence_item_at_tests;
