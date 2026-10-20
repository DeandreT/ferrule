use super::joins::{join_program, set_join};
use super::*;
use crate::{
    AggregateFunction, InnerJoin, JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource,
    JoinSourceCardinality, NamedSourceProgram,
};

fn path(segments: &[&str]) -> Vec<String> {
    segments.iter().map(|segment| (*segment).into()).collect()
}

fn singleton_plan(singleton: &[&str]) -> JoinPlan {
    JoinPlan::new(
        JoinSource::singleton(path(singleton)),
        JoinSource::new(path(&["Catalog", "B"])),
        JoinConditions::new(JoinKey::new(path(singleton), Vec::new(), path(&["aid"]))),
    )
    .unwrap()
}

fn singleton_program() -> Program {
    let mut program = join_program();
    program.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group("Header", vec![SchemaNode::scalar("Code", ScalarType::Int)]),
            SchemaNode::group(
                "A",
                vec![
                    SchemaNode::scalar("id", ScalarType::Int),
                    SchemaNode::scalar("label", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    program.extra_sources.push(NamedSourceProgram {
        name: "Settings".into(),
        source: SchemaNode::group(
            "SettingsDocument",
            vec![SchemaNode::scalar("Code", ScalarType::Int)],
        ),
        dynamic: None,
    });
    program.expressions[0].expression = Expression::JoinField {
        join: JoinId::new(7),
        collection: path(&["Catalog", "B"]),
        path: path(&["label"]),
    };
    program
}

#[test]
fn singleton_join_sources_cannot_cross_inactive_repetition() {
    let mut program = singleton_program();

    set_join(&mut program, singleton_plan(&["Header", "Code"]));
    assert_eq!(validate_program(&program), Ok(()));

    set_join(&mut program, singleton_plan(&["A", "id"]));
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::InvalidJoinSource {
            join: JoinId::new(7),
            collection: path(&["A", "id"]),
            cardinality: JoinSourceCardinality::Singleton,
        })
    );
}

#[test]
fn root_singleton_join_sources_must_resolve_from_the_root_context() {
    let mut program = singleton_program();
    set_join(&mut program, singleton_plan(&["Settings", "Code"]));
    assert_eq!(validate_program(&program), Ok(()));

    // `id` exists only beneath repeated `A`, so the root context cannot read it.
    set_join(&mut program, singleton_plan(&["id"]));
    let unresolved = ProgramValidationError::UnresolvedRootJoinSingleton {
        join: JoinId::new(7),
        collection: path(&["id"]),
    };
    assert_eq!(
        unresolved.to_string(),
        "compiled mapping join 7 singleton source `id` does not resolve from the root source context"
    );
    assert_eq!(validate_program(&program), Err(unresolved));

    // A root-context join aggregate follows the same rule.
    set_join(&mut program, singleton_plan(&["Header", "Code"]));
    program.expressions.push(ExpressionNode {
        id: 5,
        expression: Expression::JoinAggregate {
            function: AggregateFunction::Count,
            join: InnerJoin::new(JoinId::new(8), singleton_plan(&["Code"])),
            expression: None,
            arg: None,
        },
    });
    program.root.bindings.push(Binding {
        target_field: "Count".into(),
        expression: 5,
        target_domain: crate::ScalarTargetDomain::Single(ScalarType::Int),
        repeating: false,
    });
    let ir::SchemaKind::Group { children, .. } = &mut program.target.kind else {
        panic!("join fixture target is a group");
    };
    children.push(SchemaNode::scalar("Count", ScalarType::Int));
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::UnresolvedRootJoinSingleton {
            join: JoinId::new(8),
            collection: path(&["Code"]),
        })
    );

    let Expression::JoinAggregate { join, .. } = &mut program.expressions[4].expression else {
        panic!("join aggregate fixture");
    };
    *join = InnerJoin::new(JoinId::new(8), singleton_plan(&["Header", "Code"]));
    assert_eq!(validate_program(&program), Ok(()));
}
