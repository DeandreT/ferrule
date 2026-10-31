//! Directly constructed programs admit a computed join key only where the
//! interpreter does: one scalar from admitted expression kinds, read without
//! crossing repetition and, at the root, from the root source context.

use super::joins::{join_program, set_join};
use super::*;
use crate::{
    AggregateFunction, ComputedJoinSourceId, InnerJoin, JoinConditions, JoinId, JoinKey, JoinPlan,
    JoinPlanError, JoinSource, NamedSourceProgram,
};

const KEY: ComputedJoinSourceId = ComputedJoinSourceId::new(1);
const KEY_NODE: NodeId = 10;

fn path(segments: &[&str]) -> Vec<String> {
    segments.iter().map(|segment| (*segment).into()).collect()
}

fn computed_plan(key_first: bool) -> JoinPlan {
    let catalog = JoinSource::new(path(&["Catalog", "B"]));
    if key_first {
        JoinPlan::new(
            JoinSource::computed(KEY, KEY_NODE),
            catalog,
            JoinConditions::new(JoinKey::computed(KEY, path(&["aid"]))),
        )
    } else {
        JoinPlan::new(
            catalog,
            JoinSource::computed(KEY, KEY_NODE),
            JoinConditions::new(JoinKey::new(
                path(&["Catalog", "B"]),
                path(&["aid"]),
                Vec::new(),
            )),
        )
    }
    .expect("a computed two-source plan")
}

/// The join fixture reading only the catalog, plus a root `Header/Code`, a
/// named `Settings/Code`, and the key node.
fn computed_program(key: Expression) -> Program {
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
    program.expressions.push(ExpressionNode {
        id: KEY_NODE,
        expression: key,
    });
    set_join(&mut program, computed_plan(true));
    program
}

fn field(segments: &[&str]) -> Expression {
    Expression::SourceField {
        frame: None,
        path: path(segments),
    }
}

fn set_key(program: &mut Program, key: Expression) {
    let node = program
        .expressions
        .iter_mut()
        .find(|node| node.id == KEY_NODE)
        .expect("key node");
    node.expression = key;
}

#[test]
fn computed_join_plans_follow_mapping_plan_rules() {
    assert_eq!(
        JoinPlan::new(
            JoinSource::computed(KEY, 1),
            JoinSource::computed(ComputedJoinSourceId::new(2), 2),
            JoinConditions::new(JoinKey::computed(KEY, Vec::new())),
        ),
        Err(JoinPlanError::NoCollectionSource)
    );
    let second = computed_plan(true).then(
        JoinSource::computed(ComputedJoinSourceId::new(2), 2),
        JoinConditions::new(JoinKey::computed(KEY, Vec::new())),
    );
    assert_eq!(
        second,
        Err(JoinPlanError::MultipleComputedSources(
            ComputedJoinSourceId::new(2)
        ))
    );
    assert_eq!(
        JoinPlan::new(
            JoinSource::new(path(&["A"])),
            JoinSource::computed(KEY, 1),
            JoinConditions::new(JoinKey::new(path(&["A"]), path(&["id"]), path(&["id"]))),
        ),
        Err(JoinPlanError::ComputedRightPath {
            source: KEY,
            right_path: path(&["id"]),
        })
    );
    assert_eq!(
        JoinPlan::new(
            JoinSource::new(path(&["A"])),
            JoinSource::new(path(&["Catalog", "B"])),
            JoinConditions::new(JoinKey::computed(KEY, path(&["aid"]))),
        ),
        Err(JoinPlanError::UnknownLeftComputed(KEY))
    );
    let plan = computed_plan(false);
    assert_eq!(plan.collection_sources().count(), 1);
    assert_eq!(
        plan.computed_source().map(|key| key.value()),
        Some(KEY_NODE)
    );
}

#[test]
fn computed_keys_admit_scalar_expressions_from_either_side() {
    for key_first in [true, false] {
        for key in [
            Expression::Const {
                value: Value::String("C2".into()),
            },
            field(&["Header", "Code"]),
            field(&["Settings", "Code"]),
        ] {
            let mut program = computed_program(key);
            set_join(&mut program, computed_plan(key_first));
            assert_eq!(validate_program(&program), Ok(()));
        }
    }
    // Calls, conditionals and value maps are walked to their reads.
    let mut program = computed_program(Expression::Call {
        function: crate::ScalarFunction::Upper,
        args: vec![11],
    });
    program.expressions.push(ExpressionNode {
        id: 11,
        expression: field(&["Header", "Code"]),
    });
    assert_eq!(validate_program(&program), Ok(()));
    program.expressions.last_mut().expect("read").expression = field(&["A", "id"]);
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::ComputedJoinKeyCrossesRepetition {
            join: JoinId::new(7),
            computed: KEY,
            node: 11,
            path: path(&["A", "id"]),
        })
    );
}

#[test]
fn computed_keys_refuse_what_the_interpreter_refuses() {
    let mut program = computed_program(field(&["A", "id"]));
    let crossing = ProgramValidationError::ComputedJoinKeyCrossesRepetition {
        join: JoinId::new(7),
        computed: KEY,
        node: KEY_NODE,
        path: path(&["A", "id"]),
    };
    assert_eq!(
        crossing.to_string(),
        "compiled mapping join 7 computed key 1 source field 10 `A/id` crosses or ends at a repeating source level"
    );
    assert_eq!(validate_program(&program), Err(crossing));

    // `id` exists only beneath repeated `A`, so the root context cannot read it.
    set_key(&mut program, field(&["id"]));
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::UnresolvedRootComputedJoinKey {
            join: JoinId::new(7),
            computed: KEY,
            node: KEY_NODE,
            path: path(&["id"]),
        })
    );

    for refused in [
        Expression::SourceField {
            frame: Some(path(&["A"])),
            path: path(&["id"]),
        },
        Expression::Aggregate {
            function: AggregateFunction::Count,
            collection: path(&["A"]),
            value: crate::AggregateValue::Path(Vec::new()),
            arg: None,
        },
        Expression::JoinPosition {
            join: JoinId::new(7),
        },
    ] {
        set_key(&mut program, refused);
        assert_eq!(
            validate_program(&program),
            Err(ProgramValidationError::InvalidComputedJoinKeyNode {
                join: JoinId::new(7),
                computed: KEY,
                node: KEY_NODE,
            })
        );
    }

    program.expressions.retain(|node| node.id != KEY_NODE);
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::MissingComputedJoinKey {
            join: JoinId::new(7),
            computed: KEY,
            expression: KEY_NODE,
        })
    );
}

#[test]
fn joined_aggregates_validate_their_computed_key_in_the_receiving_context() {
    let mut program = computed_program(field(&["Header", "Code"]));
    program.expressions.push(ExpressionNode {
        id: 5,
        expression: Expression::JoinAggregate {
            function: AggregateFunction::Count,
            join: InnerJoin::new(JoinId::new(8), computed_plan(false)),
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
    assert_eq!(validate_program(&program), Ok(()));

    // The aggregate is checked before the scope that shares its key.
    set_key(&mut program, field(&["id"]));
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::UnresolvedRootComputedJoinKey {
            join: JoinId::new(8),
            computed: KEY,
            node: KEY_NODE,
            path: path(&["id"]),
        })
    );
}

/// Inside an `A` iteration the key reads the current item, which anchors the
/// join there like a current-item singleton.
#[test]
fn nested_computed_keys_anchor_their_join_in_the_current_item() {
    let mut program = computed_program(field(&["label"]));
    let ir::SchemaKind::Group { children, .. } = &mut program.target.kind else {
        panic!("join fixture target is a group");
    };
    let row = children.remove(0);
    children.push(SchemaNode::group("Item", vec![row]).repeating());
    let join_scope = program.root.children.remove(0);
    program.root.children.push(TargetScope {
        target_field: "Item".into(),
        repeating: true,
        iteration: Some(IterationPlan::new(
            SourceIteration::new(path(&["A"])),
            None,
            None,
            Vec::new(),
            IterationOutput::Repeated,
        )),
        construction: TargetConstruction::Group,
        bindings: Vec::new(),
        children: vec![join_scope],
    });
    for key_first in [true, false] {
        program.root.children[0].children[0].iteration = Some(IterationPlan::join(InnerJoin::new(
            JoinId::new(7),
            computed_plan(key_first),
        )));
        assert_eq!(validate_program(&program), Ok(()));
    }
    // A key crossing the repeated collection stays refused when nested.
    set_key(&mut program, field(&["A", "label"]));
    assert!(matches!(
        validate_program(&program),
        Err(ProgramValidationError::ComputedJoinKeyCrossesRepetition { node: KEY_NODE, .. })
    ));
}
