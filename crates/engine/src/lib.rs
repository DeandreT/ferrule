//! Interprets a mapping graph against a source instance to produce a target instance.

use std::path::Path;

#[cfg(test)]
use ir::ScalarType;
use ir::{Instance, Value};
#[cfg(test)]
use mapping::{Graph, IterationOutput, Node, Scope, ScopeConstruction};
use mapping::{JoinId, NodeId, Project, RuntimeValue};
use thiserror::Error;

mod aggregate;
mod context;
mod dynamic_target;
mod eval_expr;
mod eval_scope;
mod grouping;
mod iteration_output;
mod join;
mod resolve;
mod sequence;
mod source_iteration;
mod validate;
mod validate_join;

#[cfg(test)]
use aggregate::{aggregate, value_ordering};
use context::runtime_field;
use eval_scope::eval_scope;

pub use validate::{ValidationIssue, validate};

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
    #[error("generate-sequence requested {requested} items; maximum is {max}")]
    GeneratedSequenceTooLarge { requested: u128, max: u128 },
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
    run_internal(project, source, Vec::new(), None)
}

/// Host values available to runtime graph nodes.
#[derive(Debug, Clone, Copy)]
pub struct ExecutionContext<'a> {
    mapping_file_path: &'a Path,
    main_mapping_file_path: &'a Path,
    current_datetime: Option<&'a str>,
}

impl<'a> ExecutionContext<'a> {
    /// Uses one path for both the active and top-level mapping.
    pub fn new(mapping_file_path: &'a Path) -> Self {
        Self {
            mapping_file_path,
            main_mapping_file_path: mapping_file_path,
            current_datetime: None,
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
        }
    }

    /// Supplies one stable XML `dateTime` lexical value for the run.
    pub fn with_current_datetime(mut self, current_datetime: &'a str) -> Self {
        self.current_datetime = Some(current_datetime);
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
    run_internal(project, source, Vec::new(), Some(execution))
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
    run_internal(project, source, extras, None)
}

/// Like [`run_with_sources`], with host-provided runtime values.
pub fn run_with_sources_and_context(
    project: &Project,
    source: &Instance,
    extras: Vec<(String, Instance)>,
    execution: &ExecutionContext<'_>,
) -> Result<Instance, EngineError> {
    run_internal(project, source, extras, Some(execution))
}

fn run_internal(
    project: &Project,
    source: &Instance,
    extras: Vec<(String, Instance)>,
    execution: Option<&ExecutionContext<'_>>,
) -> Result<Instance, EngineError> {
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
    eval_scope(
        &project.graph,
        &project.root,
        Some(&project.target),
        &[&runtime_frame, &extras_frame, source],
        &[],
    )
}

#[cfg(test)]
mod aggregate_tests;
#[cfg(test)]
mod collection_tests;
#[cfg(test)]
mod core_tests;
#[cfg(test)]
mod dynamic_target_tests;
#[cfg(test)]
mod group_blocks_tests;
#[cfg(test)]
mod group_starting_tests;
#[cfg(test)]
mod iteration_output_tests;
#[cfg(test)]
mod join_tests;
#[cfg(test)]
mod sequence_exists_tests;
