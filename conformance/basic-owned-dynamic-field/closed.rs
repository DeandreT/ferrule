//! Refusal controls: unsupported owned dynamic reads, older generation
//! profiles and directly constructed Programs. Expected errors are authored here.
use super::*;
use codegen::{
    OwnedSourceCapabilityOrigin as Origin, OwnedSourceCapabilityReason as Reason,
    OwnedSourceDiagnostic as Diagnostic, OwnedSourceDiagnosticLocation as Location,
    OwnedSourceDiagnosticOwner as Owner, OwnedSourceDiagnosticRequest as Request,
    OwnedSourceNamespace as Namespace, OwnedSourceSchemaShape as Shape,
    OwnedSourceStructureReason as Structure, ProgramValidationError,
};
use mapping::{OwnedDynamicSourceField, OwnedSourceRef, SourceScopeLocation, SourceScopeTarget};
use projects::{COLOR, FIRST_NODE, LOOKUP, path};

fn named(name: &str) -> Owner {
    Owner::Named { name: name.into() }
}
fn read(owner: Owner, object: &[&str], frame: Option<&[&str]>) -> Request {
    Request::DynamicField {
        owner,
        object: path(object),
        frame: frame.map(path),
    }
}
fn root() -> SourceScopeLocation {
    SourceScopeLocation::root(SourceScopeTarget::Primary)
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
fn expression(node: u32, scope: Option<SourceScopeLocation>) -> Location {
    Location::Expression {
        namespace: Namespace::Main,
        node,
        scope,
    }
}
/// Every earlier profile refuses D02's first Lookup read at its reached node.
pub fn lookup_read_closed() -> ProgramValidationError {
    capability(
        expression(FIRST_NODE, Some(root())),
        Some(read(named(LOOKUP), &["Props"], None)),
        Reason::UnsupportedBasicContext,
        Origin::Expression {
            namespace: Namespace::Main,
            node: FIRST_NODE,
        },
    )
}
fn diagnostic(error: ProgramValidationError) -> codegen::Diagnostic {
    let ProgramValidationError::OwnedSource { diagnostic } = error else {
        panic!("authored owned refusal");
    };
    codegen::Diagnostic::OwnedSource { diagnostic }
}
fn validation(location: &str, message: &str) -> codegen::Diagnostic {
    codegen::Diagnostic::Validation {
        location: location.into(),
        message: message.into(),
    }
}
fn lookup_read(object: &[&str], frame: Option<&[&str]>) -> Node {
    projects::dynamic(LOOKUP, object, frame, COLOR)
}
/// D02 with its first binding replaced by `node`.
fn rebound(node: Node) -> Project {
    let mut p = projects::project("D02-named-object");
    p.graph.nodes.insert(FIRST_NODE, node);
    p
}

/// Projects refused by the owned dynamic-field profile, with complete diagnostics.
pub fn projects() -> Vec<(&'static str, Project, Vec<codegen::Diagnostic>)> {
    let mut result = Vec::new();
    // Failure-rule expressions keep their closed scalar subset.
    let mut p = projects::project("D02-named-object");
    p.graph.nodes.insert(
        60,
        Node::Call {
            function: "exists".into(),
            args: vec![FIRST_NODE],
        },
    );
    p.failure_rules = vec![mapping::FailureRule {
        iteration: mapping::FailureIteration::Source {
            collection: path(&["Rows"]),
        },
        selection: mapping::FailureSelection::WhenTrue { predicate: 60 },
        message: None,
    }];
    result.push((
        "C01-failure-predicate",
        p,
        vec![diagnostic(capability(
            expression(FIRST_NODE, None),
            Some(read(named(LOOKUP), &["Props"], None)),
            Reason::FailureRule,
            Origin::Failure { index: 0 },
        ))],
    ));
    // An isolated function body never reads an endpoint.
    let mut p = projects::project("D02-named-object");
    let function = mapping::FunctionId::new(1);
    let mut body = Graph::default();
    body.nodes.insert(
        1,
        Node::Const {
            value: Value::String("color".into()),
        },
    );
    body.nodes
        .insert(2, projects::dynamic(LOOKUP, &["Props"], None, 1));
    p.user_functions.insert(
        function,
        UserFunction {
            library: "user".into(),
            name: "read".into(),
            description: None,
            parameters: Vec::new(),
            output_name: "result".into(),
            output_type: ScalarType::String,
            body,
            output: 2,
        },
    );
    p.graph.nodes.insert(
        FIRST_NODE,
        Node::UserFunctionCall {
            function,
            args: Vec::new(),
        },
    );
    result.push((
        "C02-function-body",
        p,
        vec![diagnostic(capability(
            Location::Expression {
                namespace: Namespace::Function(function),
                node: 2,
                scope: None,
            },
            Some(read(named(LOOKUP), &["Props"], None)),
            Reason::IsolatedFunction,
            Origin::FunctionBody { function },
        ))],
    ));
    // A legacy dynamic read stays closed in the paired profile.
    let p = rebound(Node::DynamicSourceField {
        object: path(&["Props"]),
        frame: None,
        key: COLOR,
    });
    result.push((
        "C03-legacy-read",
        p,
        vec![diagnostic(capability(
            expression(FIRST_NODE, None),
            None,
            Reason::DynamicLoader,
            Origin::RetainedExpression {
                namespace: Namespace::Main,
                node: FIRST_NODE,
            },
        ))],
    ));
    // Engine validation owns undeclared owners and closed objects.
    let p = rebound(projects::dynamic("Missing", &["Props"], None, COLOR));
    result.push((
        "C04-unknown-owner",
        p,
        vec![validation(
            &format!("graph node {FIRST_NODE}"),
            "unknown source owner `Missing`",
        )],
    ));
    let p = rebound(lookup_read(&["Rows"], None));
    result.push((
        "C05-closed-object",
        p,
        vec![validation(
            &format!("graph node {FIRST_NODE}"),
            "owned dynamic object `Rows` matches no open scalar object in its owner",
        )],
    ));
    let p = rebound(lookup_read(&["Props"], Some(&["Rows"])));
    result.push((
        "C06-frame-outside-object",
        p,
        vec![validation(
            &format!("graph node {FIRST_NODE}"),
            "owned frame `Rows` does not prefix dynamic object `Props`",
        )],
    ));
    // A dynamic named owner stays closed.
    let mut p = projects::project("D02-named-object");
    p.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath {
        node: COLOR,
        iteration: Vec::new(),
    });
    result.push((
        "C07-dynamic-owner",
        p,
        vec![diagnostic(capability(
            expression(FIRST_NODE, Some(root())),
            Some(read(named(LOOKUP), &["Props"], None)),
            Reason::DynamicOwner,
            Origin::DynamicSource {
                index: 0,
                name: LOOKUP.into(),
            },
        ))],
    ));
    result
}

/// Programs constructed directly from a valid lowered owned-read Program.
pub fn programs(
    base: &codegen::Program,
) -> Vec<(&'static str, codegen::Program, ProgramValidationError)> {
    let with = |owner: &str, object: &[&str], frame: Option<&[&str]>| {
        let mut program = base.clone();
        let field = OwnedDynamicSourceField {
            object: OwnedSourceRef {
                owner: projects::owner(owner),
                path: path(object),
            },
            frame: frame.map(path),
            key: COLOR,
        };
        for node in &mut program.expressions {
            if node.id == FIRST_NODE {
                node.expression = codegen::Expression::OwnedDynamicSourceField(field.clone());
            }
        }
        program
    };
    let structure = |owner: Owner, object: &[&str], frame: Option<&[&str]>, reason| {
        ProgramValidationError::OwnedSource {
            diagnostic: Box::new(Diagnostic::Structure {
                location: expression(FIRST_NODE, None),
                request: read(owner, object, frame),
                reason,
            }),
        }
    };
    let mut dynamic = base.clone();
    dynamic.extra_sources[0].dynamic = Some(codegen::DynamicSourceProgram {
        path: COLOR,
        driver: codegen::SourceIteration::new(path(&["Rows"])),
    });
    vec![
        (
            "P01-unknown-owner",
            with("Missing", &["Props"], None),
            structure(named("Missing"), &["Props"], None, Structure::UnknownOwner),
        ),
        (
            "P02-empty-owner",
            with("", &["Props"], None),
            structure(named(""), &["Props"], None, Structure::InvalidNamedOwner),
        ),
        (
            "P03-missing-object",
            with(LOOKUP, &["Missing"], None),
            structure(
                named(LOOKUP),
                &["Missing"],
                None,
                Structure::MissingOwnedPath { segment: 0 },
            ),
        ),
        (
            "P04-closed-object",
            with(LOOKUP, &["Rows"], None),
            structure(
                named(LOOKUP),
                &["Rows"],
                None,
                Structure::InvalidOwnedDynamicObject {
                    found: Shape::Group { repeating: true },
                },
            ),
        ),
        (
            "P05-frame-outside-object",
            with(LOOKUP, &["Props"], Some(&["Rows"])),
            structure(
                named(LOOKUP),
                &["Props"],
                Some(&["Rows"]),
                Structure::IncompatibleFramePath,
            ),
        ),
        (
            "P06-dynamic-owner",
            dynamic,
            capability(
                expression(FIRST_NODE, Some(root())),
                Some(read(named(LOOKUP), &["Props"], None)),
                Reason::DynamicOwner,
                Origin::DynamicSource {
                    index: 0,
                    name: LOOKUP.into(),
                },
            ),
        ),
    ]
}

/// Every generation profile that predates owned dynamic reads.
pub fn old_lower(project: &Project) -> Vec<Result<codegen::Program, codegen::LowerError>> {
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
pub fn old_validators(p: &codegen::Program) -> Vec<Result<(), ProgramValidationError>> {
    vec![
        codegen::validate_program(p),
        codegen::validate_owned_emission(p),
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
pub fn old_catalogues(p: &codegen::Program) -> Vec<Result<(), ProgramValidationError>> {
    [
        codegen::legacy_named_input_admission_plan(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_filters(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_sort(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_windows(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_row_failures(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_by(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_item_at(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_starting_with(p)
            .map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_ending_with(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_failure_sources(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_path_hierarchy(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_adjacency_tree(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_into_blocks(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_recursive_collect(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_exists(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_recursive_filter(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_lookup_find(p).map(drop),
    ]
    .into()
}
pub fn old_rust(
    p: &codegen::Program,
    options: &codegen_rust::Options,
) -> Vec<Result<codegen::ArtifactSet, codegen_rust::EmitError>> {
    vec![
        codegen_rust::emit(p, options),
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
pub fn old_csharp(
    p: &codegen::Program,
) -> Vec<Result<codegen::ArtifactSet, codegen_csharp::EmitError>> {
    vec![
        codegen_csharp::emit(p),
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
