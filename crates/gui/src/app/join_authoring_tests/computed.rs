//! Join plans with a computed key source are read-only: neither key editing
//! nor joined-output authoring is offered for them.

use super::*;
use mapping::ComputedJoinSourceId;

const KEY: ComputedJoinSourceId = ComputedJoinSourceId::new(1);

fn available() -> Vec<Collection> {
    let schema = SchemaNode::group(
        "Source",
        vec![row_schema("Orders", "Code"), row_schema("Products", "Name")],
    );
    let mut choices = Vec::new();
    collections(&schema, &[], &mut choices);
    choices
}

fn scope(plan: JoinPlan) -> Scope {
    Scope {
        target_field: "Rows".into(),
        iteration: ScopeIteration::InnerJoin {
            id: JoinId::new(1),
            plan,
        },
        ..Default::default()
    }
}

fn two_collections() -> JoinPlan {
    JoinPlan::new(
        JoinSource::new(vec!["Orders".into()]),
        JoinSource::new(vec!["Products".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["Orders".into()],
            vec!["Key".into()],
            vec!["Key".into()],
        )),
    )
    .unwrap()
}

#[test]
fn computed_key_plans_offer_no_key_editing() {
    let available = available();
    assert!(editable_keys(&scope(two_collections()), &available).is_some());

    // Two collection sources plus a computed key look like an editable pair
    // and a three-source plan once the computed source is ignored.
    let with_key = two_collections()
        .then(
            JoinSource::computed(KEY, 9),
            JoinConditions::new(JoinKey::new(
                vec!["Orders".into()],
                vec!["Code".into()],
                Vec::new(),
            )),
        )
        .unwrap();
    let key_first = JoinPlan::new(
        JoinSource::computed(KEY, 9),
        JoinSource::new(vec!["Orders".into()]),
        JoinConditions::new(JoinKey::computed(KEY, vec!["Key".into()])),
    )
    .unwrap()
    .then(
        JoinSource::new(vec!["Products".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["Orders".into()],
            vec!["Key".into()],
            vec!["Key".into()],
        )),
    )
    .unwrap();
    for plan in [with_key, key_first] {
        let scope = scope(plan);
        assert!(editable_keys(&scope, &available).is_none());
        assert!(editable_nary_keys(&scope, &available).is_none());
    }
}

#[test]
fn the_scope_editor_lists_computed_key_plans_read_only() {
    let plan = JoinPlan::new(
        JoinSource::new(vec!["Orders".into()]),
        JoinSource::computed(KEY, 9),
        JoinConditions::new(JoinKey::new(
            vec!["Orders".into()],
            vec!["Key".into()],
            Vec::new(),
        )),
    )
    .unwrap();
    assert_eq!(
        crate::scope_editor::computed_join_lines(JoinId::new(1), &plan),
        Some(vec![
            "inner join #1 (2 sources, 1 conditions)".to_string(),
            "input 1: Orders".to_string(),
            "input 2: computed key 1 (node 9)".to_string(),
            "Orders/Key = computed key 1".to_string(),
        ])
    );
    assert_eq!(
        crate::scope_editor::computed_join_lines(JoinId::new(1), &two_collections()),
        None
    );
}
