use super::*;
use crate::{SourcePathError, boolean, field, group, integer, repeated, scalar, string};

fn row(name: &str, selected: Value) -> Instance {
    group([
        field("Name", scalar(string(name))),
        field("Selected", scalar(selected)),
    ])
}

#[test]
fn candidates_flatten_repetitions_and_retain_each_raw_position() {
    let bucket = |rows| group([field("Rows", repeated(rows))]);
    let department = |buckets| group([field("Buckets", repeated(buckets))]);
    let source = group([field(
        "Departments",
        repeated([
            department(vec![bucket(vec![row("first", boolean(false))])]),
            department(vec![
                bucket(vec![
                    row("second", Value::Null),
                    row("third", boolean(true)),
                ]),
                bucket(vec![row("fourth", boolean(true))]),
            ]),
        ]),
    )]);

    let items = ScopeContext::new(&source)
        .collection_find_items(&["Departments", "Buckets", "Rows"])
        .expect("collection exists");

    assert_eq!(items.len(), 4);
    assert_eq!(
        items
            .iter()
            .map(|item| item.resolve_scalar(&["Name"]))
            .collect::<Result<Vec<_>, _>>(),
        Ok(vec![
            string("first"),
            string("second"),
            string("third"),
            string("fourth"),
        ])
    );
    assert_eq!(
        items
            .iter()
            .map(|item| item.position(&["Departments"]))
            .collect::<Vec<_>>(),
        [1, 2, 2, 2]
    );
    assert_eq!(
        items
            .iter()
            .map(|item| item.position(&["Buckets"]))
            .collect::<Vec<_>>(),
        [1, 1, 1, 2]
    );
    assert_eq!(
        items
            .iter()
            .map(|item| item.position(&["Rows"]))
            .collect::<Vec<_>>(),
        [1, 1, 2, 1]
    );
}

#[test]
fn missing_and_empty_roots_follow_collection_find_rules() {
    let source = group([field("Ordinary", group(Vec::new()))]);
    assert!(matches!(
        ScopeContext::new(&source).collection_find_items(&["Missing"]),
        Err(SourcePathError::MissingCollection { path }) if path == ["Missing"]
    ));
    assert!(matches!(
        ScopeContext::new(&source).collection_find_items(&[]),
        Err(SourcePathError::MissingCollection { path }) if path.is_empty()
    ));

    let repeated_root = repeated([scalar(integer(4)), scalar(integer(7))]);
    let items = ScopeContext::new(&repeated_root)
        .collection_find_items(&[])
        .expect("empty path selects a repeated root");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].resolve_scalar(&[]), Ok(integer(4)));
    assert_eq!(items[1].resolve_scalar(&[]), Ok(integer(7)));
    assert_eq!(items[1].position(&[]), 2);

    let documents = Instance::DocumentSet(Vec::new());
    assert!(matches!(
        ScopeContext::new(&documents).collection_find_items(&[]),
        Err(SourcePathError::MissingCollection { path }) if path.is_empty()
    ));
}

#[test]
fn named_inputs_are_valid_collection_find_roots() {
    let source = group(Vec::new());
    let catalog = repeated([
        row("named first", boolean(false)),
        row("named second", boolean(true)),
    ]);
    let inputs = [NamedInput {
        name: "Catalog",
        instance: &catalog,
    }];
    let items = ScopeContext::with_named_inputs(&source, &inputs)
        .collection_find_items(&["Catalog"])
        .expect("named collection exists");

    assert_eq!(items.len(), 2);
    assert_eq!(
        items[1].resolve_scalar(&["Name"]),
        Ok(string("named second"))
    );
    assert_eq!(items[1].position(&["Catalog"]), 2);
}

#[test]
fn nullable_predicates_skip_and_other_scalars_fail() {
    for value in [boolean(true), boolean(false), Value::Null, Value::xml_nil()] {
        assert_eq!(
            crate::collection_find_selected(12, value.clone()),
            Ok(matches!(value, Value::Bool(true)))
        );
    }
    assert_eq!(
        crate::collection_find_selected(12, string("not a bool")),
        Err(crate::RuntimeError::NotABool {
            node: 12,
            found: "string",
        })
    );
}
