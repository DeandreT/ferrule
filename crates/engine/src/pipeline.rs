//! In-memory execution of typed, ordered mapping stages.

use std::collections::{BTreeMap, BTreeSet};

use ir::{Instance, SchemaNode};
use mapping::{Pipeline, PipelineInput};
use thiserror::Error;

use crate::{
    EngineError, ExecutionContext, ExecutionOutputs, run_outputs_with_sources_and_context,
};

/// A problem with one stage graph or one of its complete mappings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineValidationIssue {
    pub stage: Option<String>,
    pub message: String,
}

impl std::fmt::Display for PipelineValidationIssue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.stage {
            Some(stage) => write!(formatter, "stage `{stage}`: {}", self.message),
            None => formatter.write_str(&self.message),
        }
    }
}

/// A complete mapping result, identified by its stage.
#[derive(Debug, Clone)]
pub struct PipelineStageOutput {
    pub id: String,
    pub outputs: ExecutionOutputs,
}

/// Stage results in their deterministic execution order.
#[derive(Debug, Clone)]
pub struct PipelineOutputs {
    pub stages: Vec<PipelineStageOutput>,
}

impl PipelineOutputs {
    pub fn stage(&self, id: &str) -> Option<&ExecutionOutputs> {
        self.stages
            .iter()
            .find(|stage| stage.id == id)
            .map(|stage| &stage.outputs)
    }
}

#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("pipeline validation failed: {0:?}")]
    Invalid(Vec<PipelineValidationIssue>),
    #[error("host input `{name}` is missing")]
    MissingHostInput { name: String },
    #[error("host input `{name}` is not used by the pipeline")]
    UnexpectedHostInput { name: String },
    #[error("stage `{stage}` failed: {source}")]
    StageExecution {
        stage: String,
        #[source]
        source: EngineError,
    },
}

struct Plan {
    order: Vec<usize>,
    hosts: BTreeSet<String>,
}

const MAX_PIPELINE_STAGES: usize = 1_024;

/// Validate every mapping and input edge before executing any stage.
pub fn validate_pipeline(pipeline: &Pipeline) -> Vec<PipelineValidationIssue> {
    match plan_pipeline(pipeline) {
        Ok(_) => Vec::new(),
        Err(issues) => issues,
    }
}

/// Evaluate a stage DAG entirely in memory. Results are returned only after
/// all stages succeed, so a later failure cannot expose partial output as a
/// successful run. External effects inside a stage's dynamic loader remain the
/// host's responsibility.
pub fn run_pipeline_with_context(
    pipeline: &Pipeline,
    hosts: &BTreeMap<String, Instance>,
    execution: &ExecutionContext<'_>,
) -> Result<PipelineOutputs, PipelineError> {
    run_pipeline_with_stage_contexts(pipeline, hosts, |_| *execution)
}

/// Evaluate a stage DAG with a separate host context for each mapping. The
/// provider is called in dependency order after graph and host validation.
/// This lets stages resolve local dynamic source names independently while
/// callers can still share one clock, parameter set, and top-level path.
pub fn run_pipeline_with_stage_contexts<'a>(
    pipeline: &Pipeline,
    hosts: &BTreeMap<String, Instance>,
    mut context_for_stage: impl FnMut(&str) -> ExecutionContext<'a>,
) -> Result<PipelineOutputs, PipelineError> {
    let plan = plan_pipeline(pipeline).map_err(PipelineError::Invalid)?;
    for name in &plan.hosts {
        if !hosts.contains_key(name) {
            return Err(PipelineError::MissingHostInput { name: name.clone() });
        }
    }
    for name in hosts.keys() {
        if !plan.hosts.contains(name) {
            return Err(PipelineError::UnexpectedHostInput { name: name.clone() });
        }
    }

    let mut completed = BTreeMap::new();
    for &index in &plan.order {
        let stage = &pipeline.stages[index];
        let execution = context_for_stage(&stage.id);
        let source = input_value(&stage.source, hosts, &completed);
        let extras = stage
            .extra_sources
            .iter()
            .map(|binding| {
                (
                    binding.name.clone(),
                    input_value(&binding.from, hosts, &completed).clone(),
                )
            })
            .collect();
        let outputs =
            run_outputs_with_sources_and_context(&stage.project, source, extras, &execution)
                .map_err(|source| PipelineError::StageExecution {
                    stage: stage.id.clone(),
                    source,
                })?;
        completed.insert(stage.id.as_str(), outputs);
    }
    let stages = plan
        .order
        .into_iter()
        .map(|index| {
            let id = pipeline.stages[index].id.clone();
            PipelineStageOutput {
                outputs: completed
                    .remove(id.as_str())
                    .expect("completed pipeline stage"),
                id,
            }
        })
        .collect();
    Ok(PipelineOutputs { stages })
}

/// The usual hostless run contract, for mappings that do not read clock,
/// path, parameter, or dynamic-source values.
pub fn run_pipeline(
    pipeline: &Pipeline,
    hosts: &BTreeMap<String, Instance>,
) -> Result<PipelineOutputs, PipelineError> {
    run_pipeline_with_context(
        pipeline,
        hosts,
        &ExecutionContext::new(std::path::Path::new(".")),
    )
}

fn input_value<'a>(
    input: &PipelineInput,
    hosts: &'a BTreeMap<String, Instance>,
    completed: &'a BTreeMap<&str, ExecutionOutputs>,
) -> &'a Instance {
    match input {
        PipelineInput::Host { name } => &hosts[name],
        PipelineInput::StageTarget { stage, target } => {
            let outputs = &completed[stage.as_str()];
            match target {
                None => &outputs.primary,
                Some(name) => {
                    &outputs
                        .extras
                        .iter()
                        .find(|output| output.name == *name)
                        .expect("pipeline plan resolved named target")
                        .instance
                }
            }
        }
    }
}

fn plan_pipeline(pipeline: &Pipeline) -> Result<Plan, Vec<PipelineValidationIssue>> {
    let mut issues = Vec::new();
    if pipeline.stages.is_empty() {
        issues.push(issue(None, "pipeline has no stages"));
    }
    if pipeline.stages.len() > MAX_PIPELINE_STAGES {
        issues.push(issue(
            None,
            format!("pipeline exceeds {MAX_PIPELINE_STAGES} stages"),
        ));
    }

    let mut ids = BTreeMap::new();
    for (index, stage) in pipeline.stages.iter().enumerate() {
        if stage.id.is_empty() || stage.id.contains('\0') || stage.id.len() > 256 {
            issues.push(issue(
                Some(stage.id.clone()),
                "stage ID must be nonempty, at most 256 bytes, and contain no NUL",
            ));
        }
        if ids.insert(stage.id.as_str(), index).is_some() {
            issues.push(issue(Some(stage.id.clone()), "stage ID is duplicated"));
        }
        issues.extend(crate::validate(&stage.project).into_iter().map(|finding| {
            issue(
                Some(stage.id.clone()),
                format!("{}: {}", finding.location, finding.message),
            )
        }));
    }

    let mut builder = PlanBuilder {
        pipeline,
        ids,
        dependencies: vec![BTreeSet::new(); pipeline.stages.len()],
        hosts: BTreeMap::new(),
        issues,
    };
    for (index, stage) in pipeline.stages.iter().enumerate() {
        builder.check_input(index, &stage.source, &stage.project.source);
        let declared: BTreeMap<_, _> = stage
            .project
            .extra_sources
            .iter()
            .map(|source| (source.name.as_str(), source))
            .collect();
        let mut bound = BTreeSet::new();
        for binding in &stage.extra_sources {
            if !bound.insert(binding.name.as_str()) {
                builder.issues.push(issue(
                    Some(stage.id.clone()),
                    format!("named source `{}` is bound more than once", binding.name),
                ));
            }
            let Some(source) = declared.get(binding.name.as_str()) else {
                builder.issues.push(issue(
                    Some(stage.id.clone()),
                    format!("named source `{}` is not declared", binding.name),
                ));
                continue;
            };
            if source.dynamic_path.is_some() {
                builder.issues.push(issue(
                    Some(stage.id.clone()),
                    format!(
                        "dynamic named source `{}` cannot have a static pipeline binding",
                        binding.name
                    ),
                ));
                continue;
            }
            builder.check_input(index, &binding.from, &source.schema);
        }
        for source in &stage.project.extra_sources {
            if source.dynamic_path.is_none() && !bound.contains(source.name.as_str()) {
                builder.issues.push(issue(
                    Some(stage.id.clone()),
                    format!(
                        "static named source `{}` has no pipeline binding",
                        source.name
                    ),
                ));
            }
        }
    }

    let mut remaining = builder.dependencies;
    let mut ready: BTreeSet<_> = remaining
        .iter()
        .enumerate()
        .filter(|(_, deps)| deps.is_empty())
        .map(|(index, _)| index)
        .collect();
    let mut order = Vec::with_capacity(pipeline.stages.len());
    while let Some(index) = ready.pop_first() {
        order.push(index);
        for (dependent, deps) in remaining.iter_mut().enumerate() {
            if deps.remove(&index) && deps.is_empty() {
                ready.insert(dependent);
            }
        }
    }
    if order.len() != pipeline.stages.len() {
        builder
            .issues
            .push(issue(None, "pipeline stage dependencies contain a cycle"));
    }
    if builder.issues.is_empty() {
        Ok(Plan {
            order,
            hosts: builder
                .hosts
                .keys()
                .map(|name| (*name).to_string())
                .collect(),
        })
    } else {
        Err(builder.issues)
    }
}

struct PlanBuilder<'a> {
    pipeline: &'a Pipeline,
    ids: BTreeMap<&'a str, usize>,
    dependencies: Vec<BTreeSet<usize>>,
    hosts: BTreeMap<&'a str, &'a SchemaNode>,
    issues: Vec<PipelineValidationIssue>,
}

impl<'a> PlanBuilder<'a> {
    fn check_input(
        &mut self,
        stage_index: usize,
        input: &'a PipelineInput,
        expected: &'a SchemaNode,
    ) {
        let stage = &self.pipeline.stages[stage_index];
        match input {
            PipelineInput::Host { name } => {
                if name.is_empty() || name.contains('\0') {
                    self.issues.push(issue(
                        Some(stage.id.clone()),
                        "host input name must be nonempty and contain no NUL",
                    ));
                }
                if let Some(previous) = self.hosts.insert(name, expected)
                    && !boundary_compatible(previous, expected)
                {
                    self.issues.push(issue(
                        Some(stage.id.clone()),
                        format!("host input `{name}` is used with incompatible schemas"),
                    ));
                }
            }
            PipelineInput::StageTarget {
                stage: from,
                target,
            } => {
                let Some(&producer_index) = self.ids.get(from.as_str()) else {
                    self.issues.push(issue(
                        Some(stage.id.clone()),
                        format!("input refers to unknown stage `{from}`"),
                    ));
                    return;
                };
                self.dependencies[stage_index].insert(producer_index);
                let producer = &self.pipeline.stages[producer_index].project;
                let actual = match target {
                    None => Some(&producer.target),
                    Some(name) => producer
                        .extra_targets
                        .iter()
                        .find(|target| target.name == *name)
                        .map(|target| &target.schema),
                };
                let Some(actual) = actual else {
                    self.issues.push(issue(
                        Some(stage.id.clone()),
                        format!(
                            "stage `{from}` has no named target `{}`",
                            target.as_deref().unwrap_or_default()
                        ),
                    ));
                    return;
                };
                if !boundary_compatible(actual, expected) {
                    self.issues.push(issue(
                        Some(stage.id.clone()),
                        format!(
                            "stage `{from}` target schema does not match the bound input schema"
                        ),
                    ));
                }
            }
        }
    }
}

/// Instance trees do not carry their root schema name. A stage may therefore
/// pass an otherwise identical value to a differently named input boundary.
fn boundary_compatible(actual: &SchemaNode, expected: &SchemaNode) -> bool {
    if actual == expected {
        return true;
    }
    let mut renamed = actual.clone();
    renamed.name.clone_from(&expected.name);
    renamed == *expected
}

fn issue(stage: Option<String>, message: impl Into<String>) -> PipelineValidationIssue {
    PipelineValidationIssue {
        stage,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::{ScalarType, Value};
    use mapping::{
        Binding, Graph, NamedSource, NamedTarget, Node, PipelineNamedInput, PipelineStage, Scope,
        ScopeConstruction,
    };

    fn schema(ty: ScalarType) -> SchemaNode {
        SchemaNode::group("Record", vec![SchemaNode::scalar("Value", ty)])
    }

    fn input(value: &str) -> Instance {
        Instance::Group(vec![(
            "Value".into(),
            Instance::Scalar(Value::String(value.into())),
        )])
    }

    fn copy_project(ty: ScalarType) -> mapping::Project {
        let schema = schema(ty);
        mapping::Project {
            source: schema.clone(),
            target: schema,
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            extra_targets: Vec::new(),
            failure_rules: Vec::new(),
            user_functions: Default::default(),
            graph: Graph::default(),
            root: Scope {
                construction: ScopeConstruction::CopyCurrentSource,
                ..Scope::default()
            },
        }
    }

    fn host(name: &str) -> PipelineInput {
        PipelineInput::Host { name: name.into() }
    }

    fn output(stage: &str, target: Option<&str>) -> PipelineInput {
        PipelineInput::StageTarget {
            stage: stage.into(),
            target: target.map(str::to_string),
        }
    }

    fn stage(id: &str, project: mapping::Project, source: PipelineInput) -> PipelineStage {
        PipelineStage {
            id: id.into(),
            project,
            source,
            extra_sources: Vec::new(),
        }
    }

    #[test]
    fn forward_references_and_named_output_bindings_run_in_dependency_order() {
        let mut producer = copy_project(ScalarType::String);
        producer.graph.nodes.insert(
            0,
            Node::Const {
                value: Value::String("named value".into()),
            },
        );
        producer.extra_targets.push(NamedTarget {
            name: "shadow".into(),
            path: None,
            schema: schema(ScalarType::String),
            options: Default::default(),
            root: Scope {
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 0,
                }],
                ..Scope::default()
            },
        });

        let mut consumer = copy_project(ScalarType::String);
        consumer.root = Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            ..Scope::default()
        };
        consumer.graph.nodes.insert(
            0,
            Node::SourceField {
                path: vec!["lookup".into(), "Value".into()],
                frame: None,
            },
        );
        consumer.extra_sources.push(NamedSource {
            name: "lookup".into(),
            path: String::new(),
            schema: schema(ScalarType::String),
            options: Default::default(),
            dynamic_path: None,
        });

        // The consumer is deliberately declared before its producer.
        let pipeline = Pipeline {
            stages: vec![
                PipelineStage {
                    extra_sources: vec![PipelineNamedInput {
                        name: "lookup".into(),
                        from: output("producer", Some("shadow")),
                    }],
                    ..stage("consumer", consumer, output("producer", None))
                },
                stage("producer", producer, host("main")),
            ],
        };
        assert!(validate_pipeline(&pipeline).is_empty());
        let hosts = BTreeMap::from([("main".to_string(), input("host value"))]);
        let result = run_pipeline(&pipeline, &hosts).unwrap();
        assert_eq!(
            result
                .stages
                .iter()
                .map(|stage| stage.id.as_str())
                .collect::<Vec<_>>(),
            vec!["producer", "consumer"]
        );
        assert_eq!(
            result.stage("producer").unwrap().primary,
            input("host value")
        );
        assert_eq!(
            result.stage("consumer").unwrap().primary,
            input("named value")
        );
    }

    #[test]
    fn dependency_cycles_missing_targets_and_schema_mismatches_are_rejected() {
        let mut pipeline = Pipeline {
            stages: vec![
                stage(
                    "first",
                    copy_project(ScalarType::String),
                    output("second", None),
                ),
                stage(
                    "second",
                    copy_project(ScalarType::String),
                    output("first", None),
                ),
            ],
        };
        assert!(
            validate_pipeline(&pipeline)
                .iter()
                .any(|issue| issue.message.contains("cycle"))
        );

        pipeline.stages[0].source = output("second", Some("missing"));
        assert!(
            validate_pipeline(&pipeline)
                .iter()
                .any(|issue| issue.message.contains("no named target `missing`"))
        );

        pipeline.stages[0].source = output("second", None);
        pipeline.stages[1].source = host("main");
        pipeline.stages[1].project = copy_project(ScalarType::Int);
        assert!(
            validate_pipeline(&pipeline)
                .iter()
                .any(|issue| issue.message.contains("schema does not match"))
        );
    }

    #[test]
    fn identical_in_memory_shapes_can_connect_across_different_root_names() {
        let producer = copy_project(ScalarType::String);
        let mut consumer = copy_project(ScalarType::String);
        consumer.source.name = "RenamedInput".into();
        let pipeline = Pipeline {
            stages: vec![
                stage("producer", producer, host("main")),
                stage("consumer", consumer, output("producer", None)),
            ],
        };
        assert!(validate_pipeline(&pipeline).is_empty());
        let result = run_pipeline(
            &pipeline,
            &BTreeMap::from([("main".to_string(), input("same value"))]),
        )
        .unwrap();
        assert_eq!(
            result.stage("consumer").unwrap().primary,
            input("same value")
        );
    }

    #[test]
    fn host_contract_and_later_stage_failure_do_not_return_partial_success() {
        let mut failing = copy_project(ScalarType::String);
        failing.root = Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            ..Scope::default()
        };
        failing.graph.nodes.insert(
            0,
            Node::RuntimeParameter {
                name: "required".into(),
                ty: ScalarType::String,
            },
        );
        let pipeline = Pipeline {
            stages: vec![
                stage("first", copy_project(ScalarType::String), host("main")),
                stage("failing", failing, output("first", None)),
            ],
        };
        assert!(matches!(
            run_pipeline(&pipeline, &BTreeMap::new()),
            Err(PipelineError::MissingHostInput { name }) if name == "main"
        ));
        let hosts = BTreeMap::from([("main".to_string(), input("host value"))]);
        assert!(matches!(
            run_pipeline(&pipeline, &hosts),
            Err(PipelineError::StageExecution { stage, .. }) if stage == "failing"
        ));
    }
}
