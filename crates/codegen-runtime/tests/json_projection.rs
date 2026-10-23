use codegen_runtime::{
    JsonAcquisitionError, JsonProjectionError, JsonProjectionLimits, JsonSourceAcquisition,
    JsonSourceDocument, project_json_source_document,
};
use ir::{Instance, Value};
use mapping::{JsonProjectionPlanError, JsonSourceProperty, JsonSourceShape};
use serde_json::{Value as Json, json};

fn view(value: &Instance) -> Json {
    match value {
        Instance::Scalar(Value::String(text)) => json!(text),
        Instance::Scalar(Value::Bool(value)) => json!(value),
        Instance::Group(fields) => Json::Object(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), view(value)))
                .collect(),
        ),
        Instance::Repeated(values) => Json::Array(values.iter().map(view).collect()),
        _ => panic!("unexpected projection value"),
    }
}

#[test]
fn complete_typed_projection_retains_duplicates_and_lexicals() {
    let corpus: Json = serde_json::from_str(include_str!(
        "../../../conformance/json-source/projection.json"
    ))
    .unwrap();
    let shape: JsonSourceShape = serde_json::from_value(corpus["shape"].clone()).unwrap();
    for case in corpus["cases"].as_array().unwrap() {
        let document =
            JsonSourceDocument::parse("doc.json", case["input"].as_str().unwrap().as_bytes())
                .unwrap();
        let value = project_json_source_document(&shape, &document, Default::default()).unwrap();
        assert_eq!(view(&value), case["expected"]);
        let schema = shape.document_schema().unwrap();
        assert!(schema.child("Path").is_some());
        assert!(schema.child("Root").is_some());
    }
}

#[test]
fn projection_has_independent_complete_output_limits() {
    let shape = JsonSourceShape::Number;
    let document = JsonSourceDocument::parse("unchanged", b"1e999").unwrap();
    let exact = JsonProjectionLimits {
        values: 5,
        name_bytes: 13,
    };
    assert!(project_json_source_document(&shape, &document, exact).is_ok());
    assert_eq!(
        project_json_source_document(
            &shape,
            &document,
            JsonProjectionLimits { values: 4, ..exact }
        ),
        Err(JsonProjectionError::Values)
    );
    assert_eq!(
        project_json_source_document(
            &shape,
            &document,
            JsonProjectionLimits {
                name_bytes: 12,
                ..exact
            }
        ),
        Err(JsonProjectionError::Names)
    );
    assert_eq!(document.root().number_text(), Some("1e999"));
    assert_eq!(
        project_json_source_document(
            &shape,
            &document,
            JsonProjectionLimits {
                values: 1_000_001,
                ..exact
            }
        ),
        Err(JsonProjectionError::InvalidLimits)
    );
}

#[test]
fn invalid_plans_refuse_before_projection() {
    let duplicate = JsonSourceShape::Object {
        properties: vec![
            JsonSourceProperty {
                name: "same".into(),
                shape: JsonSourceShape::String,
            },
            JsonSourceProperty {
                name: "same".into(),
                shape: JsonSourceShape::Number,
            },
        ],
    };
    assert_eq!(
        duplicate.validate(),
        Err(JsonProjectionPlanError::DuplicateProperty)
    );
    let invalid = JsonSourceShape::Object {
        properties: vec![JsonSourceProperty {
            name: "".into(),
            shape: JsonSourceShape::String,
        }],
    };
    assert_eq!(
        invalid.validate(),
        Err(JsonProjectionPlanError::InvalidName)
    );
    let mut deep = JsonSourceShape::String;
    for _ in 0..25 {
        deep = JsonSourceShape::Array {
            item: Box::new(deep),
        };
    }
    assert_eq!(deep.validate(), Err(JsonProjectionPlanError::Depth));
    assert!(serde_json::from_str::<JsonSourceShape>(r#"{"kind":"number","coerce":true}"#).is_err());
}

#[test]
fn acquisition_is_lazy_owned_and_terminal_after_a_reached_error() {
    use std::{cell::RefCell, rc::Rc};
    let log = Rc::new(RefCell::new(Vec::new()));
    let shape = JsonSourceShape::Number;
    let mut source = JsonSourceAcquisition::new(
        {
            let log = log.clone();
            move || {
                log.borrow_mut().push("filename");
                Ok::<_, &str>("input/*.json".into())
            }
        },
        {
            let log = log.clone();
            move |path| {
                assert_eq!(path, "input/*.json");
                log.borrow_mut().push("select");
                Ok(vec![Ok("a".into()), Err("request"), Ok("unreached".into())].into_iter())
            }
        },
        {
            let log = log.clone();
            move |path| {
                assert_eq!(path, "a");
                log.borrow_mut().push("load");
                Ok(JsonSourceDocument::parse("actual/a.json", b"1e999").unwrap())
            }
        },
        &shape,
        Default::default(),
        3,
    )
    .unwrap();
    assert!(log.borrow().is_empty());
    let first = source.next().unwrap().unwrap();
    assert_eq!(&*log.borrow(), &["filename", "select", "load"]);
    assert!(matches!(
        source.next(),
        Some(Err(JsonAcquisitionError::Request("request")))
    ));
    assert!(source.next().is_none());
    assert_eq!(&*log.borrow(), &["filename", "select", "load"]);
    assert_eq!(
        view(&first),
        json!({"Path":"actual/a.json","Root":[{"Value":"1e999"}]})
    );
}

#[test]
fn acquisition_limits_and_panics_do_not_reenter_the_loader() {
    let shape = JsonSourceShape::Null;
    let mut zero = JsonSourceAcquisition::new(
        || Ok::<_, &str>("zero".into()),
        |_| Ok::<_, &str>(vec![Ok("zero".into())].into_iter()),
        |_| panic!("limit must precede load"),
        &shape,
        Default::default(),
        0,
    )
    .unwrap();
    assert!(matches!(
        zero.next(),
        Some(Err(JsonAcquisitionError::Documents))
    ));
    assert!(zero.next().is_none());
    let mut panic = JsonSourceAcquisition::new(
        || Ok::<_, &str>("panic".into()),
        |_| Ok::<_, &str>(vec![Ok("panic".into())].into_iter()),
        |_| panic!("host"),
        &shape,
        Default::default(),
        1,
    )
    .unwrap();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| panic.next())).is_err());
    assert!(panic.next().is_none());
}
