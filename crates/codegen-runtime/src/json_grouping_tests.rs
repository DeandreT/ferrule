use super::*;
use crate::{JsonSourceAcquisition, JsonSourceDocument, project_json_source_document};

const SHAPE: &str = r#"{"kind":"object","properties":[{"name":"rows","shape":{"kind":"array","item":{"kind":"object","properties":[{"name":"key","shape":{"kind":"string"}},{"name":"payload","shape":{"kind":"string"}},{"name":"__group_key","shape":{"kind":"string"}},{"name":"__group_member","shape":{"kind":"string"}}]}}}]}"#;
const PLAN: &str = r#"{"collection":["Root","rows","Items"],"keys":["key"]}"#;

fn fixture(text: &str) -> (JsonGroupingPreparation, JsonSourceShape, Instance) {
    let plan = parse_json_grouping_preparation(PLAN).unwrap();
    let shape = crate::parse_json_projection_shape(SHAPE).unwrap();
    let raw = JsonSourceDocument::parse("doc", text.as_bytes()).unwrap();
    let projected = project_json_source_document(&shape, &raw, Default::default()).unwrap();
    (plan, shape, projected)
}

fn members(document: &Instance) -> &[Instance] {
    document.field("Root").unwrap().as_repeated().unwrap()[0]
        .field("rows")
        .unwrap()
        .as_repeated()
        .unwrap()[0]
        .field("Items")
        .unwrap()
        .as_repeated()
        .unwrap()
}

#[test]
fn distinct_keys_preserve_original_members_and_first_seen_order() {
    let (plan, shape, original) = fixture(
        r#"{"rows":[{"key":"B","key":"A","key":"B","key":"","payload":"é😀","__group_key":"original-key","__group_member":"original-member"},{"key":"A"},{"key":null},{}]}"#,
    );
    let expected = original.clone();
    let prepared =
        prepare_json_grouping_document(&plan, &shape, original, Default::default()).unwrap();
    let actual = members(&prepared);
    assert_eq!(actual.len(), 4);
    for (index, key) in ["B", "A", "", "A"].iter().enumerate() {
        assert_eq!(
            actual[index].field(JsonGroupingPreparation::KEY),
            Some(&Instance::Scalar(Value::String((*key).into())))
        );
        assert_eq!(
            actual[index].field(JsonGroupingPreparation::MEMBER),
            Some(&members(&expected)[usize::from(index == 3)])
        );
    }
    assert_eq!(prepared.field("Path"), expected.field("Path"));
}

#[test]
fn wrong_root_and_empty_members_produce_no_candidates() {
    for text in [
        "null",
        "[]",
        "{}",
        r#"{"rows":[]}"#,
        r#"{"rows":[null,false,{"key":null},{}]}"#,
    ] {
        let (plan, shape, original) = fixture(text);
        let prepared =
            prepare_json_grouping_document(&plan, &shape, original, Default::default()).unwrap();
        assert!(!format!("{prepared:?}").contains("__group_member\": Group"));
    }
}

#[test]
fn exact_logical_node_and_utf8_name_budgets_include_input_and_copies() {
    let shape = crate::parse_json_projection_shape(r#"{"kind":"object","properties":[{"name":"rows","shape":{"kind":"array","item":{"kind":"object","properties":[{"name":"key","shape":{"kind":"string"}}]}}}]}"#).unwrap();
    let plan = parse_json_grouping_preparation(PLAN).unwrap();
    let raw = JsonSourceDocument::parse("doc", br#"{"rows":[{"key":"A"}]}"#).unwrap();
    let original = project_json_source_document(&shape, &raw, Default::default()).unwrap();
    let exact = JsonProjectionLimits {
        values: 17,
        name_bytes: 58,
    };
    prepare_json_grouping_document(&plan, &shape, original.clone(), exact).unwrap();
    assert!(matches!(
        prepare_json_grouping_document(
            &plan,
            &shape,
            original.clone(),
            JsonProjectionLimits {
                values: 16,
                ..exact
            }
        ),
        Err(JsonGroupingError::Values)
    ));
    assert!(matches!(
        prepare_json_grouping_document(
            &plan,
            &shape,
            original.clone(),
            JsonProjectionLimits {
                name_bytes: 57,
                ..exact
            }
        ),
        Err(JsonGroupingError::Names)
    ));
    assert!(matches!(
        prepare_json_grouping_document(
            &plan,
            &shape,
            original,
            JsonProjectionLimits {
                values: usize::MAX,
                ..exact
            }
        ),
        Err(JsonGroupingError::InvalidLimits)
    ));
    let mut budget = Budget {
        values: 1,
        names: 6,
        text: 6,
    };
    budget.name("é😀").unwrap();
    budget.text("é😀").unwrap();
    assert!(matches!(budget.name("x"), Err(JsonGroupingError::Names)));
    assert!(matches!(budget.text("x"), Err(JsonGroupingError::Text)));
}

#[test]
fn copied_text_is_bounded_before_materialization() {
    let text = format!(
        r#"{{"rows":[{{"key":"A","key":"B","payload":"{}"}}]}}"#,
        "x".repeat(22 * 1024 * 1024)
    );
    let (plan, shape, original) = fixture(&text);
    assert!(matches!(
        prepare_json_grouping_document(&plan, &shape, original, Default::default()),
        Err(JsonGroupingError::Text)
    ));
}

#[test]
fn malformed_descriptors_and_runtime_shapes_fail_closed() {
    for text in [
        "null",
        "{}",
        r#"{"collection":["Root"],"keys":[]}"#,
        r#"{"collection":["Elsewhere"],"keys":["key"]}"#,
        r#"{"collection":["Root"],"keys":["\u0000"]}"#,
        r#"{"collection":["Root"],"collection":["Root"],"keys":["key"]}"#,
        r#"{"collection":["Root"],"keys":["key"],"extra":0}"#,
    ] {
        assert!(parse_json_grouping_preparation(text).is_err(), "{text}");
    }
    let (plan, shape, original) = fixture(r#"{"rows":[{"key":"A"}]}"#);
    let mut fields = match original.clone() {
        Instance::Group(fields) => fields,
        _ => unreachable!(),
    };
    fields.push(("Root".into(), Instance::Repeated(vec![])));
    assert!(matches!(
        prepare_json_grouping_document(&plan, &shape, Instance::Group(fields), Default::default()),
        Err(JsonGroupingError::Shape)
    ));
    let duplicate = Instance::Group(
        vec![
            ("Value".into(), Instance::Scalar(Value::String("A".into()))),
            ("Value".into(), Instance::Scalar(Value::String("B".into()))),
        ]
        .into(),
    );
    assert!(matches!(
        select_keys(&duplicate, &[], &mut vec![]),
        Err(JsonGroupingError::Shape)
    ));
}

#[test]
fn acquisition_preparation_failure_is_terminal_and_does_not_read_next_input() {
    let (plan, shape, _) = fixture("{}");
    let loaded = std::cell::RefCell::new(Vec::new());
    let mut acquisition = JsonSourceAcquisition::new(
        || Ok::<_, String>("*.json".into()),
        |_| Ok([Ok("first".into()), Ok("second".into())].into_iter()),
        |path| {
            loaded.borrow_mut().push(path.to_owned());
            JsonSourceDocument::parse(path, br#"{"rows":[{"key":"A"}]}"#)
                .map_err(|error| error.to_string())
        },
        &shape,
        JsonProjectionLimits {
            values: 16,
            name_bytes: 1024,
        },
        2,
    )
    .unwrap()
    .with_grouping_preparation(&plan)
    .unwrap();
    assert!(matches!(
        acquisition.next(),
        Some(Err(crate::JsonAcquisitionError::Grouping(_)))
    ));
    assert!(acquisition.next().is_none());
    assert_eq!(&*loaded.borrow(), &["first"]);
}
