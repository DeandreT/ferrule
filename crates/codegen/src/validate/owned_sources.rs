//! Exact ownership structure and the owned capability gates.
//!
//! Structure checks run for every validator. Closed-context admission
//! (`admission`) and complete Basic emission (`basic`) then read one
//! [`OwnedCapabilities`] set; each capability's own checks live in its
//! submodule.
use super::{
    OwnedSourceCapabilityOrigin as Origin, OwnedSourceCapabilityReason as Capability,
    OwnedSourceDiagnostic as Diagnostic, OwnedSourceDiagnosticLocation as Location,
    OwnedSourceDiagnosticOwner as Owner, OwnedSourceDiagnosticRequest as Request,
    OwnedSourceNamespace as Namespace, OwnedSourceSchemaShape as Shape,
    OwnedSourceStructureReason as Structure, ProgramValidationError, graph_dependencies,
};
use crate::{
    Expression, FailureIteration, IterationOutput, IterationSource, OwnedCapabilities, OwnedPreset,
    Program, TargetConstruction, TargetScope,
};
use ir::{SchemaKind, SchemaNode};
use mapping::{NodeId, SourceScopeLocation, SourceScopeStep, SourceScopeTarget};

mod admission;
pub(super) use admission::validate_closed_contexts;
mod basic;
pub use basic::validate_basic_owned_emission_with_capabilities;
mod controls;
mod dynamic_loading;
pub use dynamic_loading::{OwnedDynamicLoading, owned_dynamic_loading};

mod adjacency_tree;
use adjacency_tree::{owned_root, owned_rows};
mod collection_aggregates;
mod dynamic_field;
mod failure_sources;
mod joins;
mod lookup_find;
mod primary_row_failures;
pub use primary_row_failures::is_basic_owned_primary_row_failure;
mod recursive_collect;
pub use recursive_collect::owned_sequence_emission_error;
mod recursive_filter;
mod xml_serialize;

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
        Expression::OwnedRowDocumentPath(path) => Some(Request::RowDocumentPath {
            owner: (&path.owner).into(),
            collection: path.collection.clone(),
        }),
        Expression::OwnedSourceDocumentPath(path) => Some(Request::DocumentPath {
            owner: (&path.owner).into(),
        }),
        Expression::OwnedDynamicSourceField(field) => Some(dynamic_field::request(field)),
        // A consumer of an owned producer requests that root collection.
        Expression::SequenceExists { sequence, .. }
        | Expression::SequenceItemAt { sequence, .. }
        | Expression::SequenceAggregate { sequence, .. } => {
            sequence.owned_source().map(iteration_request)
        }
        // An owned search walks its collection exactly like owned iteration.
        Expression::OwnedLookup(mapping::OwnedLookup { collection, .. })
        | Expression::OwnedCollectionFind(mapping::OwnedCollectionFind { collection, .. })
        | Expression::OwnedAggregate(mapping::OwnedAggregate { collection, .. }) => {
            Some(iteration_request(collection))
        }
        Expression::OwnedJoinAggregate(aggregate) => joins::aggregate_request(aggregate),
        Expression::OwnedXmlSerialize(serializer) => Some(Request::XmlElement {
            owner: (&serializer.source.owner).into(),
            path: serializer.source.path.clone(),
            frame: serializer.frame.clone(),
        }),
        // Mixed-content atomization reads its group as one owned element.
        Expression::OwnedXmlMixedContent(mixed) => Some(Request::XmlElement {
            owner: (&mixed.source.owner).into(),
            path: mixed.source.path.clone(),
            frame: mixed.frame.clone(),
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
        }
        | Request::RowDocumentPath {
            owner,
            collection: path,
        } => {
            let root = endpoint(program, owner)?;
            if let Some(path) = path {
                collection(root, path)?;
            }
            Ok(())
        }
        Request::DocumentPath { owner } => endpoint(program, owner).map(|_| ()),
        Request::Iteration { owner, path } => collection(endpoint(program, owner)?, path),
        Request::DynamicField {
            owner,
            object,
            frame,
        } => dynamic_field::check(program, owner, object, frame.as_deref()),
        // The element's schema identity is checked with the serializer schema;
        // here the owner, frame and path must exist in the exact endpoint.
        Request::XmlElement { owner, path, frame } => {
            let root = endpoint(program, owner)?;
            if let Some(frame) = frame {
                if !path.starts_with(frame) {
                    return Err(Structure::IncompatibleFramePath);
                }
                collection(root, frame)?;
            }
            resolved(root, follow(root, path)?).map(|_| ())
        }
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
        || owned_rows(scope).is_some()
        || scope.iteration.as_ref().is_some_and(|iteration| {
            iteration.owned_source_iteration().is_some()
                || iteration.owned_inner_join().is_some()
                || recursive_collect::scope_sequence(iteration).is_some()
                || iteration
                    .concatenated()
                    .is_some_and(|segments| segments.iter().any(scope_has_owned))
        })
        || scope.children.iter().any(scope_has_owned)
        || matches!(&scope.construction, TargetConstruction::DynamicGroup { children, .. } if children.iter().any(|child| scope_has_owned(&child.scope)))
}
fn has_owned(program: &Program) -> bool {
    program
        .expressions
        .iter()
        .any(|node| request(&node.expression).is_some() || joins::is_join_read(&node.expression))
        || program.user_functions.iter().any(|function| {
            function.expressions.iter().any(|node| {
                request(&node.expression).is_some() || joins::is_join_read(&node.expression)
            })
        })
        || scope_has_owned(&program.root)
        || program
            .extra_targets
            .iter()
            .any(|target| scope_has_owned(&target.root))
        || program.failure_rules.iter().any(|rule| {
            matches!(rule.iteration, FailureIteration::OwnedSource(_))
                || recursive_collect::failure_sequence(rule).is_some()
        })
        || owned_drivers(program).next().is_some()
}
/// Each dynamic declaration with an owned driver, in declaration order.
fn owned_drivers(
    program: &Program,
) -> impl Iterator<Item = (usize, &crate::NamedSourceProgram, &mapping::OwnedSourceRef)> {
    program
        .extra_sources
        .iter()
        .enumerate()
        .filter_map(|(index, source)| {
            let driver = source.dynamic.as_ref()?.driver.owned()?;
            Some((index, source, driver))
        })
}
fn driver_location(index: usize, source: &crate::NamedSourceProgram) -> Location {
    Location::DynamicDriver {
        index,
        name: source.name.clone(),
    }
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
            let location = Location::Expression {
                namespace: Namespace::Main,
                node: node.id,
                scope: None,
            };
            structure(program, location.clone(), query)?;
            match &node.expression {
                Expression::OwnedLookup(lookup) => {
                    lookup_find::lookup_structure(program, location, lookup)?
                }
                Expression::OwnedAggregate(aggregate) => {
                    collection_aggregates::value_structure(program, location, aggregate)?
                }
                Expression::OwnedJoinAggregate(aggregate) => {
                    joins::aggregate_structure(program, location, aggregate)?
                }
                _ => {}
            }
        }
    }
    for function in &program.user_functions {
        let mut nodes = function.expressions.iter().collect::<Vec<_>>();
        nodes.sort_by_key(|node| node.id);
        for node in nodes {
            if let Some(query) = request(&node.expression) {
                let location = Location::Expression {
                    namespace: Namespace::Function(function.id),
                    node: node.id,
                    scope: None,
                };
                structure(program, location.clone(), query)?;
                match &node.expression {
                    Expression::OwnedLookup(lookup) => {
                        lookup_find::lookup_structure(program, location, lookup)?
                    }
                    Expression::OwnedAggregate(aggregate) => {
                        collection_aggregates::value_structure(program, location, aggregate)?
                    }
                    Expression::OwnedJoinAggregate(aggregate) => {
                        joins::aggregate_structure(program, location, aggregate)?
                    }
                    _ => {}
                }
            }
        }
    }
    for (scope, route) in all_scopes(program) {
        if let Some(join) = scope
            .iteration
            .as_ref()
            .and_then(|iteration| iteration.owned_inner_join())
        {
            joins::plan_structure(program, &route, join)?;
        }
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
        if let Some(collection) = owned_path_hierarchy(scope)
            .or_else(|| owned_rows(scope))
            .or_else(|| {
                scope
                    .iteration
                    .as_ref()
                    .and_then(recursive_collect::scope_sequence)
            })
        {
            structure(
                program,
                Location::Iteration { scope: route },
                iteration_request(collection),
            )?;
        }
    }
    for (index, source, driver) in owned_drivers(program) {
        structure(
            program,
            driver_location(index, source),
            iteration_request(driver),
        )?;
        if matches!(&driver.owner, mapping::SourceOwner::Named { name } if *name == source.name) {
            return Err(ProgramValidationError::OwnedSource {
                diagnostic: Diagnostic::Structure {
                    location: driver_location(index, source),
                    request: iteration_request(driver),
                    reason: Structure::SelfDrivenDynamicSource,
                }
                .into(),
            });
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
        if let Some(source) = recursive_collect::failure_sequence(rule) {
            structure(
                program,
                Location::FailureIteration { index },
                iteration_request(source),
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
    loaded_owner(program, query, false)
}
/// A named owner that is a dynamic source, unless `loaded` admits exact reads
/// of an owned dynamic endpoint's loaded documents.
fn loaded_owner(program: &Program, query: &Request, loaded: bool) -> Option<Closed> {
    let owner = match query {
        Request::Field { owner, .. }
        | Request::Position { owner, .. }
        | Request::RowDocumentPath { owner, .. }
        | Request::DocumentPath { owner }
        | Request::Iteration { owner, .. }
        | Request::DynamicField { owner, .. }
        | Request::XmlElement { owner, .. } => owner,
    };
    let Owner::Named { name } = owner else {
        return None;
    };
    program
        .extra_sources
        .iter()
        .enumerate()
        .find(|(_, source)| source.name == *name && source.dynamic.is_some())
        .filter(|(_, source)| {
            !(loaded
                && source
                    .dynamic
                    .as_ref()
                    .is_some_and(|dynamic| dynamic.driver.owned().is_some()))
        })
        .map(|(index, source)| Closed {
            reason: Capability::DynamicOwner,
            origin: Origin::DynamicSource {
                index,
                name: source.name.clone(),
            },
        })
}
/// Whether `scope` or a descendant reads or iterates an owned source.
fn basic_scope_needs_owned(program: &Program, scope: &TargetScope) -> bool {
    if owned_path_hierarchy(scope).is_some()
        || owned_rows(scope).is_some()
        || scope.iteration.as_ref().is_some_and(|iteration| {
            iteration.owned_source_iteration().is_some() || iteration.owned_inner_join().is_some()
        })
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

/// Whether `path` names a dynamic named source.
fn basic_dynamic_path(program: &Program, path: &[String]) -> bool {
    path.first().is_some_and(|name| {
        program
            .extra_sources
            .iter()
            .any(|source| source.name == *name && source.dynamic.is_some())
    })
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
    validate_closed_contexts(program, OwnedPreset::ControlFree.capabilities())?;
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
