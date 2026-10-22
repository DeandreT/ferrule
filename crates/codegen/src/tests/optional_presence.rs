use super::*;
use crate::{IterationOutput, OptionalPresence, TargetScope};

fn project() -> Project {
    let row = SchemaNode::group(
        "Rows",
        vec![typed_scalar("Keep", ScalarType::Bool), scalar("Name")],
    )
    .repeating();
    let item = SchemaNode::group(
        "Item",
        vec![scalar("Name"), typed_scalar("Position", ScalarType::Int)],
    )
    .repeating();
    let field = |name: &str| Node::SourceField {
        path: vec![name.into()],
        frame: Some(vec!["Rows".into()]),
    };
    Project {
        source: SchemaNode::group("Source", vec![row]),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::group("Rows", vec![item]).repeating()],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        user_functions: BTreeMap::new(),
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (1, field("Keep")),
                (2, field("Name")),
                (
                    3,
                    Node::Position {
                        collection: vec!["Rows".into()],
                    },
                ),
                (4, field("Keep")),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Rows".into(),
                iteration: ScopeIteration::Source(vec!["Rows".into()]),
                children: vec![Scope {
                    target_field: "Item".into(),
                    iteration: ScopeIteration::OptionalPresence { condition: 1 },
                    bindings: vec![
                        MappingBinding {
                            target_field: "Name".into(),
                            node: 2,
                        },
                        MappingBinding {
                            target_field: "Position".into(),
                            node: 3,
                        },
                    ],
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn item(program: &mut crate::Program) -> &mut TargetScope {
    &mut program.root.children[0].children[0]
}

#[test]
fn lowers_optional_presence_as_a_parent_context_iteration() {
    let mut program = lower(&project()).expect("optional presence lowers");
    let iteration = item(&mut program).iteration.clone().expect("presence plan");
    assert_eq!(iteration.presence(), Some(OptionalPresence::new(1)));
    assert_eq!(iteration.output(), IterationOutput::Repeated);
    assert!(iteration.filter().is_none() && iteration.windows().is_empty());
    assert!(iteration.roots().any(|node| node == 1));
    assert!(program.expressions.iter().any(|node| node.id == 1));
    // The unused graph node stays out of the generated program.
    assert!(program.expressions.iter().all(|node| node.id != 4));
}

#[test]
fn validation_refuses_controlled_or_dangling_optional_presence() {
    let mut filtered = lower(&project()).expect("optional presence lowers");
    item(&mut filtered).iteration = Some(IterationPlan::new(
        OptionalPresence::new(1),
        Some(1),
        None,
        Vec::new(),
        IterationOutput::Repeated,
    ));
    assert_eq!(
        validate_program(&filtered),
        Err(ProgramValidationError::InvalidOptionalPresence {
            target_path: vec!["Rows".into(), "Item".into()],
        })
    );
    let mut dangling = lower(&project()).expect("optional presence lowers");
    item(&mut dangling).iteration = Some(IterationPlan::optional_presence(42));
    assert_eq!(
        validate_program(&dangling),
        Err(ProgramValidationError::MissingOptionalPresenceExpression {
            target_path: vec!["Rows".into(), "Item".into()],
            expression: 42,
        })
    );
    let mut singular = lower(&project()).expect("optional presence lowers");
    item(&mut singular).repeating = false;
    assert!(validate_program(&singular).is_err());
}
