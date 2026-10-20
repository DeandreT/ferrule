use super::joins::{join_program, set_join};
use super::*;
use crate::{JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource, JoinSourceCardinality};

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

#[test]
fn singleton_join_sources_cannot_cross_inactive_repetition() {
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
    program.expressions[0].expression = Expression::JoinField {
        join: JoinId::new(7),
        collection: path(&["Catalog", "B"]),
        path: path(&["label"]),
    };

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
