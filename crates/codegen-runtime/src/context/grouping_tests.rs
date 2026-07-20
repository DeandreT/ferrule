use super::*;
use crate::{GroupedItems, Value, field, group, repeated, scalar};

fn row(key: Value, label: &str) -> Instance {
    group([
        field("Key", scalar(key)),
        field("Label", scalar(Value::String(label.to_string()))),
    ])
}

fn text(value: &str) -> Value {
    Value::String(value.to_string())
}

#[test]
fn exact_keys_keep_first_seen_order_and_expose_members_through_both_views() {
    let source = group([field(
        "Rows",
        repeated([
            row(Value::Int(1), "first-int"),
            row(text("1"), "string"),
            row(Value::Int(1), "second-int"),
        ]),
    )]);
    let candidates = ScopeContext::new(&source)
        .walk_source(&["Rows"])
        .into_iter()
        .map(|candidate| {
            let key = candidate.resolve_scalar(&["Key"])?;
            Ok((candidate, key))
        })
        .collect::<Result<Vec<_>, SourcePathError>>()
        .expect("self-authored row keys resolve");
    let grouped = GroupedItems::by(candidates, Some("Rows"));
    let contexts = grouped.contexts();

    assert_eq!(contexts.len(), 2);
    assert_eq!(
        contexts[0].resolve_scalar(&["Label"]),
        Ok(text("first-int"))
    );
    assert_eq!(contexts[1].resolve_scalar(&["Label"]), Ok(text("string")));
    assert_eq!(contexts[0].position(&["Rows"]), 1);
    assert_eq!(contexts[1].position(&["Rows"]), 2);

    let unnamed_members = contexts[0].aggregate_items(&[]);
    let named_members = contexts[0].aggregate_items(&["Rows"]);
    assert_eq!(unnamed_members.len(), 2);
    assert_eq!(named_members.len(), 2);
    assert_eq!(unnamed_members[1].position(&["Rows"]), 2);
    assert_eq!(
        named_members[1].aggregate_current_scalar(&["Label"]),
        text("second-int")
    );

    let child_members = contexts[0].walk_source(&[]);
    assert_eq!(child_members.len(), 2);
    assert_eq!(child_members[0].position(&["Rows"]), 1);
    assert_eq!(child_members[1].position(&["Rows"]), 2);
    assert_eq!(
        child_members[1].resolve_scalar_in_frame(&["Rows"], &["Label"]),
        Ok(text("second-int"))
    );
}

#[test]
fn contiguous_and_fixed_block_groups_retain_nested_outer_frames() {
    let source = group([field(
        "Orders",
        repeated([group([
            field("Id", scalar(text("O-1"))),
            field(
                "Items",
                group([field(
                    "Row",
                    repeated([
                        row(text("A"), "one"),
                        row(text("A"), "two"),
                        row(text("B"), "three"),
                        row(text("B"), "four"),
                        row(text("C"), "five"),
                    ]),
                )]),
            ),
        ])]),
    )]);
    let order = ScopeContext::new(&source)
        .walk_source(&["Orders"])
        .into_iter()
        .next()
        .expect("one order exists");
    let rows = order.walk_source(&["Items", "Row"]);
    let starts = rows
        .iter()
        .map(|row| {
            row.resolve_scalar(&["Label"])
                .map(|label| label == text("three"))
        })
        .collect::<Result<Vec<_>, _>>()
        .expect("labels resolve");
    let contiguous =
        GroupedItems::starting_with(rows.clone().into_iter().zip(starts).collect(), Some("Row"));
    let groups = contiguous.contexts();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].aggregate_items(&["Row"]).len(), 2);
    assert_eq!(groups[1].aggregate_items(&["Row"]).len(), 3);
    assert_eq!(
        groups[1].resolve_scalar_in_frame(&["Orders"], &["Id"]),
        Ok(text("O-1"))
    );
    assert_eq!(
        groups[1].resolve_scalar_in_frame(&["Orders", "Items", "Row"], &["Label"]),
        Ok(text("three"))
    );

    let blocks =
        GroupedItems::into_blocks(rows, Some("Row"), 2, 42).expect("a positive block size groups");
    assert_eq!(
        blocks
            .contexts()
            .iter()
            .map(|context| context.aggregate_items(&[]).len())
            .collect::<Vec<_>>(),
        [2, 2, 1]
    );
}

#[test]
fn block_grouping_rejects_zero_and_generated_groups_need_no_wrapper() {
    let source = scalar(text("unused"));
    assert!(matches!(
        GroupedItems::into_blocks(Vec::new(), None, 0, 9),
        Err(crate::RuntimeError::InvalidBlockSize { node: 9 })
    ));

    let root = ScopeContext::new(&source);
    let items = GeneratedItems::new(vec![text("a"), text("a"), text("b")]);
    let candidates = root
        .generated_items(&items)
        .into_iter()
        .map(|candidate| {
            let key = candidate.resolve_scalar(&[])?;
            Ok((candidate, key))
        })
        .collect::<Result<Vec<_>, SourcePathError>>()
        .expect("generated scalar keys resolve");
    let grouped = GroupedItems::by(candidates, None);
    let contexts = grouped.contexts();
    assert_eq!(contexts.len(), 2);
    assert_eq!(contexts[0].resolve_scalar(&[]), Ok(text("a")));
    assert_eq!(contexts[0].aggregate_items(&[]).len(), 2);
    assert_eq!(contexts[0].walk_source(&[]).len(), 2);
}
