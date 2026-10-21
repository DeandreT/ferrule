//! Complete typed refusals, distinct from mapping execution cases.
use super::*;
use codegen::{
    OwnedSourceCapabilityOrigin as Origin, OwnedSourceCapabilityReason as Reason,
    OwnedSourceDiagnostic, OwnedSourceDiagnosticLocation as Location,
    OwnedSourceDiagnosticOwner as Owner, OwnedSourceDiagnosticRequest as Request,
    OwnedSourceNamespace::Main,
};
use mapping::{AggregateOp, SourceScopeLocation, SourceScopeStep, SourceScopeTarget};

fn root() -> SourceScopeLocation {
    SourceScopeLocation::root(SourceScopeTarget::Primary)
}
fn child() -> SourceScopeLocation {
    root().descendant(SourceScopeStep::Child(0))
}
fn named_code() -> Request {
    Request::Field {
        owner: Owner::Named {
            name: projects::NAMED.into(),
        },
        path: projects::path(&["Code"]),
        frame: Some(vec![]),
    }
}
fn primary_code() -> Request {
    Request::Field {
        owner: Owner::Primary,
        path: projects::path(&["Code"]),
        frame: Some(vec![]),
    }
}
fn capability(
    node: u32,
    scope: Option<SourceScopeLocation>,
    request: Option<Request>,
    reason: Reason,
    origin: Origin,
) -> OwnedSourceDiagnostic {
    OwnedSourceDiagnostic::Capability {
        location: Location::Expression {
            namespace: Main,
            node,
            scope,
        },
        request,
        reason,
        origin,
    }
}
fn retained(node: u32, reason: Reason) -> OwnedSourceDiagnostic {
    capability(
        node,
        None,
        None,
        reason,
        Origin::RetainedExpression {
            namespace: Main,
            node,
        },
    )
}
fn diagnostic(diagnostic: OwnedSourceDiagnostic) -> Vec<codegen::Diagnostic> {
    vec![codegen::Diagnostic::OwnedSource {
        diagnostic: Box::new(diagnostic),
    }]
}
/// `Value = count(tokenize("x|M|U|U|M|Q|M"), equal(item, Cfg.Code))` at the root.
fn counted() -> Project {
    let mut p = projects::base();
    projects::call(&mut p, 21, "equal", &[90, 101]);
    replace_consumer(&mut p, projects::tokenize(904, 90), Some(21), None, None);
    p.graph.nodes.insert(
        90,
        Node::SourceField {
            path: vec![],
            frame: None,
        },
    );
    p.root.bindings = vec![Binding {
        target_field: "Value".into(),
        node: 10,
    }];
    p
}
fn replace_consumer(
    p: &mut Project,
    sequence: mapping::SequenceExpr,
    predicate: Option<u32>,
    expression: Option<u32>,
    arg: Option<u32>,
) {
    p.graph.nodes.insert(
        10,
        Node::SequenceAggregate {
            function: AggregateOp::Count,
            sequence,
            predicate,
            expression,
            arg,
        },
    );
}
fn recursive(item: u32) -> mapping::SequenceExpr {
    mapping::SequenceExpr::RecursiveCollect {
        collection: projects::path(&["Rows"]),
        children: vec![],
        descent_value: projects::path(&["Code"]),
        values: vec![],
        value: projects::path(&["Label"]),
        prefix: 919,
        separator: 900,
        item,
    }
}
/// The authored refusal of a closed producer reached by the root binding.
pub fn closed_producer(node: u32, request: Request) -> codegen::ProgramValidationError {
    codegen::ProgramValidationError::OwnedSource {
        diagnostic: Box::new(capability(
            node,
            Some(root()),
            Some(request),
            Reason::GeneratedItem,
            Origin::Expression {
                namespace: Main,
                node: 10,
            },
        )),
    }
}
fn pure_function(name: &str, value: Value, ty: ScalarType) -> UserFunction {
    UserFunction {
        library: "test".into(),
        name: name.into(),
        description: None,
        parameters: vec![
            mapping::FunctionParameter {
                id: mapping::FunctionParameterId::new(0),
                name: "Value".into(),
                ty: ScalarType::Int,
            },
            mapping::FunctionParameter {
                id: mapping::FunctionParameterId::new(1),
                name: "Position".into(),
                ty: ScalarType::Int,
            },
        ],
        output_name: "Result".into(),
        output_type: ty,
        body: Graph {
            nodes: BTreeMap::from([(1, Node::Const { value })]),
        },
        output: 1,
    }
}

pub fn projects() -> Vec<(&'static str, Project, Vec<codegen::Diagnostic>)> {
    let mut controls = Vec::new();
    let closed = |error: codegen::ProgramValidationError| match error {
        codegen::ProgramValidationError::OwnedSource { diagnostic } => {
            vec![codegen::Diagnostic::OwnedSource { diagnostic }]
        }
        _ => unreachable!("authored owned refusal"),
    };

    let mut p = counted();
    replace_consumer(&mut p, recursive(90), Some(21), None, None);
    controls.push((
        "recursive-collect-stays-closed",
        p,
        closed(closed_producer(101, named_code())),
    ));

    // The owned recursive walk is qualified only for parent-context ItemAt.
    let mut p = counted();
    replace_consumer(
        &mut p,
        mapping::SequenceExpr::OwnedRecursiveCollect(mapping::OwnedRecursiveCollect {
            collection: mapping::OwnedSourceRef {
                owner: projects::owner("Primary"),
                path: projects::path(&["Rows"]),
            },
            children: vec![],
            descent_value: projects::path(&["Code"]),
            values: vec![],
            value: projects::path(&["Label"]),
            prefix: 919,
            separator: 900,
            item: 90,
        }),
        Some(21),
        None,
        None,
    );
    controls.push((
        "owned-recursive-collect-stays-closed",
        p,
        diagnostic(capability(
            10,
            Some(root()),
            Some(Request::Iteration {
                owner: Owner::Primary,
                path: projects::path(&["Rows"]),
            }),
            Reason::GeneratedItem,
            Origin::Expression {
                namespace: Main,
                node: 10,
            },
        )),
    ));

    let mut p = counted();
    let (predicate, mapper) = (FunctionId::new(7), FunctionId::new(8));
    p.user_functions.insert(
        predicate,
        pure_function("keep", Value::Bool(true), ScalarType::Bool),
    );
    p.user_functions.insert(
        mapper,
        pure_function("map", Value::String("M".into()), ScalarType::String),
    );
    p.graph.nodes.insert(
        95,
        Node::SourceField {
            path: vec![],
            frame: None,
        },
    );
    replace_consumer(
        &mut p,
        mapping::SequenceExpr::FilterMapV1(mapping::FilterMapV1 {
            source: Box::new(mapping::SequenceExpr::Generate {
                from: None,
                to: 910,
                item: 95,
            }),
            item: 90,
            predicate,
            mapper,
            output_type: ScalarType::String,
            captures: vec![],
        }),
        Some(21),
        None,
        None,
    );
    controls.push((
        "filter-map-stays-closed",
        p,
        closed(closed_producer(101, named_code())),
    ));

    // A closed operator in any phase is refused as retained content, even
    // beside an owned read that the profile admits.
    for (id, phase) in [
        ("collection-aggregate-in-predicate", 0),
        ("collection-aggregate-in-expression", 1),
        ("collection-aggregate-in-argument", 2),
    ] {
        let mut p = counted();
        p.graph.nodes.insert(
            60,
            Node::Aggregate {
                function: AggregateOp::Count,
                collection: projects::path(&["Rows"]),
                value: vec![],
                expression: None,
                arg: None,
            },
        );
        projects::call(&mut p, 62, "equal", &[60, 910]);
        projects::call(&mut p, 61, "and", &[62, 21]);
        projects::call(&mut p, 63, "concat", &[90, 60]);
        let (predicate, expression, arg) = match phase {
            0 => (Some(61), None, None),
            1 => (Some(21), Some(63), None),
            _ => (Some(21), None, Some(60)),
        };
        if phase != 0 {
            p.graph.nodes.remove(&61);
            p.graph.nodes.remove(&62);
        }
        if phase != 1 {
            p.graph.nodes.remove(&63);
        }
        replace_consumer(
            &mut p,
            projects::tokenize(904, 90),
            predicate,
            expression,
            arg,
        );
        controls.push((id, p, diagnostic(retained(60, Reason::AggregateLookup))));
    }

    let mut p = counted();
    let function = FunctionId::new(7);
    let mut body = pure_function("map", Value::String("M".into()), ScalarType::String);
    body.parameters.clear();
    p.user_functions.insert(function, body);
    p.graph.nodes.insert(
        60,
        Node::UserFunctionCall {
            function,
            args: vec![],
        },
    );
    replace_consumer(
        &mut p,
        projects::tokenize(904, 90),
        Some(21),
        Some(60),
        None,
    );
    controls.push((
        "user-function-in-expression",
        p,
        diagnostic(OwnedSourceDiagnostic::Capability {
            location: Location::Expression {
                namespace: codegen::OwnedSourceNamespace::Function(function),
                node: 1,
                scope: None,
            },
            request: None,
            reason: Reason::IsolatedFunction,
            origin: Origin::FunctionBody { function },
        }),
    ));

    let mut p = counted();
    projects::call(&mut p, 63, "equal", &[10, 911]);
    p.failure_rules = vec![mapping::FailureRule {
        iteration: mapping::FailureIteration::Source {
            collection: projects::path(&["Rows"]),
        },
        selection: mapping::FailureSelection::WhenTrue { predicate: 63 },
        message: None,
    }];
    p.root.bindings.clear();
    controls.push((
        "failure-rule-predicate-stays-closed",
        p,
        diagnostic(capability(
            101,
            None,
            Some(named_code()),
            Reason::FailureRule,
            Origin::Failure { index: 0 },
        )),
    ));

    let mut p = counted();
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group("Rows", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                .repeating(),
        ],
    );
    p.graph.nodes.insert(
        94,
        Node::SourceField {
            path: vec![],
            frame: None,
        },
    );
    p.root = Scope {
        children: vec![Scope {
            target_field: "Rows".into(),
            iteration: ScopeIteration::Sequence(projects::tokenize(916, 94)),
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 10,
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    controls.push((
        "generated-scope-stays-closed",
        p,
        diagnostic(capability(
            101,
            Some(child()),
            Some(named_code()),
            Reason::GeneratedItem,
            Origin::Scope { scope: child() },
        )),
    ));

    let mut p = projects::project("G02-starting");
    let groups = &mut p.root.children[0];
    groups.group_starting_with = None;
    groups.group_into_blocks = Some(910);
    controls.push((
        "fixed-blocks-stay-closed",
        p,
        diagnostic(OwnedSourceDiagnostic::Capability {
            location: Location::Iteration { scope: child() },
            request: Some(Request::Iteration {
                owner: Owner::Primary,
                path: projects::path(&["Rows"]),
            }),
            reason: Reason::Grouping,
            origin: Origin::Scope { scope: child() },
        }),
    ));
    controls
}

/// Directly constructed Programs widening a lowered consumer.
pub fn programs(
    base: &codegen::Program,
) -> Vec<(
    &'static str,
    codegen::Program,
    codegen::ProgramValidationError,
)> {
    let mut programs = Vec::new();
    let mut program = base.clone();
    for node in &mut program.expressions {
        if node.id == 10
            && let codegen::Expression::SequenceAggregate { sequence, .. } = &mut node.expression
        {
            *sequence = codegen::GeneratedSequence::RecursiveCollect {
                collection: projects::path(&["Rows"]),
                children: vec![],
                descent_value: projects::path(&["Code"]),
                values: vec![],
                value: projects::path(&["Label"]),
                prefix: 900,
                separator: 900,
                item: 90,
            };
        }
    }
    programs.push((
        "direct-recursive-collect",
        program,
        closed_producer(1, primary_code()),
    ));
    let mut program = base.clone();
    program.expressions.push(codegen::ExpressionNode {
        id: 60,
        expression: codegen::Expression::Aggregate {
            function: codegen::AggregateFunction::Count,
            collection: projects::path(&["Rows"]),
            value: codegen::AggregateValue::Path(projects::path(&["Code"])),
            arg: None,
        },
    });
    for node in &mut program.expressions {
        if node.id == 10
            && let codegen::Expression::SequenceAggregate { arg, .. } = &mut node.expression
        {
            *arg = Some(60);
        }
    }
    programs.push((
        "direct-collection-aggregate-argument",
        program,
        codegen::ProgramValidationError::OwnedSource {
            diagnostic: Box::new(retained(60, Reason::AggregateLookup)),
        },
    ));
    programs
}
