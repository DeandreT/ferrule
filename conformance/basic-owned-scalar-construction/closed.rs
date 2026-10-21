//! Every Basic generation route, plus the specialized contexts and directly
//! constructed Programs that keep refusing owned scalar reads. Expected errors
//! are authored here.
use super::*;
use codegen::{
    OwnedSourceCapabilityOrigin as Origin, OwnedSourceCapabilityReason as Reason,
    OwnedSourceDiagnostic as Diagnostic, OwnedSourceDiagnosticLocation as Location,
    OwnedSourceDiagnosticOwner as Owner, OwnedSourceDiagnosticRequest as Request,
    OwnedSourceNamespace as Namespace, OwnedSourceStructureReason as Structure,
    ProgramValidationError,
};
use mapping::{
    OwnedSourceRef, ScopeConstruction, SourceScopeLocation, SourceScopeStep, SourceScopeTarget,
};
use projects::{REF, VALUE, path};

fn root() -> SourceScopeLocation {
    SourceScopeLocation::root(SourceScopeTarget::Primary)
}
fn child() -> SourceScopeLocation {
    root().descendant(SourceScopeStep::Child(0))
}
fn field(owner: Owner, parts: &[&str]) -> Request {
    Request::Field {
        owner,
        path: path(parts),
        frame: None,
    }
}
fn capability(
    location: Location,
    request: Option<Request>,
    reason: Reason,
    origin: Origin,
) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(Diagnostic::Capability {
            location,
            request,
            reason,
            origin,
        }),
    }
}
fn structure(node: u32, request: Request, reason: Structure) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(Diagnostic::Structure {
            location: Location::Expression {
                namespace: Namespace::Main,
                node,
                scope: None,
            },
            request,
            reason,
        }),
    }
}
/// An owned read refused inside the Primary target's first child scope.
fn read_in_child(node: u32, reason: Reason) -> ProgramValidationError {
    capability(
        Location::Expression {
            namespace: Namespace::Main,
            node,
            scope: Some(child()),
        },
        Some(field(Owner::Primary, &["Value"])),
        reason,
        Origin::Scope { scope: child() },
    )
}

/// Every Basic lowering route. All of them admit owned scalar construction.
pub fn lowerers(project: &Project) -> Vec<Result<codegen::Program, codegen::LowerError>> {
    vec![
        codegen::lower(project),
        codegen::lower_with_basic_owned_filters(project),
        codegen::lower_with_basic_owned_sort(project),
        codegen::lower_with_basic_owned_windows(project),
        codegen::lower_with_basic_owned_row_failures(project),
        codegen::lower_with_basic_owned_group_by(project),
        codegen::lower_with_basic_owned_sequence_item_at(project),
        codegen::lower_with_basic_owned_group_adjacent_by(project),
        codegen::lower_with_basic_owned_group_starting_with(project),
        codegen::lower_with_basic_owned_group_ending_with(project),
        codegen::lower_with_basic_owned_failure_sources(project),
        codegen::lower_with_basic_owned_path_hierarchy(project),
        codegen::lower_with_basic_owned_adjacency_tree(project),
        codegen::lower_with_basic_owned_group_into_blocks(project),
        codegen::lower_with_basic_owned_recursive_collect(project),
        codegen::lower_with_basic_owned_sequence_exists(project),
        codegen::lower_with_basic_owned_recursive_filter(project),
        codegen::lower_with_basic_owned_lookup_find(project),
    ]
}
pub fn validators(p: &codegen::Program) -> Vec<Result<(), ProgramValidationError>> {
    vec![
        codegen::validate_program(p),
        codegen::validate_basic_owned_emission(p),
        codegen::validate_program_with_basic_owned_filters(p),
        codegen::validate_basic_owned_filter_emission(p),
        codegen::validate_program_with_basic_owned_sort(p),
        codegen::validate_basic_owned_sort_emission(p),
        codegen::validate_program_with_basic_owned_windows(p),
        codegen::validate_basic_owned_window_emission(p),
        codegen::validate_program_with_basic_owned_row_failures(p),
        codegen::validate_basic_owned_row_failure_emission(p),
        codegen::validate_program_with_basic_owned_group_by(p),
        codegen::validate_basic_owned_group_by_emission(p),
        codegen::validate_program_with_basic_owned_sequence_item_at(p),
        codegen::validate_basic_owned_sequence_item_at_emission(p),
        codegen::validate_program_with_basic_owned_group_adjacent_by(p),
        codegen::validate_basic_owned_group_adjacent_by_emission(p),
        codegen::validate_program_with_basic_owned_group_starting_with(p),
        codegen::validate_basic_owned_group_starting_with_emission(p),
        codegen::validate_program_with_basic_owned_group_ending_with(p),
        codegen::validate_basic_owned_group_ending_with_emission(p),
        codegen::validate_program_with_basic_owned_failure_sources(p),
        codegen::validate_basic_owned_failure_source_emission(p),
        codegen::validate_program_with_basic_owned_path_hierarchy(p),
        codegen::validate_basic_owned_path_hierarchy_emission(p),
        codegen::validate_program_with_basic_owned_adjacency_tree(p),
        codegen::validate_basic_owned_adjacency_tree_emission(p),
        codegen::validate_program_with_basic_owned_group_into_blocks(p),
        codegen::validate_basic_owned_group_into_blocks_emission(p),
        codegen::validate_program_with_basic_owned_recursive_collect(p),
        codegen::validate_basic_owned_recursive_collect_emission(p),
        codegen::validate_program_with_basic_owned_sequence_exists(p),
        codegen::validate_basic_owned_sequence_exists_emission(p),
        codegen::validate_program_with_basic_owned_recursive_filter(p),
        codegen::validate_basic_owned_recursive_filter_emission(p),
        codegen::validate_program_with_basic_owned_lookup_find(p),
        codegen::validate_basic_owned_lookup_find_emission(p),
    ]
}
pub fn catalogues(
    p: &codegen::Program,
) -> Vec<Result<codegen::NamedInputAdmissionPlan<'_>, ProgramValidationError>> {
    vec![
        codegen::legacy_named_input_admission_plan(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_filters(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_sort(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_windows(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_row_failures(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_by(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_item_at(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_starting_with(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_ending_with(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_failure_sources(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_path_hierarchy(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_adjacency_tree(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_into_blocks(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_recursive_collect(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_exists(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_recursive_filter(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_lookup_find(p),
    ]
}
/// Every owned generation companion; the ordinary emitter stays closed.
pub fn rust(
    p: &codegen::Program,
    options: &codegen_rust::Options,
) -> Vec<Result<codegen::ArtifactSet, codegen_rust::EmitError>> {
    vec![
        codegen_rust::emit_with_basic_owned_sources(p, options),
        codegen_rust::emit_with_basic_owned_filters(p, options),
        codegen_rust::emit_with_basic_owned_sort(p, options),
        codegen_rust::emit_with_basic_owned_windows(p, options),
        codegen_rust::emit_with_basic_owned_row_failures(p, options),
        codegen_rust::emit_with_basic_owned_group_by(p, options),
        codegen_rust::emit_with_basic_owned_sequence_item_at(p, options),
        codegen_rust::emit_with_basic_owned_group_adjacent_by(p, options),
        codegen_rust::emit_with_basic_owned_group_starting_with(p, options),
        codegen_rust::emit_with_basic_owned_group_ending_with(p, options),
        codegen_rust::emit_with_basic_owned_failure_sources(p, options),
        codegen_rust::emit_with_basic_owned_path_hierarchy(p, options),
        codegen_rust::emit_with_basic_owned_adjacency_tree(p, options),
        codegen_rust::emit_with_basic_owned_group_into_blocks(p, options),
        codegen_rust::emit_with_basic_owned_recursive_collect(p, options),
        codegen_rust::emit_with_basic_owned_sequence_exists(p, options),
        codegen_rust::emit_with_basic_owned_recursive_filter(p, options),
        codegen_rust::emit_with_basic_owned_lookup_find(p, options),
    ]
}
pub fn csharp(
    p: &codegen::Program,
) -> Vec<Result<codegen::ArtifactSet, codegen_csharp::EmitError>> {
    vec![
        codegen_csharp::emit_with_basic_owned_sources(p),
        codegen_csharp::emit_with_basic_owned_filters(p),
        codegen_csharp::emit_with_basic_owned_sort(p),
        codegen_csharp::emit_with_basic_owned_windows(p),
        codegen_csharp::emit_with_basic_owned_row_failures(p),
        codegen_csharp::emit_with_basic_owned_group_by(p),
        codegen_csharp::emit_with_basic_owned_sequence_item_at(p),
        codegen_csharp::emit_with_basic_owned_group_adjacent_by(p),
        codegen_csharp::emit_with_basic_owned_group_starting_with(p),
        codegen_csharp::emit_with_basic_owned_group_ending_with(p),
        codegen_csharp::emit_with_basic_owned_failure_sources(p),
        codegen_csharp::emit_with_basic_owned_path_hierarchy(p),
        codegen_csharp::emit_with_basic_owned_adjacency_tree(p),
        codegen_csharp::emit_with_basic_owned_group_into_blocks(p),
        codegen_csharp::emit_with_basic_owned_recursive_collect(p),
        codegen_csharp::emit_with_basic_owned_sequence_exists(p),
        codegen_csharp::emit_with_basic_owned_recursive_filter(p),
        codegen_csharp::emit_with_basic_owned_lookup_find(p),
    ]
}
/// The ordinary emitters' closed refusal of the lowest-numbered owned read.
pub fn ordinary_refusal(p: &codegen::Program) -> Option<ProgramValidationError> {
    let node = p
        .expressions
        .iter()
        .filter(|node| {
            matches!(
                node.expression,
                codegen::Expression::OwnedSourceField(_) | codegen::Expression::OwnedPosition(_)
            )
        })
        .min_by_key(|node| node.id)?;
    Some(codegen::owned_expression_emission_error(
        Namespace::Main,
        node.id,
        &node.expression,
    ))
}

fn owned_value() -> Node {
    projects::owned("Primary", &["Value"])
}
/// A Primary target whose only child is `scope`, filling a field of `schema`.
fn single(scope: Scope, schema: SchemaNode) -> Project {
    let mut project = projects::project("S01-primary-root");
    project.target = SchemaNode::group("Out", vec![schema]);
    project.root = Scope {
        children: vec![scope],
        ..Scope::default()
    };
    project
}

/// Specialized contexts under scalar targets, with complete diagnostics.
pub fn projects() -> Vec<(&'static str, Project, ProgramValidationError)> {
    let mut result = Vec::new();
    // A generated item never becomes an endpoint-bearing scalar frame.
    let mut p = single(
        Scope {
            iteration: ScopeIteration::Sequence(mapping::SequenceExpr::Tokenize {
                input: 30,
                delimiter: 31,
                item: 32,
            }),
            ..projects::scalar_scope("Item", VALUE)
        },
        SchemaNode::scalar("Item", ScalarType::String).repeating(),
    );
    p.graph.nodes.extend([
        (VALUE, owned_value()),
        (
            30,
            Node::Const {
                value: Value::String("a,b".into()),
            },
        ),
        (
            31,
            Node::Const {
                value: Value::String(",".into()),
            },
        ),
        (
            32,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        ),
    ]);
    result.push((
        "K01-generated-item",
        p,
        read_in_child(VALUE, Reason::GeneratedItem),
    ));
    // A scalar argument of a collection aggregate stays an aggregate context.
    let mut p = projects::project("S01-primary-root");
    p.graph.nodes.extend([
        (10, owned_value()),
        (
            VALUE,
            Node::Aggregate {
                function: mapping::AggregateOp::Join,
                collection: path(&["Rows"]),
                value: path(&["Value"]),
                expression: None,
                arg: Some(10),
            },
        ),
    ]);
    result.push((
        "K02-aggregate-argument",
        p,
        capability(
            Location::Expression {
                namespace: Namespace::Main,
                node: 10,
                scope: Some(root()),
            },
            Some(field(Owner::Primary, &["Value"])),
            Reason::AggregateLookup,
            Origin::Expression {
                namespace: Namespace::Main,
                node: VALUE,
            },
        ),
    ));
    // An isolated user function never receives an owned scalar context.
    let mut p = projects::project("S01-primary-root");
    let function = mapping::FunctionId::new(7);
    let parameter = mapping::FunctionParameterId::new(1);
    let mut body = Graph::default();
    body.nodes.insert(1, Node::FunctionParameter { parameter });
    p.user_functions.insert(
        function,
        mapping::UserFunction {
            library: "scalar550".into(),
            name: "identity".into(),
            description: None,
            parameters: vec![mapping::FunctionParameter {
                id: parameter,
                name: "value".into(),
                ty: ScalarType::String,
            }],
            output_name: "result".into(),
            output_type: ScalarType::String,
            body,
            output: 1,
        },
    );
    p.graph.nodes.extend([
        (10, owned_value()),
        (
            VALUE,
            Node::UserFunctionCall {
                function,
                args: vec![10],
            },
        ),
    ]);
    result.push((
        "K03-isolated-function",
        p,
        capability(
            Location::Expression {
                namespace: Namespace::Function(function),
                node: 1,
                scope: None,
            },
            None,
            Reason::IsolatedFunction,
            Origin::FunctionBody { function },
        ),
    ));
    // An exact copy remains the closed special construction.
    let rows = match &projects::endpoint_schema("Input").kind {
        ir::SchemaKind::Group { children, .. } => children
            .iter()
            .find(|child| child.name == "Rows")
            .cloned()
            .expect("authored Rows schema"),
        _ => unreachable!("authored endpoint group"),
    };
    let mut copy = rows;
    copy.name = "Copy".into();
    let p = single(
        Scope {
            target_field: "Copy".into(),
            iteration: ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: projects::owner("Primary"),
                    path: path(&["Rows"]),
                },
                output_path: None,
            },
            construction: ScopeConstruction::CopyCurrentSource,
            ..Scope::default()
        },
        copy,
    );
    result.push((
        "K04-copy-current-source",
        p,
        capability(
            Location::Iteration { scope: child() },
            Some(Request::Iteration {
                owner: Owner::Primary,
                path: path(&["Rows"]),
            }),
            Reason::SpecialConstruction,
            Origin::Scope { scope: child() },
        ),
    ));
    // Grouped scalar construction stays with the grouping companions.
    let mut p = single(
        Scope {
            iteration: ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: projects::owner("Primary"),
                    path: path(&["Rows"]),
                },
                output_path: None,
            },
            group_by: Some(VALUE),
            ..projects::scalar_scope("Item", VALUE)
        },
        SchemaNode::scalar("Item", ScalarType::String).repeating(),
    );
    p.graph
        .nodes
        .insert(VALUE, projects::owned("Primary", &["Rows", "Value"]));
    result.push((
        "K05-grouped-scalar",
        p,
        capability(
            Location::Iteration { scope: child() },
            Some(Request::Iteration {
                owner: Owner::Primary,
                path: path(&["Rows"]),
            }),
            Reason::Grouping,
            Origin::Scope { scope: child() },
        ),
    ));
    result
}

/// The lowering routes that must refuse closed Project `id`.
pub fn closed_lowerers(
    id: &str,
    project: &Project,
) -> Vec<Result<codegen::Program, codegen::LowerError>> {
    let mut routes = lowerers(project);
    if id == "K05-grouped-scalar" {
        // Base, filter, sort, window and row-failure companions have no grouping.
        routes.truncate(5);
    }
    routes
}

/// Programs constructed directly from a valid lowered owned scalar Program.
pub fn programs(
    base: &codegen::Program,
) -> Vec<(&'static str, codegen::Program, ProgramValidationError)> {
    let with = |owner: mapping::SourceOwner, parts: &[&str]| {
        let mut program = base.clone();
        for node in &mut program.expressions {
            if node.id == VALUE {
                node.expression =
                    codegen::Expression::OwnedSourceField(mapping::OwnedSourceField {
                        source: OwnedSourceRef {
                            owner: owner.clone(),
                            path: path(parts),
                        },
                        frame: None,
                    });
            }
        }
        program
    };
    let mut dynamic = base.clone();
    dynamic.expressions.push(codegen::ExpressionNode {
        id: 99,
        expression: codegen::Expression::Const {
            value: Value::String("ref.json".into()),
        },
    });
    dynamic.extra_sources[0].dynamic = Some(codegen::DynamicSourceProgram {
        path: 99,
        driver: codegen::SourceIteration::new(path(&["Rows"])),
    });
    let audit = SourceScopeLocation::root(SourceScopeTarget::Named {
        index: 0,
        name: projects::AUDIT.into(),
    });
    vec![
        (
            "D01-unknown-owner",
            with(projects::owner("Missing"), &["Value"]),
            structure(
                VALUE,
                field(
                    Owner::Named {
                        name: "Missing".into(),
                    },
                    &["Value"],
                ),
                Structure::UnknownOwner,
            ),
        ),
        (
            "D02-missing-path",
            with(projects::owner(REF), &["Missing"]),
            structure(
                VALUE,
                field(Owner::Named { name: REF.into() }, &["Missing"]),
                Structure::MissingOwnedPath { segment: 0 },
            ),
        ),
        (
            "D03-group-terminal",
            with(projects::owner("Primary"), &["Ref"]),
            structure(
                VALUE,
                field(Owner::Primary, &["Ref"]),
                Structure::InvalidOwnedField {
                    found: codegen::OwnedSourceSchemaShape::Group { repeating: false },
                },
            ),
        ),
        (
            "D04-dynamic-owner",
            dynamic,
            capability(
                Location::Expression {
                    namespace: Namespace::Main,
                    node: projects::AUDIT_VALUE,
                    scope: Some(audit),
                },
                Some(field(Owner::Named { name: REF.into() }, &["Value"])),
                Reason::DynamicOwner,
                Origin::DynamicSource {
                    index: 0,
                    name: REF.into(),
                },
            ),
        ),
    ]
}
