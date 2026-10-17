//! Generation-time legacy eager-input metadata, never a runtime input table.
//! Owned requests are demand-driven and do not make a declaration eager.
//! Existing public emission capability checks remain separate and unchanged.
use crate::{
    Expression, FailureIteration, GeneratedSequence, InnerJoin, IterationPlan, IterationSource,
    Program, ProgramValidationError, TargetConstruction, TargetScope, UserFunctionProgram,
    validate_program, validate_program_with_basic_owned_filters,
};
use crate::{validate_program_with_basic_owned_sort, validate_program_with_basic_owned_windows};
use mapping::{FunctionId, NodeId, OwnedSourceRef};
use std::collections::{BTreeMap, BTreeSet};

/// Availability attached to the complete original declaration inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamedInputAvailability {
    Optional,
    RequiredLegacy,
    /// A dynamic declaration is reserved even when this selection never uses it.
    ReservedUnavailable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NamedInputAdmission<'a> {
    pub declaration_index: usize,
    pub name: &'a str,
    pub availability: NamedInputAvailability,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedInputCatalogue<'a> {
    pub entries: Vec<NamedInputAdmission<'a>>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedTargetInputCatalogue<'a> {
    pub target_index: usize,
    pub name: &'a str,
    pub catalogue: NamedInputCatalogue<'a>,
}
/// A complete plan; generated runtime target selection owns its old error policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedInputAdmissionPlan<'a> {
    pub all_outputs: NamedInputCatalogue<'a>,
    pub primary: NamedInputCatalogue<'a>,
    pub named_targets: Vec<NamedTargetInputCatalogue<'a>>,
}

/// Validate existing structure first, then describe only potential legacy eager
/// requirements. This does not open emission or evaluate/load any source.
/// Names borrow the validated Program unchanged; ordinals never compact.
pub fn legacy_named_input_admission_plan(
    program: &Program,
) -> Result<NamedInputAdmissionPlan<'_>, ProgramValidationError> {
    validate_program(program)?;
    Ok(admission_plan(program))
}

/// Validate the complete explicit Basic filter-only capability, then describe
/// potential legacy eager requirements through the same declaration analyzer.
/// This does not select a target, acquire inputs, or activate an emitter. Names
/// and original ordinals borrow the unmodified Program exactly as in the default API.
pub fn legacy_named_input_admission_plan_with_basic_owned_filters(
    program: &Program,
) -> Result<NamedInputAdmissionPlan<'_>, ProgramValidationError> {
    validate_program_with_basic_owned_filters(program)?;
    Ok(admission_plan(program))
}

/// Validate the explicit Basic sort capability, then describe potential legacy
/// eager requirements through the unchanged complete declaration analyzer.
/// Names and original ordinals borrow the unmodified Program; this API does not
/// select targets, acquire inputs, evaluate keys, or activate an emitter.
pub fn legacy_named_input_admission_plan_with_basic_owned_sort(
    program: &Program,
) -> Result<NamedInputAdmissionPlan<'_>, ProgramValidationError> {
    validate_program_with_basic_owned_sort(program)?;
    Ok(admission_plan(program))
}

/// Validate explicit Basic windows, then use the unchanged declaration analyzer.
/// Window endpoints retain their parent environment and potential dependencies;
/// this companion does not evaluate bounds, select targets or acquire inputs.
pub fn legacy_named_input_admission_plan_with_basic_owned_windows(
    program: &Program,
) -> Result<NamedInputAdmissionPlan<'_>, ProgramValidationError> {
    validate_program_with_basic_owned_windows(program)?;
    Ok(admission_plan(program))
}

/// Plan named-input admission for the explicit primary-row failure companion.
/// Authenticated primary-row collections do not demand a same-name secondary
/// input. Owned reads stay lazy; ordinary iteration and expression aliases keep
/// their existing eager requirements.
pub fn legacy_named_input_admission_plan_with_basic_owned_row_failures(
    program: &Program,
) -> Result<NamedInputAdmissionPlan<'_>, ProgramValidationError> {
    crate::validate_basic_owned_row_failure_emission(program)?;
    Ok(admission_plan_with_primary_row_failures(program, true))
}

fn admission_plan(program: &Program) -> NamedInputAdmissionPlan<'_> {
    admission_plan_with_primary_row_failures(program, false)
}

fn admission_plan_with_primary_row_failures(
    program: &Program,
    primary_row_failures: bool,
) -> NamedInputAdmissionPlan<'_> {
    let primary_required =
        analyze(program, &program.root, Root::Primary, primary_row_failures).required;
    let primary = catalogue(program, &primary_required);
    let mut all_required = primary_required;
    let mut named_targets = Vec::with_capacity(program.extra_targets.len());
    for (index, target) in program.extra_targets.iter().enumerate() {
        let required = analyze(
            program,
            &target.root,
            Root::Target(index),
            primary_row_failures,
        )
        .required;
        all_required.extend(required.iter().copied());
        named_targets.push(NamedTargetInputCatalogue {
            target_index: index,
            name: &target.name,
            catalogue: catalogue(program, &required),
        });
    }
    NamedInputAdmissionPlan {
        all_outputs: catalogue(program, &all_required),
        primary,
        named_targets,
    }
}
fn catalogue<'a>(program: &'a Program, required: &BTreeSet<usize>) -> NamedInputCatalogue<'a> {
    NamedInputCatalogue {
        entries: program
            .extra_sources
            .iter()
            .enumerate()
            .map(|(index, source)| NamedInputAdmission {
                declaration_index: index,
                name: &source.name,
                availability: if source.dynamic.is_some() {
                    NamedInputAvailability::ReservedUnavailable
                } else if required.contains(&index) {
                    NamedInputAvailability::RequiredLegacy
                } else {
                    NamedInputAvailability::Optional
                },
            })
            .collect(),
    }
}

// Eq-only environments are interned separately from comparable visit keys.
// No Ord/Hash/serialization requirement is added to any mapping DTO.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Root {
    Primary,
    Target(usize),
    Failure(usize),
    Dynamic(usize),
    Function(usize),
}
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Child(usize),
    Segment(usize),
    DynamicChild(usize),
    Source(Vec<String>),
    OwnedSource(OwnedSourceRef),
    Generated(NodeId),
    Join(u64),
    Grouped,
    Collection(NodeId, Vec<String>),
    XmlReplacement(NodeId, usize),
    RecursiveItems,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct Environment {
    root: Root,
    steps: Vec<Step>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Namespace {
    Main,
    Function(usize),
}
#[derive(Debug, Clone, Copy)]
struct Pending {
    namespace: Namespace,
    context: usize,
    node: NodeId,
}
struct Analysis<'a> {
    program: &'a Program,
    primary_row_failures: bool,
    main: BTreeMap<NodeId, &'a Expression>,
    functions: Vec<&'a UserFunctionProgram>,
    function_index: BTreeMap<FunctionId, usize>,
    function_expressions: Vec<BTreeMap<NodeId, &'a Expression>>,
    environments: Vec<Environment>,
    required: BTreeSet<usize>,
    inspected_dynamic: BTreeSet<usize>,
    pending: Vec<Pending>,
    visited: BTreeSet<(Namespace, usize, NodeId)>,
}
impl<'a> Analysis<'a> {
    fn new(program: &'a Program, primary_row_failures: bool) -> Self {
        Self {
            program,
            primary_row_failures,
            main: program
                .expressions
                .iter()
                .map(|n| (n.id, &n.expression))
                .collect(),
            functions: program.user_functions.iter().collect(),
            function_index: program
                .user_functions
                .iter()
                .enumerate()
                .map(|(i, f)| (f.id, i))
                .collect(),
            function_expressions: program
                .user_functions
                .iter()
                .map(|f| {
                    f.expressions
                        .iter()
                        .map(|n| (n.id, &n.expression))
                        .collect()
                })
                .collect(),
            environments: vec![],
            required: BTreeSet::new(),
            inspected_dynamic: BTreeSet::new(),
            pending: vec![],
            visited: BTreeSet::new(),
        }
    }
    fn root(&mut self, root: Root) -> usize {
        self.intern(Environment {
            root,
            steps: vec![],
        })
    }
    fn child(&mut self, parent: usize, step: Step) -> usize {
        let mut environment = self.environments[parent].clone();
        environment.steps.push(step);
        self.intern(environment)
    }
    fn intern(&mut self, environment: Environment) -> usize {
        if let Some(index) = self.environments.iter().position(|old| *old == environment) {
            return index;
        }
        let index = self.environments.len();
        self.environments.push(environment);
        index
    }
    fn queue(&mut self, namespace: Namespace, context: usize, node: NodeId) {
        self.pending.push(Pending {
            namespace,
            context,
            node,
        });
    }
    fn optional(&mut self, namespace: Namespace, context: usize, node: Option<NodeId>) {
        if let Some(node) = node {
            self.queue(namespace, context, node);
        }
    }
    fn expression(&self, namespace: Namespace, node: NodeId) -> Option<&'a Expression> {
        match namespace {
            Namespace::Main => self.main.get(&node).copied(),
            Namespace::Function(index) => self.function_expressions[index].get(&node).copied(),
        }
    }
    fn function(&mut self, function: FunctionId) {
        if let Some(index) = self.function_index.get(&function).copied() {
            // User functions are isolated, never inheriting caller source frames.
            let context = self.root(Root::Function(index));
            self.queue(
                Namespace::Function(index),
                context,
                self.functions[index].output,
            );
        }
    }
    fn path(&mut self, path: &[String]) {
        let Some(first) = path.first() else {
            return;
        };
        let program = self.program;
        let indices = program
            .extra_sources
            .iter()
            .enumerate()
            .filter(|(_, source)| source.name == *first)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        // Match the native helper's conservative alias contract: a primary
        // schema field with this spelling does not remove the eager alias.
        for index in indices {
            if let Some(dynamic) = program.extra_sources[index].dynamic.as_ref() {
                if !self.inspected_dynamic.insert(index) {
                    continue;
                }
                let driver = dynamic.driver.path();
                self.path(driver);
                // One fixed per-declaration environment. D->E->D therefore
                // revisits a finite context rather than extending caller ancestry.
                let root = self.root(Root::Dynamic(index));
                let context = self.child(root, Step::Source(driver.to_vec()));
                self.queue(Namespace::Main, context, dynamic.path);
            } else {
                self.required.insert(index);
            }
        }
    }
    fn sequence(
        &mut self,
        namespace: Namespace,
        parent: usize,
        sequence: &GeneratedSequence,
    ) -> usize {
        for input in sequence.inputs() {
            self.queue(namespace, parent, input);
        }
        if let GeneratedSequence::RecursiveCollect { collection, .. } = sequence {
            self.path(collection);
        }
        for function in sequence.stage_functions() {
            self.function(function);
        }
        // Item IDs are declarations of permissions, not unconditional reads.
        self.child(parent, Step::Generated(sequence.item()))
    }
    fn join(&mut self, parent: usize, join: &InnerJoin) -> usize {
        for source in join.plan().sources() {
            self.path(source.collection());
        }
        self.child(parent, Step::Join(join.id().get()))
    }
    fn iteration(&mut self, parent: usize, iteration: &IterationPlan) -> usize {
        let item = match iteration.input() {
            IterationSource::OwnedSource(source) => {
                self.child(parent, Step::OwnedSource(source.source().clone()))
            }
            IterationSource::Source(source) => {
                self.path(source.path());
                self.child(parent, Step::Source(source.path().to_vec()))
            }
            IterationSource::DynamicDocuments(source) => {
                self.path(source.source().path());
                let item = self.child(parent, Step::Source(source.source().path().to_vec()));
                self.queue(Namespace::Main, item, source.output_path());
                item
            }
            IterationSource::Generated(sequence) => {
                self.sequence(Namespace::Main, parent, sequence)
            }
            IterationSource::InnerJoin(join) => self.join(parent, join),
            IterationSource::Concatenate(segments) => {
                for (index, segment) in segments.iter().enumerate() {
                    let context = self.child(parent, Step::Segment(index));
                    self.scope(segment, context);
                }
                parent
            }
        };
        self.optional(Namespace::Main, item, iteration.filter());
        if let Some(sort) = iteration.sort() {
            for key in sort.keys() {
                self.queue(Namespace::Main, item, key.expression);
            }
        }
        for window in iteration.windows() {
            for node in window.nodes() {
                self.queue(Namespace::Main, parent, node);
            }
        }
        let current = if let Some(grouping) = iteration.grouping() {
            self.optional(Namespace::Main, parent, grouping.parent_expression());
            self.optional(Namespace::Main, item, grouping.item_expression());
            self.child(item, Step::Grouped)
        } else {
            item
        };
        self.optional(Namespace::Main, current, iteration.post_group_filter());
        current
    }
    fn scope(&mut self, scope: &TargetScope, parent: usize) {
        let current = scope
            .iteration
            .as_ref()
            .map_or(parent, |iteration| self.iteration(parent, iteration));
        for binding in &scope.bindings {
            self.queue(Namespace::Main, current, binding.expression);
        }
        match &scope.construction {
            TargetConstruction::Scalar { expression, .. } => {
                self.queue(Namespace::Main, current, *expression)
            }
            TargetConstruction::DynamicGroup {
                bindings, children, ..
            } => {
                for binding in bindings {
                    self.queue(Namespace::Main, current, binding.key);
                    self.queue(Namespace::Main, current, binding.value);
                }
                for (index, child) in children.iter().enumerate() {
                    self.queue(Namespace::Main, current, child.key);
                    let context = self.child(current, Step::DynamicChild(index));
                    self.scope(&child.scope, context);
                }
            }
            TargetConstruction::RecursiveFilter { predicate, .. } => {
                let item = self.child(current, Step::RecursiveItems);
                self.queue(Namespace::Main, item, *predicate);
            }
            TargetConstruction::PathHierarchy { collection, .. } => self.path(collection),
            TargetConstruction::AdjacencyTree {
                collection, root, ..
            } => {
                self.path(collection);
                self.optional(Namespace::Main, current, *root);
            }
            TargetConstruction::Group
            | TargetConstruction::CopyCurrentSource
            | TargetConstruction::XmlMixedContent { .. } => {}
        }
        for (index, child) in scope.children.iter().enumerate() {
            let context = self.child(current, Step::Child(index));
            self.scope(child, context);
        }
    }
    fn failures(&mut self) {
        let program = self.program;
        for (index, rule) in program.failure_rules.iter().enumerate() {
            let parent = self.root(Root::Failure(index));
            let current = match &rule.iteration {
                FailureIteration::Source(source) => {
                    // Only this companion's authenticated Primary row route
                    // cannot select a named collection alias. Ordinary rules
                    // and all legacy expression dependencies remain eager.
                    if !self.primary_row_failures
                        || !crate::is_basic_owned_primary_row_failure(program, rule)
                    {
                        self.path(source.path());
                    }
                    self.child(parent, Step::Source(source.path().to_vec()))
                }
                FailureIteration::Generated(sequence) => {
                    self.sequence(Namespace::Main, parent, sequence)
                }
            };
            self.optional(Namespace::Main, current, rule.selection.predicate());
            self.optional(Namespace::Main, current, rule.message);
        }
    }
    fn run(&mut self) {
        while let Some(Pending {
            namespace,
            context,
            node,
        }) = self.pending.pop()
        {
            if !self.visited.insert((namespace, context, node)) {
                continue;
            }
            let Some(expression) = self.expression(namespace, node) else {
                continue;
            };
            match expression {
                Expression::OwnedSourceField(_) | Expression::OwnedPosition(_) => {}
                Expression::SourceField { path, frame }
                | Expression::DynamicSourceField {
                    object: path,
                    frame,
                    ..
                }
                | Expression::XmlSerialize { path, frame, .. } => {
                    self.path(frame.as_deref().unwrap_or(path));
                    if let Expression::DynamicSourceField { key, .. } = expression {
                        self.queue(namespace, context, *key);
                    }
                }
                Expression::Position { collection } | Expression::JoinField { collection, .. } => {
                    self.path(collection)
                }
                Expression::XmlMixedContent {
                    path,
                    frame,
                    replacements,
                } => {
                    self.path(frame.as_deref().unwrap_or(path));
                    for (index, replacement) in replacements.iter().enumerate() {
                        self.path(&replacement.collection);
                        let item = if replacement.collection.is_empty() {
                            context
                        } else {
                            self.child(context, Step::XmlReplacement(node, index))
                        };
                        self.queue(namespace, item, replacement.expression);
                    }
                }
                Expression::RuntimeParameterDefault { default, .. } => {
                    self.queue(namespace, context, *default)
                }
                Expression::Call { args, .. } | Expression::UserFunctionCall { args, .. } => {
                    for argument in args {
                        self.queue(namespace, context, *argument);
                    }
                    if let Expression::UserFunctionCall { function, .. } = expression {
                        self.function(*function);
                    }
                }
                Expression::DelimitedTextField { input, .. }
                | Expression::ValueMap { input, .. } => self.queue(namespace, context, *input),
                Expression::If {
                    condition,
                    then,
                    else_,
                } => {
                    // Potential legacy eager requirements preserve both branches.
                    self.queue(namespace, context, *condition);
                    self.queue(namespace, context, *then);
                    self.queue(namespace, context, *else_);
                }
                Expression::Raise { message } => self.optional(namespace, context, *message),
                Expression::Lookup {
                    collection,
                    matches,
                    ..
                } => {
                    self.path(collection);
                    self.queue(namespace, context, *matches);
                }
                Expression::CollectionFind {
                    collection,
                    predicate,
                    value,
                } => {
                    self.path(collection);
                    let item = self.child(context, Step::Collection(node, collection.clone()));
                    self.queue(namespace, item, *predicate);
                    self.queue(namespace, item, *value);
                }
                Expression::Aggregate {
                    collection,
                    value,
                    arg,
                    ..
                } => {
                    self.path(collection);
                    let item = self.child(context, Step::Collection(node, collection.clone()));
                    self.optional(namespace, item, value.expression());
                    self.optional(namespace, context, *arg);
                }
                Expression::JoinAggregate {
                    join,
                    expression,
                    arg,
                    ..
                } => {
                    let item = self.join(context, join);
                    self.optional(namespace, item, *expression);
                    self.optional(namespace, context, *arg);
                }
                Expression::SequenceExists {
                    sequence,
                    predicate,
                } => {
                    let item = self.sequence(namespace, context, sequence);
                    self.queue(namespace, item, *predicate);
                }
                Expression::SequenceItemAt { sequence, index } => {
                    self.sequence(namespace, context, sequence);
                    self.queue(namespace, context, *index);
                }
                Expression::SequenceAggregate {
                    sequence,
                    predicate,
                    expression,
                    arg,
                    ..
                } => {
                    let item = self.sequence(namespace, context, sequence);
                    self.optional(namespace, item, *predicate);
                    self.optional(namespace, item, *expression);
                    self.optional(namespace, context, *arg);
                }
                Expression::SourceRootXmlTypeEquals { .. }
                | Expression::SourceRootField { .. }
                | Expression::SourceDocumentPath
                | Expression::JoinPosition { .. }
                | Expression::Const { .. }
                | Expression::FunctionParameter { .. }
                | Expression::RuntimeValue { .. }
                | Expression::RuntimeParameter { .. } => {}
            }
        }
    }
}
fn analyze<'a>(
    program: &'a Program,
    scope: &TargetScope,
    root: Root,
    primary_row_failures: bool,
) -> Analysis<'a> {
    let mut analysis = Analysis::new(program, primary_row_failures);
    let context = analysis.root(root);
    analysis.scope(scope, context);
    analysis.failures();
    analysis.run();
    analysis
}
#[cfg(test)]
pub(crate) fn primary_visits_for_test(
    program: &Program,
) -> Result<Vec<(Option<FunctionId>, usize, NodeId)>, ProgramValidationError> {
    validate_program(program)?;
    let analysis = analyze(program, &program.root, Root::Primary, false);
    eprintln!(
        "Complete admission trace input={program:#?} environments={:#?} visits={:#?} required={:#?} inspected_dynamic={:#?}",
        analysis.environments, analysis.visited, analysis.required, analysis.inspected_dynamic
    );
    Ok(analysis
        .visited
        .iter()
        .map(|(namespace, context, node)| {
            (
                match namespace {
                    Namespace::Main => None,
                    Namespace::Function(index) => Some(analysis.functions[*index].id),
                },
                *context,
                *node,
            )
        })
        .collect())
}

#[cfg(test)]
pub(crate) fn primary_visits_for_test_with_basic_owned_windows(
    program: &Program,
) -> Result<Vec<(Option<FunctionId>, usize, NodeId)>, ProgramValidationError> {
    validate_program_with_basic_owned_windows(program)?;
    let analysis = analyze(program, &program.root, Root::Primary, false);
    eprintln!(
        "Complete admission trace input={program:#?} environments={:#?} visits={:#?} required={:#?} inspected_dynamic={:#?}",
        analysis.environments, analysis.visited, analysis.required, analysis.inspected_dynamic
    );
    Ok(analysis
        .visited
        .iter()
        .map(|(namespace, context, node)| {
            (
                match namespace {
                    Namespace::Main => None,
                    Namespace::Function(index) => Some(analysis.functions[*index].id),
                },
                *context,
                *node,
            )
        })
        .collect())
}
