//! Exact ownership structure and separately closed generated capability.
use super::{
    OwnedSourceCapabilityOrigin as Origin, OwnedSourceCapabilityReason as Capability,
    OwnedSourceDiagnostic as Diagnostic, OwnedSourceDiagnosticLocation as Location,
    OwnedSourceDiagnosticOwner as Owner, OwnedSourceDiagnosticRequest as Request,
    OwnedSourceNamespace as Namespace, OwnedSourceSchemaShape as Shape,
    OwnedSourceStructureReason as Structure, ProgramValidationError, graph_dependencies,
};
use crate::{
    Expression, FailureIteration, IterationOutput, IterationSource, Program, TargetConstruction,
    TargetScope,
};
use ir::{SchemaKind, SchemaNode};
use mapping::{NodeId, SourceScopeLocation, SourceScopeStep, SourceScopeTarget};

mod failure_sources;
pub(super) use failure_sources::FailurePolicy;
pub use failure_sources::validate_basic_owned_failure_source_emission;
mod primary_row_failures;
pub use primary_row_failures::{
    is_basic_owned_primary_row_failure, validate_basic_owned_row_failure_emission,
};

fn request(expression: &Expression) -> Option<Request> {
    match expression {
        Expression::OwnedSourceField(field) => Some(Request::Field {
            owner: (&field.source.owner).into(),
            path: field.source.path.clone(),
            frame: field.frame.clone(),
        }),
        Expression::OwnedPosition(position) => Some(Request::Position {
            owner: (&position.owner).into(),
            collection: position.collection.clone(),
        }),
        _ => None,
    }
}
fn iteration_request(source: &mapping::OwnedSourceRef) -> Request {
    Request::Iteration {
        owner: (&source.owner).into(),
        path: source.path.clone(),
    }
}
fn shape(node: &SchemaNode) -> Shape {
    match node.kind {
        SchemaKind::Scalar { .. } => Shape::Scalar {
            repeating: node.repeating,
        },
        SchemaKind::ScalarUnion { .. } => Shape::ScalarUnion {
            repeating: node.repeating,
        },
        SchemaKind::Group { .. } => Shape::Group {
            repeating: node.repeating,
        },
    }
}
fn endpoint<'a>(program: &'a Program, owner: &Owner) -> Result<&'a SchemaNode, Structure> {
    match owner {
        Owner::Primary => Ok(&program.source),
        Owner::Named { name } if name.is_empty() => Err(Structure::InvalidNamedOwner),
        Owner::Named { name } => program
            .extra_sources
            .iter()
            .find(|source| source.name == *name)
            .map(|source| &source.source)
            .ok_or(Structure::UnknownOwner),
    }
}
fn anchor<'a>(node: &'a SchemaNode, name: &str) -> Option<&'a SchemaNode> {
    if node.recursive_ref.is_none()
        && node.name == name
        && matches!(node.kind, SchemaKind::Group { .. })
    {
        return Some(node);
    }
    match &node.kind {
        SchemaKind::Group { children, .. } => children.iter().find_map(|child| anchor(child, name)),
        _ => None,
    }
}
fn resolved<'a>(root: &'a SchemaNode, node: &'a SchemaNode) -> Result<&'a SchemaNode, Structure> {
    match &node.recursive_ref {
        None => Ok(node),
        Some(name) => anchor(root, name).ok_or_else(|| Structure::UnresolvedRecursiveAnchor {
            anchor: name.clone(),
        }),
    }
}
fn follow<'a>(root: &'a SchemaNode, path: &[String]) -> Result<&'a SchemaNode, Structure> {
    let mut occurrence = root;
    for (segment, name) in path.iter().enumerate() {
        occurrence = resolved(root, occurrence)?
            .child(name)
            .ok_or(Structure::MissingOwnedPath { segment })?;
    }
    Ok(occurrence)
}
fn collection(root: &SchemaNode, path: &[String]) -> Result<(), Structure> {
    let occurrence = follow(root, path)?;
    // Resolving child shape never substitutes the selected occurrence metadata.
    let terminal_shape = resolved(root, occurrence)?;
    if occurrence.repeating
        || path.is_empty() && matches!(terminal_shape.kind, SchemaKind::Group { .. })
    {
        return Ok(());
    }
    Err(Structure::InvalidOwnedCollection {
        found: shape(occurrence),
    })
}
fn check_request(program: &Program, query: &Request) -> Result<(), Structure> {
    match query {
        Request::Field { owner, path, frame } => {
            let root = endpoint(program, owner)?;
            if let Some(frame) = frame {
                if !path.starts_with(frame) {
                    return Err(Structure::IncompatibleFramePath);
                }
                collection(root, frame)?;
            }
            let occurrence = follow(root, path)?;
            if matches!(
                resolved(root, occurrence)?.kind,
                SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. }
            ) {
                Ok(())
            } else {
                Err(Structure::InvalidOwnedField {
                    found: shape(occurrence),
                })
            }
        }
        Request::Position {
            owner,
            collection: path,
        } => {
            let root = endpoint(program, owner)?;
            if let Some(path) = path {
                collection(root, path)?;
            }
            Ok(())
        }
        Request::Iteration { owner, path } => collection(endpoint(program, owner)?, path),
    }
}
fn structure(
    program: &Program,
    location: Location,
    query: Request,
) -> Result<(), ProgramValidationError> {
    check_request(program, &query).map_err(|reason| ProgramValidationError::OwnedSource {
        diagnostic: Diagnostic::Structure {
            location,
            request: query,
            reason,
        }
        .into(),
    })
}
fn owned_path_hierarchy(scope: &TargetScope) -> Option<&mapping::OwnedSourceRef> {
    match &scope.construction {
        TargetConstruction::OwnedPathHierarchy { collection, .. } => Some(collection),
        _ => None,
    }
}
fn scope_has_owned(scope: &TargetScope) -> bool {
    owned_path_hierarchy(scope).is_some()
        || scope.iteration.as_ref().is_some_and(|iteration| {
            iteration.owned_source_iteration().is_some()
                || iteration
                    .concatenated()
                    .is_some_and(|segments| segments.iter().any(scope_has_owned))
        })
        || scope.children.iter().any(scope_has_owned)
        || matches!(&scope.construction, TargetConstruction::DynamicGroup { children, .. } if children.iter().any(|child| scope_has_owned(&child.scope)))
}
fn has_owned(program: &Program) -> bool {
    program.expressions.iter().any(|node| {
        matches!(
            node.expression,
            Expression::OwnedSourceField(_) | Expression::OwnedPosition(_)
        )
    }) || program.user_functions.iter().any(|function| {
        function.expressions.iter().any(|node| {
            matches!(
                node.expression,
                Expression::OwnedSourceField(_) | Expression::OwnedPosition(_)
            )
        })
    }) || scope_has_owned(&program.root)
        || program
            .extra_targets
            .iter()
            .any(|target| scope_has_owned(&target.root))
        || program
            .failure_rules
            .iter()
            .any(|rule| matches!(rule.iteration, FailureIteration::OwnedSource(_)))
}
fn visit_scopes<'a>(
    scope: &'a TargetScope,
    route: SourceScopeLocation,
    output: &mut Vec<(&'a TargetScope, SourceScopeLocation)>,
) {
    output.push((scope, route.clone()));
    if let Some(segments) = scope
        .iteration
        .as_ref()
        .and_then(|iteration| iteration.concatenated())
    {
        for (index, segment) in segments.iter().enumerate() {
            visit_scopes(
                segment,
                route.descendant(SourceScopeStep::Segment(index)),
                output,
            );
        }
    }
    for (index, child) in scope.children.iter().enumerate() {
        visit_scopes(
            child,
            route.descendant(SourceScopeStep::Child(index)),
            output,
        );
    }
    if let TargetConstruction::DynamicGroup { children, .. } = &scope.construction {
        for (index, child) in children.iter().enumerate() {
            visit_scopes(
                &child.scope,
                route.descendant(SourceScopeStep::DynamicChild(index)),
                output,
            );
        }
    }
}
fn all_scopes(program: &Program) -> Vec<(&TargetScope, SourceScopeLocation)> {
    let mut result = Vec::new();
    visit_scopes(
        &program.root,
        SourceScopeLocation::root(SourceScopeTarget::Primary),
        &mut result,
    );
    for (index, target) in program.extra_targets.iter().enumerate() {
        visit_scopes(
            &target.root,
            SourceScopeLocation::root(SourceScopeTarget::Named {
                index,
                name: target.name.clone(),
            }),
            &mut result,
        );
    }
    result
}
pub(super) fn validate_structure(program: &Program) -> Result<(), ProgramValidationError> {
    if !has_owned(program) {
        return Ok(());
    }
    let mut main = program.expressions.iter().collect::<Vec<_>>();
    main.sort_by_key(|node| node.id);
    for node in main {
        if let Some(query) = request(&node.expression) {
            structure(
                program,
                Location::Expression {
                    namespace: Namespace::Main,
                    node: node.id,
                    scope: None,
                },
                query,
            )?;
        }
    }
    for function in &program.user_functions {
        let mut nodes = function.expressions.iter().collect::<Vec<_>>();
        nodes.sort_by_key(|node| node.id);
        for node in nodes {
            if let Some(query) = request(&node.expression) {
                structure(
                    program,
                    Location::Expression {
                        namespace: Namespace::Function(function.id),
                        node: node.id,
                        scope: None,
                    },
                    query,
                )?;
            }
        }
    }
    for (scope, route) in all_scopes(program) {
        if let Some(source) = scope
            .iteration
            .as_ref()
            .and_then(|iteration| iteration.owned_source_iteration())
        {
            structure(
                program,
                Location::Iteration {
                    scope: route.clone(),
                },
                iteration_request(source.source()),
            )?;
        }
        if let Some(collection) = owned_path_hierarchy(scope) {
            structure(
                program,
                Location::Iteration { scope: route },
                iteration_request(collection),
            )?;
        }
    }
    for (index, rule) in program.failure_rules.iter().enumerate() {
        if let FailureIteration::OwnedSource(source) = &rule.iteration {
            structure(
                program,
                Location::FailureIteration { index },
                iteration_request(source.source()),
            )?;
        }
    }
    Ok(())
}
#[derive(Clone, PartialEq, Eq)]
struct Closed {
    reason: Capability,
    origin: Origin,
}
#[derive(Clone, PartialEq, Eq)]
enum Ancestor<'a> {
    Owned {
        scope: SourceScopeLocation,
        source: &'a mapping::OwnedSourceRef,
    },
    Legacy {
        scope: SourceScopeLocation,
        path: &'a [String],
    },
    Specialized {
        scope: SourceScopeLocation,
        input: &'a IterationSource,
    },
    GroupMembers {
        scope: SourceScopeLocation,
    },
}
#[derive(Clone, PartialEq, Eq)]
struct Visit<'a> {
    namespace: Namespace,
    closed: Option<Closed>,
    scope: Option<SourceScopeLocation>,
    ancestry: Vec<Ancestor<'a>>,
    node: NodeId,
}
fn owns(ancestry: &[Ancestor<'_>]) -> bool {
    ancestry.iter().any(|entry| {
        matches!(
            entry,
            Ancestor::Owned { .. } | Ancestor::GroupMembers { .. }
        )
    })
}
fn failure(location: Location, query: Option<Request>, closed: Closed) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Diagnostic::Capability {
            location,
            request: query,
            reason: closed.reason,
            origin: closed.origin,
        }
        .into(),
    }
}
fn dynamic_owner(program: &Program, query: &Request) -> Option<Closed> {
    let owner = match query {
        Request::Field { owner, .. }
        | Request::Position { owner, .. }
        | Request::Iteration { owner, .. } => owner,
    };
    let Owner::Named { name } = owner else {
        return None;
    };
    program
        .extra_sources
        .iter()
        .enumerate()
        .find(|(_, source)| source.name == *name && source.dynamic.is_some())
        .map(|(index, source)| Closed {
            reason: Capability::DynamicOwner,
            origin: Origin::DynamicSource {
                index,
                name: source.name.clone(),
            },
        })
}
/// Internal policy; public callers explicitly select an additive companion.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BasicOwnedProfile {
    ControlFree,
    FilterOnly,
    SortAndFilter,
    WindowsAndSortAndFilter,
    ByAndWindowsAndSortAndFilter,
    ByAndAdjacentByAndWindowsAndSortAndFilter,
    ByAndAdjacentByAndStartingWithAndWindowsAndSortAndFilter,
    ByAndAdjacentByAndStartingWithAndEndingWithAndWindowsAndSortAndFilter,
}

/// Construction permission for an owned path-hierarchy input collection. It is
/// orthogonal to source controls; only its explicit companion opts in.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PathHierarchyPolicy {
    Closed,
    Owned,
}

/// Consumer permission is orthogonal to source controls and failure eligibility.
/// Only explicit scalar-producer companions opt in; source controls do not
/// themselves grant a consumer permission.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SequenceItemAtPolicy {
    Closed,
    BasicScalarProducers,
}

impl SequenceItemAtPolicy {
    fn permits(self, expression: &Expression) -> bool {
        self == Self::BasicScalarProducers
            && matches!(expression, Expression::SequenceItemAt { sequence, .. }
                if matches!(sequence,
                    crate::GeneratedSequence::Range { .. }
                        | crate::GeneratedSequence::Tokenize { .. }
                        | crate::GeneratedSequence::TokenizeByLength { .. }
                        | crate::GeneratedSequence::TokenizeRegex { .. }))
    }
}

impl BasicOwnedProfile {
    fn supports_groups(self) -> bool {
        matches!(
            self,
            Self::ByAndWindowsAndSortAndFilter
                | Self::ByAndAdjacentByAndWindowsAndSortAndFilter
                | Self::ByAndAdjacentByAndStartingWithAndWindowsAndSortAndFilter
                | Self::ByAndAdjacentByAndStartingWithAndEndingWithAndWindowsAndSortAndFilter
        )
    }

    fn permits_filter(self, scope: &TargetScope) -> bool {
        self == Self::FilterOnly
            && matches!(
                scope.construction,
                TargetConstruction::Group | TargetConstruction::Scalar { .. }
            )
            && scope.iteration.as_ref().is_some_and(|iteration| {
                iteration.filter().is_some()
                    && matches!(
                        iteration.input(),
                        IterationSource::Source(_) | IterationSource::OwnedSource(_)
                    )
                    && iteration.sort().is_none()
                    && iteration.grouping().is_none()
                    && iteration.post_group_filter().is_none()
                    && iteration.windows().is_empty()
                    && iteration.output() == IterationOutput::Repeated
            })
            || self == Self::SortAndFilter
                && (Self::FilterOnly.permits_filter(scope) || self.permits_sort(scope))
            || self == Self::WindowsAndSortAndFilter
                && (Self::SortAndFilter.permits_filter(scope)
                    || self.permits_windows(scope)
                        && scope
                            .iteration
                            .as_ref()
                            .is_some_and(|iteration| iteration.filter().is_some()))
            || self.supports_groups()
                && (Self::WindowsAndSortAndFilter.permits_filter(scope)
                    || self.permits_group(scope)
                        && scope
                            .iteration
                            .as_ref()
                            .is_some_and(|iteration| iteration.filter().is_some()))
    }
    fn permits_sort(self, scope: &TargetScope) -> bool {
        self == Self::SortAndFilter
            && matches!(
                scope.construction,
                TargetConstruction::Group | TargetConstruction::Scalar { .. }
            )
            && scope.iteration.as_ref().is_some_and(|iteration| {
                iteration.sort().is_some()
                    && matches!(
                        iteration.input(),
                        IterationSource::Source(_) | IterationSource::OwnedSource(_)
                    )
                    && iteration.grouping().is_none()
                    && iteration.post_group_filter().is_none()
                    && iteration.windows().is_empty()
                    && iteration.output() == IterationOutput::Repeated
            })
            || self == Self::WindowsAndSortAndFilter
                && (Self::SortAndFilter.permits_sort(scope)
                    || self.permits_windows(scope)
                        && scope
                            .iteration
                            .as_ref()
                            .is_some_and(|iteration| iteration.sort().is_some()))
            || self.supports_groups()
                && (Self::WindowsAndSortAndFilter.permits_sort(scope)
                    || self.permits_group(scope)
                        && scope
                            .iteration
                            .as_ref()
                            .is_some_and(|iteration| iteration.sort().is_some()))
    }
    fn permits_windows(self, scope: &TargetScope) -> bool {
        self == Self::WindowsAndSortAndFilter
            && matches!(
                scope.construction,
                TargetConstruction::Group | TargetConstruction::Scalar { .. }
            )
            && scope.iteration.as_ref().is_some_and(|iteration| {
                (!iteration.windows().is_empty() || iteration.output() == IterationOutput::First)
                    && matches!(
                        iteration.input(),
                        IterationSource::Source(_) | IterationSource::OwnedSource(_)
                    )
                    && iteration.grouping().is_none()
                    && iteration.post_group_filter().is_none()
                    && (iteration.output() == IterationOutput::Repeated
                        || iteration.output() == IterationOutput::First
                            && matches!(scope.construction, TargetConstruction::Group))
            })
            || self.supports_groups()
                && (Self::WindowsAndSortAndFilter.permits_windows(scope)
                    || self.permits_group(scope)
                        && scope.iteration.as_ref().is_some_and(|iteration| {
                            !iteration.windows().is_empty()
                                || iteration.output() == IterationOutput::First
                        }))
    }
    fn permits_group(self, scope: &TargetScope) -> bool {
        // Exact mode allowlist, not an ordering of profiles or a generic
        // grouping permission. Only these admitted static scopes may mint
        // GroupMembers ancestry after their candidate controls are checked.
        self.supports_groups()
            && matches!(
                scope.construction,
                TargetConstruction::Group | TargetConstruction::Scalar { .. }
            )
            && scope.iteration.as_ref().is_some_and(|iteration| {
                (match iteration.grouping() {
                    Some(crate::GroupingPlan::By { .. }) => true,
                    Some(crate::GroupingPlan::AdjacentBy { .. }) => matches!(
                        self,
                        Self::ByAndAdjacentByAndWindowsAndSortAndFilter
                            | Self::ByAndAdjacentByAndStartingWithAndWindowsAndSortAndFilter
                            | Self::ByAndAdjacentByAndStartingWithAndEndingWithAndWindowsAndSortAndFilter
                    ),
                    Some(crate::GroupingPlan::StartingWith { .. }) => matches!(
                        self,
                        Self::ByAndAdjacentByAndStartingWithAndWindowsAndSortAndFilter
                            | Self::ByAndAdjacentByAndStartingWithAndEndingWithAndWindowsAndSortAndFilter
                    ),
                    Some(crate::GroupingPlan::EndingWith { .. }) => {
                        self == Self::ByAndAdjacentByAndStartingWithAndEndingWithAndWindowsAndSortAndFilter
                    }
                    Some(crate::GroupingPlan::IntoBlocks { .. }) | None => false,
                }) && matches!(
                    iteration.input(),
                    IterationSource::Source(_) | IterationSource::OwnedSource(_)
                ) && (iteration.output() == IterationOutput::Repeated
                    || iteration.output() == IterationOutput::First
                        && matches!(scope.construction, TargetConstruction::Group))
            })
    }
}
struct Admission<'a> {
    program: &'a Program,
    profile: BasicOwnedProfile,
    sequence_item_at: SequenceItemAtPolicy,
    path_hierarchy: PathHierarchyPolicy,
    visited: Vec<Visit<'a>>,
}
impl<'a> Admission<'a> {
    fn expression(&mut self, start: Visit<'a>) -> Result<(), ProgramValidationError> {
        let mut pending = vec![start];
        while let Some(mut visit) = pending.pop() {
            if self.visited.contains(&visit) {
                continue;
            }
            self.visited.push(visit.clone());
            let expression = match visit.namespace {
                Namespace::Main => self
                    .program
                    .expressions
                    .iter()
                    .find(|node| node.id == visit.node)
                    .map(|node| &node.expression),
                Namespace::Function(function) => self
                    .program
                    .user_functions
                    .iter()
                    .find(|definition| definition.id == function)
                    .and_then(|definition| {
                        definition
                            .expressions
                            .iter()
                            .find(|node| node.id == visit.node)
                    })
                    .map(|node| &node.expression),
            };
            let Some(expression) = expression else {
                continue;
            };
            let location = Location::Expression {
                namespace: visit.namespace,
                node: visit.node,
                scope: visit.scope.clone(),
            };
            if let Some(query) = request(expression)
                && let Some(closed) = visit
                    .closed
                    .clone()
                    .or_else(|| dynamic_owner(self.program, &query))
            {
                return Err(failure(location, Some(query), closed));
            }
            if let Expression::UserFunctionCall { function, .. } = expression {
                if owns(&visit.ancestry) {
                    return Err(failure(
                        location,
                        None,
                        Closed {
                            reason: Capability::IsolatedFunction,
                            origin: Origin::FunctionCall {
                                caller_namespace: visit.namespace,
                                call: visit.node,
                                callee: *function,
                            },
                        },
                    ));
                }
                // An isolated body never inherits the caller's source ancestry.
                if let Some(definition) = self
                    .program
                    .user_functions
                    .iter()
                    .find(|definition| definition.id == *function)
                {
                    pending.push(Visit {
                        namespace: Namespace::Function(*function),
                        closed: Some(Closed {
                            reason: Capability::IsolatedFunction,
                            origin: Origin::FunctionBody {
                                function: *function,
                            },
                        }),
                        scope: None,
                        ancestry: Vec::new(),
                        node: definition.output,
                    });
                }
            }
            let reason = match expression {
                Expression::SequenceItemAt { .. } if self.sequence_item_at.permits(expression) => {
                    None
                }
                Expression::SequenceExists { .. }
                | Expression::SequenceItemAt { .. }
                | Expression::SequenceAggregate { .. } => Some(Capability::GeneratedItem),
                Expression::Aggregate { .. }
                | Expression::Lookup { .. }
                | Expression::CollectionFind { .. } => Some(Capability::AggregateLookup),
                Expression::JoinAggregate { .. }
                | Expression::JoinField { .. }
                | Expression::JoinPosition { .. } => Some(Capability::Join),
                Expression::XmlSerialize { .. }
                | Expression::XmlMixedContent { .. }
                | Expression::SourceRootXmlTypeEquals { .. }
                | Expression::SourceDocumentPath => Some(Capability::XmlOperator),
                _ => None, // Raise keeps its legacy dependency/laziness behavior.
            };
            if let Some(reason) = reason {
                let closed = visit.closed.clone().unwrap_or(Closed {
                    reason,
                    origin: Origin::Expression {
                        namespace: visit.namespace,
                        node: visit.node,
                    },
                });
                if owns(&visit.ancestry) {
                    return Err(failure(location, None, closed));
                }
                visit.closed = Some(closed);
            }
            for dependency in graph_dependencies::of(expression).into_iter().rev() {
                let mut child = visit.clone();
                child.node = dependency;
                pending.push(child);
            }
        }
        Ok(())
    }
    fn scope(
        &mut self,
        scope: &'a TargetScope,
        route: SourceScopeLocation,
        mut ancestry: Vec<Ancestor<'a>>,
        inherited: Option<Closed>,
    ) -> Result<(), ProgramValidationError> {
        // Bounds use the incoming parent, before this scope introduces an item.
        // Keep the complete Visit key when a bound is also a key or target root.
        let window_parent = self
            .profile
            .permits_windows(scope)
            .then(|| (ancestry.clone(), inherited.clone()));
        let mut closed = inherited;
        if let Some(iteration) = &scope.iteration {
            let reason = if (iteration.grouping().is_some()
                || iteration.post_group_filter().is_some())
                && !self.profile.permits_group(scope)
            {
                Some(Capability::Grouping)
            } else if iteration.filter().is_some() && !self.profile.permits_filter(scope)
                || iteration.sort().is_some() && !self.profile.permits_sort(scope)
                || !iteration.windows().is_empty() && !self.profile.permits_windows(scope)
            {
                Some(Capability::ScopeControl)
            } else if iteration.output() != IterationOutput::Repeated
                && !self.profile.permits_windows(scope)
            {
                Some(Capability::Document)
            } else {
                match iteration.input() {
                    IterationSource::DynamicDocuments(_) => Some(Capability::Document),
                    IterationSource::Generated(_) => Some(Capability::GeneratedItem),
                    IterationSource::InnerJoin(_) => Some(Capability::Join),
                    IterationSource::Concatenate(_) => Some(Capability::Concatenate),
                    _ => None,
                }
            };
            if closed.is_none() {
                closed = reason.map(|reason| Closed {
                    reason,
                    origin: Origin::Scope {
                        scope: route.clone(),
                    },
                });
            }
            let ancestor = match iteration.input() {
                IterationSource::OwnedSource(source) => Ancestor::Owned {
                    scope: route.clone(),
                    source: source.source(),
                },
                IterationSource::Source(source) => Ancestor::Legacy {
                    scope: route.clone(),
                    path: source.path(),
                },
                input => Ancestor::Specialized {
                    scope: route.clone(),
                    input,
                },
            };
            ancestry.push(ancestor);
            if let Some(source) = iteration.owned_source_iteration() {
                let query = iteration_request(source.source());
                if let Some(closed) = closed
                    .clone()
                    .or_else(|| dynamic_owner(self.program, &query))
                {
                    return Err(failure(
                        Location::Iteration {
                            scope: route.clone(),
                        },
                        Some(query),
                        closed,
                    ));
                }
            }
        }
        let construction_reason = match scope.construction {
            TargetConstruction::Group => None,
            TargetConstruction::OwnedPathHierarchy { .. }
                if self.path_hierarchy == PathHierarchyPolicy::Owned =>
            {
                None
            }
            TargetConstruction::DynamicGroup { .. } => Some(Capability::DynamicTarget),
            TargetConstruction::XmlMixedContent { .. } => Some(Capability::XmlOperator),
            TargetConstruction::RecursiveFilter { .. }
            | TargetConstruction::PathHierarchy { .. }
            | TargetConstruction::OwnedPathHierarchy { .. }
            | TargetConstruction::AdjacencyTree { .. } => Some(Capability::TreeOperator),
            TargetConstruction::Scalar { .. }
                if self.profile.permits_sort(scope)
                    || self.profile.permits_windows(scope)
                    || self.profile.supports_groups() =>
            {
                None
            }
            TargetConstruction::CopyCurrentSource | TargetConstruction::Scalar { .. } => {
                Some(Capability::SpecialConstruction)
            }
        };
        if closed.is_none() {
            closed = construction_reason.map(|reason| Closed {
                reason,
                origin: Origin::Scope {
                    scope: route.clone(),
                },
            });
        }
        // The hierarchy's own collection is an owned read in the receiving
        // context; any inherited or construction closure refuses it there.
        if let Some(collection) = owned_path_hierarchy(scope) {
            let query = iteration_request(collection);
            if let Some(closed) = closed
                .clone()
                .or_else(|| dynamic_owner(self.program, &query))
            {
                return Err(failure(
                    Location::Iteration {
                        scope: route.clone(),
                    },
                    Some(query),
                    closed,
                ));
            }
        }
        if let Some(closed) = closed.clone().filter(|_| owns(&ancestry)) {
            let query = ancestry.iter().rev().find_map(|entry| match entry {
                Ancestor::Owned { source, .. } => Some(iteration_request(source)),
                _ => None,
            });
            return Err(failure(
                Location::Iteration {
                    scope: route.clone(),
                },
                query,
                closed,
            ));
        }
        if self.profile.permits_group(scope) {
            let iteration = scope
                .iteration
                .as_ref()
                .expect("admitted grouping iteration");
            // Bounds run once in the receiving parent; filter, sort, key or
            // boundary, and retention are candidate expressions. Only target
            // expressions and descendants inherit the grouped/member view.
            if let Some((parent_ancestry, parent_closed)) = &window_parent {
                for node in iteration
                    .windows()
                    .iter()
                    .copied()
                    .flat_map(crate::SequenceWindow::nodes)
                {
                    self.expression(Visit {
                        namespace: Namespace::Main,
                        closed: parent_closed.clone(),
                        scope: Some(route.clone()),
                        ancestry: parent_ancestry.clone(),
                        node,
                    })?;
                }
            }
            for node in iteration
                .filter()
                .into_iter()
                .chain(
                    iteration
                        .sort()
                        .iter()
                        .flat_map(|sort| sort.keys().map(|key| key.expression)),
                )
                .chain(
                    iteration
                        .grouping()
                        .and_then(|grouping| grouping.item_expression()),
                )
                .chain(iteration.post_group_filter())
            {
                self.expression(Visit {
                    namespace: Namespace::Main,
                    closed: closed.clone(),
                    scope: Some(route.clone()),
                    ancestry: ancestry.clone(),
                    node,
                })?;
            }
            // A wholly ordinary island must not gain owned ancestry just by
            // using an admitted mode. A demanded paired By/AdjacentBy/StartingWith/
            // EndingWith operation establishes a member container; no other mode
            // may mint one.
            if owns(&ancestry) || basic_scope_needs_owned(self.program, scope) {
                ancestry.push(Ancestor::GroupMembers {
                    scope: route.clone(),
                });
            }
        }
        let mut roots = scope
            .bindings
            .iter()
            .map(|binding| binding.expression)
            .collect::<Vec<_>>();
        if let Some(iteration) = &scope.iteration
            && !self.profile.permits_group(scope)
        {
            if let Some((parent_ancestry, parent_closed)) = &window_parent {
                for node in iteration
                    .windows()
                    .iter()
                    .copied()
                    .flat_map(crate::SequenceWindow::nodes)
                {
                    self.expression(Visit {
                        namespace: Namespace::Main,
                        closed: parent_closed.clone(),
                        scope: Some(route.clone()),
                        ancestry: parent_ancestry.clone(),
                        node,
                    })?;
                }
                // The admitted window scope is a static Source/OwnedSource with
                // no grouping or dynamic output. Its other roots use the item.
                roots.extend(iteration.filter());
                roots.extend(
                    iteration
                        .sort()
                        .iter()
                        .flat_map(|sort| sort.keys().map(|key| key.expression)),
                );
            } else {
                roots.extend(iteration.roots());
            }
        }
        match &scope.construction {
            TargetConstruction::Scalar { expression, .. } => roots.push(*expression),
            TargetConstruction::RecursiveFilter { predicate, .. } => roots.push(*predicate),
            TargetConstruction::AdjacencyTree {
                root: Some(root), ..
            } => roots.push(*root),
            TargetConstruction::DynamicGroup {
                bindings, children, ..
            } => {
                for binding in bindings {
                    roots.extend([binding.key, binding.value]);
                }
                for child in children {
                    roots.push(child.key);
                }
            }
            _ => {}
        }
        for node in roots {
            self.expression(Visit {
                namespace: Namespace::Main,
                closed: closed.clone(),
                scope: Some(route.clone()),
                ancestry: ancestry.clone(),
                node,
            })?;
        }
        if let Some(segments) = scope
            .iteration
            .as_ref()
            .and_then(|iteration| iteration.concatenated())
        {
            for (index, segment) in segments.iter().enumerate() {
                self.scope(
                    segment,
                    route.descendant(SourceScopeStep::Segment(index)),
                    ancestry.clone(),
                    closed.clone(),
                )?;
            }
        }
        for (index, child) in scope.children.iter().enumerate() {
            self.scope(
                child,
                route.descendant(SourceScopeStep::Child(index)),
                ancestry.clone(),
                closed.clone(),
            )?;
        }
        if let TargetConstruction::DynamicGroup { children, .. } = &scope.construction {
            for (index, child) in children.iter().enumerate() {
                self.scope(
                    &child.scope,
                    route.descendant(SourceScopeStep::DynamicChild(index)),
                    ancestry.clone(),
                    closed.clone(),
                )?;
            }
        }
        Ok(())
    }
}
pub(super) fn validate_closed_contexts(program: &Program) -> Result<(), ProgramValidationError> {
    validate_closed_contexts_with_profile(program, BasicOwnedProfile::ControlFree)
}
pub(super) fn validate_closed_contexts_with_profile(
    program: &Program,
    profile: BasicOwnedProfile,
) -> Result<(), ProgramValidationError> {
    validate_closed_contexts_with_profiles(program, profile, false)
}

pub(super) fn validate_closed_contexts_with_profiles(
    program: &Program,
    profile: BasicOwnedProfile,
    primary_row_failures: bool,
) -> Result<(), ProgramValidationError> {
    validate_closed_contexts_with_policies(
        program,
        profile,
        primary_row_failures,
        SequenceItemAtPolicy::Closed,
    )
}

pub(super) fn validate_closed_contexts_with_policies(
    program: &Program,
    profile: BasicOwnedProfile,
    primary_row_failures: bool,
    sequence_item_at: SequenceItemAtPolicy,
) -> Result<(), ProgramValidationError> {
    validate_closed_contexts_with_failures(
        program,
        profile,
        FailurePolicy::from_primary_rows(primary_row_failures),
        sequence_item_at,
    )
}

pub(super) fn validate_closed_contexts_with_failures(
    program: &Program,
    profile: BasicOwnedProfile,
    failures: FailurePolicy,
    sequence_item_at: SequenceItemAtPolicy,
) -> Result<(), ProgramValidationError> {
    validate_closed_contexts_with_hierarchy(
        program,
        profile,
        failures,
        sequence_item_at,
        PathHierarchyPolicy::Closed,
    )
}

pub(super) fn validate_closed_contexts_with_hierarchy(
    program: &Program,
    profile: BasicOwnedProfile,
    failures: FailurePolicy,
    sequence_item_at: SequenceItemAtPolicy,
    path_hierarchy: PathHierarchyPolicy,
) -> Result<(), ProgramValidationError> {
    if !has_owned(program) {
        return Ok(());
    }
    let mut admission = Admission {
        program,
        profile,
        sequence_item_at,
        path_hierarchy,
        visited: Vec::new(),
    };
    admission.scope(
        &program.root,
        SourceScopeLocation::root(SourceScopeTarget::Primary),
        Vec::new(),
        None,
    )?;
    for (index, target) in program.extra_targets.iter().enumerate() {
        admission.scope(
            &target.root,
            SourceScopeLocation::root(SourceScopeTarget::Named {
                index,
                name: target.name.clone(),
            }),
            Vec::new(),
            None,
        )?;
    }
    for (index, rule) in program.failure_rules.iter().enumerate() {
        let mut roots = rule
            .selection
            .predicate()
            .into_iter()
            .chain(rule.message)
            .collect::<Vec<_>>();
        if let FailureIteration::Generated(sequence) = &rule.iteration {
            roots.extend(sequence.roots());
        }
        failure_sources::admit_iteration(program, index, rule, failures)?;
        for node in roots {
            admission.expression(Visit {
                namespace: Namespace::Main,
                closed: (!failures.admits_owned_rule(program, rule)).then_some(Closed {
                    reason: Capability::FailureRule,
                    origin: Origin::Failure { index },
                }),
                scope: None,
                ancestry: Vec::new(),
                node,
            })?;
        }
    }
    // Lowering represents every stored dynamic path; preserve that exact generated inventory.
    for (index, source) in program.extra_sources.iter().enumerate() {
        if let Some(dynamic) = &source.dynamic {
            admission.expression(Visit {
                namespace: Namespace::Main,
                closed: Some(Closed {
                    reason: Capability::DynamicLoader,
                    origin: Origin::DynamicSource {
                        index,
                        name: source.name.clone(),
                    },
                }),
                scope: None,
                ancestry: Vec::new(),
                node: dynamic.path,
            })?;
        }
    }
    for function in &program.user_functions {
        for node in &function.expressions {
            admission.expression(Visit {
                namespace: Namespace::Function(function.id),
                closed: Some(Closed {
                    reason: Capability::IsolatedFunction,
                    origin: Origin::FunctionBody {
                        function: function.id,
                    },
                }),
                scope: None,
                ancestry: Vec::new(),
                node: node.id,
            })?;
        }
    }
    // Retained direct Program main leaves may be unused by a target; named dynamic ownership still closes.
    for node in &program.expressions {
        admission.expression(Visit {
            namespace: Namespace::Main,
            closed: None,
            scope: None,
            ancestry: Vec::new(),
            node: node.id,
        })?;
    }
    Ok(())
}
/// Defensive error for an unimplemented owned expression rendering arm.
pub fn owned_expression_emission_error(
    namespace: Namespace,
    node: NodeId,
    expression: &Expression,
) -> ProgramValidationError {
    let closed = match namespace {
        Namespace::Main => Closed {
            reason: Capability::UnsupportedBasicContext,
            origin: Origin::RetainedExpression { namespace, node },
        },
        Namespace::Function(function) => Closed {
            reason: Capability::IsolatedFunction,
            origin: Origin::FunctionBody { function },
        },
    };
    failure(
        Location::Expression {
            namespace,
            node,
            scope: None,
        },
        request(expression),
        closed,
    )
}
/// Closed Stage-A gate, after the caller's unchanged Program/package/profile checks.
pub fn validate_owned_emission(program: &Program) -> Result<(), ProgramValidationError> {
    if !has_owned(program) {
        return Ok(());
    }
    validate_structure(program)?;
    validate_closed_contexts(program)?;
    let mut nodes = program.expressions.iter().collect::<Vec<_>>();
    nodes.sort_by_key(|node| node.id);
    for node in nodes {
        if request(&node.expression).is_some() {
            return Err(owned_expression_emission_error(
                Namespace::Main,
                node.id,
                &node.expression,
            ));
        }
    }
    for (scope, route) in all_scopes(program) {
        if let Some(source) = scope
            .iteration
            .as_ref()
            .and_then(|iteration| iteration.owned_source_iteration())
        {
            return Err(failure(
                Location::Iteration {
                    scope: route.clone(),
                },
                Some(iteration_request(source.source())),
                Closed {
                    reason: Capability::UnsupportedBasicContext,
                    origin: Origin::Scope { scope: route },
                },
            ));
        }
    }
    Ok(())
}

/// Validates the common, control-free Basic source context supported by both emitters.
///
/// General Program structure and stored ownership contexts are checked first.
/// This additive gate does not replace the ordinary closed emission gate.
pub fn validate_basic_owned_emission(program: &Program) -> Result<(), ProgramValidationError> {
    validate_basic_owned_with_profile(program, BasicOwnedProfile::ControlFree)
}

/// Explicitly admits only static Basic repeated scopes with a filter.
///
/// This validates capability only; no default renderer or execution path opts in.
/// Sort, windows, grouping, specialized contexts and failure ownership stay closed.
pub fn validate_basic_owned_filter_emission(
    program: &Program,
) -> Result<(), ProgramValidationError> {
    validate_basic_owned_with_profile(program, BasicOwnedProfile::FilterOnly)
}

/// Explicitly admits static Basic repeated sorting, with an optional filter.
///
/// Existing default and filter-only routes retain their policies. Both stored
/// filter orders are supported; explicit and implicit windows remain closed.
pub fn validate_basic_owned_sort_emission(program: &Program) -> Result<(), ProgramValidationError> {
    validate_basic_owned_with_profile(program, BasicOwnedProfile::SortAndFilter)
}

/// Explicitly admits ordinary static Basic windows and implicit First.
///
/// Optional filtering and sorting retain both stored orders. Existing entry
/// points and all grouping, specialized, dynamic and function gates stay closed.
pub fn validate_basic_owned_window_emission(
    program: &Program,
) -> Result<(), ProgramValidationError> {
    validate_basic_owned_with_profile(program, BasicOwnedProfile::WindowsAndSortAndFilter)
}

/// Explicitly admits only static Basic By grouping with any-member retention.
/// Qualified filtering, sorting, windows and primary-row failures compose here;
/// every older companion and every other grouping family retains its gate.
pub fn validate_basic_owned_group_by_emission(
    program: &Program,
) -> Result<(), ProgramValidationError> {
    validate_basic_owned_with_profiles(
        program,
        BasicOwnedProfile::ByAndWindowsAndSortAndFilter,
        true,
    )
}

/// Explicitly admits static Basic By and AdjacentBy with candidate retention.
/// Source controls and scalar-producer ItemAt remain orthogonal permissions;
/// owned failure expressions retain their independently closed scalar subset.
pub fn validate_basic_owned_group_adjacent_by_emission(
    program: &Program,
) -> Result<(), ProgramValidationError> {
    validate_basic_owned_with_policies(
        program,
        BasicOwnedProfile::ByAndAdjacentByAndWindowsAndSortAndFilter,
        true,
        SequenceItemAtPolicy::BasicScalarProducers,
    )
}

/// Explicitly admits static Basic By, AdjacentBy and StartingWith with candidate
/// retention. EndingWith/IntoBlocks and inherited specialized contexts remain
/// closed; ItemAt and failure permissions are independent, unchanged policies.
pub fn validate_basic_owned_group_starting_with_emission(
    program: &Program,
) -> Result<(), ProgramValidationError> {
    validate_basic_owned_with_policies(
        program,
        BasicOwnedProfile::ByAndAdjacentByAndStartingWithAndWindowsAndSortAndFilter,
        true,
        SequenceItemAtPolicy::BasicScalarProducers,
    )
}

/// Explicitly admits static Basic By, AdjacentBy, StartingWith and EndingWith
/// with candidate retention. IntoBlocks and inherited specialized contexts
/// remain closed; ItemAt and failure permissions are independent, unchanged policies.
pub fn validate_basic_owned_group_ending_with_emission(
    program: &Program,
) -> Result<(), ProgramValidationError> {
    validate_basic_owned_with_policies(
        program,
        BasicOwnedProfile::ByAndAdjacentByAndStartingWithAndEndingWithAndWindowsAndSortAndFilter,
        true,
        SequenceItemAtPolicy::BasicScalarProducers,
    )
}

/// Explicitly admits SequenceItemAt over the existing four scalar producers.
/// Producer arguments and index retain the receiving source context. By and
/// prior controls compose here; all older profiles and failure scalar subsets
/// retain their existing permissions.
pub fn validate_basic_owned_sequence_item_at_emission(
    program: &Program,
) -> Result<(), ProgramValidationError> {
    validate_basic_owned_with_policies(
        program,
        BasicOwnedProfile::ByAndWindowsAndSortAndFilter,
        true,
        SequenceItemAtPolicy::BasicScalarProducers,
    )
}

/// Explicitly admits an owned path-hierarchy input collection, composing the
/// qualified EndingWith controls, scalar-producer ItemAt, primary-row failures
/// and owned failure-source walks. Legacy hierarchies and every other tree
/// operator stay closed.
pub fn validate_basic_owned_path_hierarchy_emission(
    program: &Program,
) -> Result<(), ProgramValidationError> {
    validate_basic_owned_with_hierarchy(
        program,
        BasicOwnedProfile::ByAndAdjacentByAndStartingWithAndEndingWithAndWindowsAndSortAndFilter,
        FailurePolicy::OwnedSources,
        SequenceItemAtPolicy::BasicScalarProducers,
        PathHierarchyPolicy::Owned,
    )
}

fn validate_basic_owned_with_profile(
    program: &Program,
    profile: BasicOwnedProfile,
) -> Result<(), ProgramValidationError> {
    validate_basic_owned_with_profiles(program, profile, false)
}

fn validate_basic_owned_with_profiles(
    program: &Program,
    profile: BasicOwnedProfile,
    primary_row_failures: bool,
) -> Result<(), ProgramValidationError> {
    validate_basic_owned_with_policies(
        program,
        profile,
        primary_row_failures,
        SequenceItemAtPolicy::Closed,
    )
}

fn validate_basic_owned_with_policies(
    program: &Program,
    profile: BasicOwnedProfile,
    primary_row_failures: bool,
    sequence_item_at: SequenceItemAtPolicy,
) -> Result<(), ProgramValidationError> {
    validate_basic_owned_with_failures(
        program,
        profile,
        FailurePolicy::from_primary_rows(primary_row_failures),
        sequence_item_at,
    )
}

fn validate_basic_owned_with_failures(
    program: &Program,
    profile: BasicOwnedProfile,
    failures: FailurePolicy,
    sequence_item_at: SequenceItemAtPolicy,
) -> Result<(), ProgramValidationError> {
    validate_basic_owned_with_hierarchy(
        program,
        profile,
        failures,
        sequence_item_at,
        PathHierarchyPolicy::Closed,
    )
}

fn validate_basic_owned_with_hierarchy(
    program: &Program,
    profile: BasicOwnedProfile,
    failures: FailurePolicy,
    sequence_item_at: SequenceItemAtPolicy,
    path_hierarchy: PathHierarchyPolicy,
) -> Result<(), ProgramValidationError> {
    super::validate_program_with_owned_hierarchy(
        program,
        profile,
        failures,
        sequence_item_at,
        path_hierarchy,
    )?;
    let policy = BasicScopePolicy {
        profile,
        path_hierarchy,
    };
    basic_scope(
        program,
        &program.root,
        SourceScopeLocation::root(SourceScopeTarget::Primary),
        policy,
        false,
    )?;
    for (index, target) in program.extra_targets.iter().enumerate() {
        basic_scope(
            program,
            &target.root,
            SourceScopeLocation::root(SourceScopeTarget::Named {
                index,
                name: target.name.clone(),
            }),
            policy,
            false,
        )?;
    }
    if let Some(function) = program.user_functions.first() {
        return Err(failure(
            Location::Expression {
                namespace: Namespace::Function(function.id),
                node: function.output,
                scope: None,
            },
            None,
            Closed {
                reason: Capability::IsolatedFunction,
                origin: Origin::FunctionBody {
                    function: function.id,
                },
            },
        ));
    }
    basic_failures_with_profile(program, failures)?;
    let retained = basic_target_expressions(program, profile, sequence_item_at);
    for node in &program.expressions {
        if let Expression::SourceField { path, frame } = &node.expression
            && retained.contains(&node.id)
            && basic_dynamic_path(program, frame.as_ref().unwrap_or(path))
        {
            return Err(owned_expression_emission_error(
                Namespace::Main,
                node.id,
                &node.expression,
            ));
        }
        let reason = match &node.expression {
            Expression::SequenceItemAt { .. } if sequence_item_at.permits(&node.expression) => {
                continue;
            }
            Expression::OwnedSourceField(_)
            | Expression::OwnedPosition(_)
            | Expression::SourceField { .. }
            | Expression::Position { .. }
            | Expression::Const { .. }
            | Expression::Call { .. }
            | Expression::If { .. }
            | Expression::ValueMap { .. }
            | Expression::RuntimeValue { .. }
            | Expression::RuntimeParameter { .. }
            | Expression::RuntimeParameterDefault { .. }
            | Expression::Raise { .. } => continue,
            Expression::SourceRootXmlTypeEquals { .. }
            | Expression::XmlSerialize { .. }
            | Expression::XmlMixedContent { .. } => Capability::XmlOperator,
            Expression::SourceDocumentPath => Capability::Document,
            Expression::DynamicSourceField { .. } => Capability::DynamicLoader,
            Expression::JoinField { .. } | Expression::JoinPosition { .. } => Capability::Join,
            Expression::Lookup { .. }
            | Expression::CollectionFind { .. }
            | Expression::Aggregate { .. }
            | Expression::JoinAggregate { .. } => Capability::AggregateLookup,
            Expression::SequenceExists { .. }
            | Expression::SequenceItemAt { .. }
            | Expression::SequenceAggregate { .. } => Capability::GeneratedItem,
            Expression::UserFunctionCall { .. } | Expression::FunctionParameter { .. } => {
                Capability::IsolatedFunction
            }
            _ => Capability::UnsupportedBasicContext,
        };
        return Err(failure(
            Location::Expression {
                namespace: Namespace::Main,
                node: node.id,
                scope: None,
            },
            request(&node.expression),
            Closed {
                reason,
                origin: Origin::RetainedExpression {
                    namespace: Namespace::Main,
                    node: node.id,
                },
            },
        ));
    }
    Ok(())
}

fn basic_scope_refusal(
    scope: &TargetScope,
    route: SourceScopeLocation,
    reason: Capability,
) -> ProgramValidationError {
    let query = scope
        .iteration
        .as_ref()
        .and_then(|iteration| iteration.owned_source_iteration())
        .map(|source| iteration_request(source.source()))
        .or_else(|| owned_path_hierarchy(scope).map(iteration_request));
    failure(
        Location::Iteration {
            scope: route.clone(),
        },
        query,
        Closed {
            reason,
            origin: Origin::Scope { scope: route },
        },
    )
}

#[derive(Clone, Copy)]
struct BasicScopePolicy {
    profile: BasicOwnedProfile,
    path_hierarchy: PathHierarchyPolicy,
}

fn basic_scope(
    program: &Program,
    scope: &TargetScope,
    route: SourceScopeLocation,
    policy: BasicScopePolicy,
    inherited_owned: bool,
) -> Result<(), ProgramValidationError> {
    let profile = policy.profile;
    if let Some(iteration) = &scope.iteration {
        let reason = match iteration.input() {
            IterationSource::OwnedSource(_) | IterationSource::Source(_) => None,
            IterationSource::Generated(_) => Some(Capability::GeneratedItem),
            IterationSource::InnerJoin(_) => Some(Capability::Join),
            IterationSource::Concatenate(_) => Some(Capability::Concatenate),
            IterationSource::DynamicDocuments(_) => Some(Capability::Document),
        };
        if let Some(reason) = reason {
            return Err(basic_scope_refusal(scope, route, reason));
        }
        let legacy_grouping_island = profile.supports_groups()
            && !inherited_owned
            && !basic_scope_needs_owned(program, scope)
            && iteration.source_iteration().is_some()
            && iteration.grouping().is_some()
            && (iteration.output() == IterationOutput::Repeated
                || iteration.output() == IterationOutput::First
                    && matches!(scope.construction, TargetConstruction::Group));
        if !legacy_grouping_island
            && (iteration.filter().is_some() && !profile.permits_filter(scope)
                || iteration.sort().is_some() && !profile.permits_sort(scope)
                || (iteration.grouping().is_some() || iteration.post_group_filter().is_some())
                    && !profile.permits_group(scope)
                || !iteration.windows().is_empty() && !profile.permits_windows(scope)
                || iteration.output() != IterationOutput::Repeated
                    && !profile.permits_windows(scope))
        {
            return Err(basic_scope_refusal(scope, route, Capability::ScopeControl));
        }
    }
    let admitted_hierarchy = policy.path_hierarchy == PathHierarchyPolicy::Owned
        && owned_path_hierarchy(scope).is_some();
    if !admitted_hierarchy
        && !matches!(
            scope.construction,
            TargetConstruction::Group | TargetConstruction::Scalar { .. }
        )
    {
        let reason = match scope.construction {
            TargetConstruction::XmlMixedContent { .. } => Capability::XmlOperator,
            TargetConstruction::DynamicGroup { .. } => Capability::DynamicTarget,
            TargetConstruction::RecursiveFilter { .. }
            | TargetConstruction::PathHierarchy { .. }
            | TargetConstruction::OwnedPathHierarchy { .. }
            | TargetConstruction::AdjacencyTree { .. } => Capability::TreeOperator,
            _ => Capability::SpecialConstruction,
        };
        return Err(basic_scope_refusal(scope, route, reason));
    }
    if let Some(iteration) = &scope.iteration
        && let IterationSource::Source(source) = iteration.input()
        && basic_dynamic_path(program, source.path())
    {
        return Err(basic_scope_refusal(scope, route, Capability::DynamicLoader));
    }
    let inherited_owned = inherited_owned
        || scope.iteration.as_ref().is_some_and(|iteration| {
            iteration.owned_source_iteration().is_some()
                || profile.permits_group(scope) && basic_scope_needs_owned(program, scope)
        });
    for (index, child) in scope.children.iter().enumerate() {
        basic_scope(
            program,
            child,
            route.descendant(SourceScopeStep::Child(index)),
            policy,
            inherited_owned,
        )?;
    }
    Ok(())
}

fn basic_scope_needs_owned(program: &Program, scope: &TargetScope) -> bool {
    if owned_path_hierarchy(scope).is_some()
        || scope
            .iteration
            .as_ref()
            .is_some_and(|iteration| iteration.owned_source_iteration().is_some())
    {
        return true;
    }
    let mut pending = scope
        .bindings
        .iter()
        .map(|binding| binding.expression)
        .collect::<Vec<_>>();
    if let Some(iteration) = &scope.iteration {
        pending.extend(iteration.roots());
    }
    if let TargetConstruction::Scalar { expression, .. } = &scope.construction {
        pending.push(*expression);
    }
    let mut visited = std::collections::BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let Some(node) = program.expressions.iter().find(|node| node.id == id) else {
            continue;
        };
        if request(&node.expression).is_some() {
            return true;
        }
        pending.extend(graph_dependencies::of(&node.expression));
    }
    scope
        .children
        .iter()
        .any(|child| basic_scope_needs_owned(program, child))
}

fn basic_dynamic_path(program: &Program, path: &[String]) -> bool {
    path.first().is_some_and(|name| {
        program
            .extra_sources
            .iter()
            .any(|source| source.name == *name && source.dynamic.is_some())
    })
}

fn basic_expression_nodes(
    program: &Program,
    roots: impl IntoIterator<Item = NodeId>,
) -> std::collections::BTreeSet<NodeId> {
    let mut pending = roots.into_iter().collect::<Vec<_>>();
    let mut found = std::collections::BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !found.insert(id) {
            continue;
        }
        let Some(node) = program.expressions.iter().find(|node| node.id == id) else {
            continue;
        };
        // These are precisely the potential dependency edges of the admitted
        // ordinary scalar subset; If stays conservative and execution stays lazy.
        if matches!(
            node.expression,
            Expression::Call { .. }
                | Expression::If { .. }
                | Expression::RuntimeParameterDefault { .. }
                | Expression::Raise { .. }
        ) {
            pending.extend(graph_dependencies::of(&node.expression));
        }
    }
    found
}

fn basic_target_expressions(
    program: &Program,
    profile: BasicOwnedProfile,
    sequence_item_at: SequenceItemAtPolicy,
) -> std::collections::BTreeSet<NodeId> {
    fn roots(scope: &TargetScope, profile: BasicOwnedProfile, output: &mut Vec<NodeId>) {
        if let TargetConstruction::Scalar { expression, .. } = &scope.construction {
            output.push(*expression);
        }
        output.extend(scope.bindings.iter().map(|binding| binding.expression));
        if profile.permits_filter(scope) {
            // A predicate retains every potential scalar dependency, including
            // unchosen branches/defaults and the existing target ValueMap edge.
            output.extend(
                scope
                    .iteration
                    .as_ref()
                    .and_then(|iteration| iteration.filter()),
            );
        }
        if profile.permits_sort(scope)
            && let Some(sort) = scope
                .iteration
                .as_ref()
                .and_then(|iteration| iteration.sort())
        {
            // Every stored key retains its potential scalar dependencies;
            // static admission does not skip secondary or inactive key edges.
            output.extend(sort.keys().map(|key| key.expression));
        }
        if profile.permits_windows(scope)
            && let Some(iteration) = &scope.iteration
        {
            // Count bounds retain all potential scalar edges even when the
            // source is empty or a preceding window would discard every row.
            output.extend(
                iteration
                    .windows()
                    .iter()
                    .copied()
                    .flat_map(crate::SequenceWindow::nodes),
            );
        }
        if profile.supports_groups()
            && let Some(iteration) = &scope.iteration
        {
            // Grouping profiles also retain controls of ordinary legacy grouping
            // islands. Their capability was checked independently by basic_scope.
            output.extend(iteration.roots());
        }
        for child in &scope.children {
            roots(child, profile, output);
        }
    }
    let mut pending = Vec::new();
    roots(&program.root, profile, &mut pending);
    for target in &program.extra_targets {
        roots(&target.root, profile, &mut pending);
    }
    let mut retained = basic_expression_nodes(program, pending);
    let mut pending = retained.iter().copied().collect::<Vec<_>>();
    while let Some(id) = pending.pop() {
        let Some(node) = program.expressions.iter().find(|node| node.id == id) else {
            continue;
        };
        let dependencies = match &node.expression {
            Expression::ValueMap { input, .. } => vec![*input],
            expression if sequence_item_at.permits(expression) => {
                // Both producer scalar inputs and index are parent demands;
                // generated item declarations grant no endpoint permission.
                graph_dependencies::of(expression)
            }
            _ => continue,
        };
        // Only target admission gains the explicitly permitted consumer edges.
        // Failure traversal keeps its independent, closed scalar subset.
        for dependency in basic_expression_nodes(program, dependencies) {
            if retained.insert(dependency) {
                pending.push(dependency);
            }
        }
    }
    retained
}

fn basic_failure_refusal(index: usize, reason: Capability) -> ProgramValidationError {
    failure(
        Location::Iteration {
            scope: SourceScopeLocation::root(SourceScopeTarget::Primary),
        },
        None,
        Closed {
            reason,
            origin: Origin::Failure { index },
        },
    )
}

fn basic_failures_with_profile(
    program: &Program,
    failures: FailurePolicy,
) -> Result<(), ProgramValidationError> {
    for (index, rule) in program.failure_rules.iter().enumerate() {
        if failures.admits_owned_rule(program, rule) {
            continue;
        }
        let FailureIteration::Source(source) = &rule.iteration else {
            return Err(basic_failure_refusal(index, Capability::FailureRule));
        };
        if basic_dynamic_path(program, source.path()) {
            return Err(basic_failure_refusal(index, Capability::DynamicLoader));
        }
        for root in rule.selection.predicate().into_iter().chain(rule.message) {
            let retained = basic_expression_nodes(program, [root]);
            for node in &program.expressions {
                if !retained.contains(&node.id) {
                    continue;
                }
                let dynamic = match &node.expression {
                    Expression::SourceField { path, frame } => {
                        basic_dynamic_path(program, frame.as_ref().unwrap_or(path))
                    }
                    Expression::DynamicSourceField { .. } => true,
                    _ => false,
                };
                if dynamic {
                    return Err(failure(
                        Location::Expression {
                            namespace: Namespace::Main,
                            node: node.id,
                            scope: None,
                        },
                        None,
                        Closed {
                            reason: Capability::DynamicLoader,
                            origin: Origin::Failure { index },
                        },
                    ));
                }
                if !matches!(
                    &node.expression,
                    Expression::SourceField { .. }
                        | Expression::Position { .. }
                        | Expression::Const { .. }
                        | Expression::Call { .. }
                        | Expression::If { .. }
                        | Expression::RuntimeValue { .. }
                        | Expression::RuntimeParameter { .. }
                        | Expression::RuntimeParameterDefault { .. }
                        | Expression::Raise { .. }
                ) {
                    return Err(basic_failure_refusal(index, Capability::FailureRule));
                }
            }
        }
    }
    Ok(())
}
