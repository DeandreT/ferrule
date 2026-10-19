use std::cell::{Cell, RefCell};

use codegen_runtime::{
    JsonSourceDocument, JsonSourceDocuments, JsonSourceKind, JsonSourceLoadError,
    JsonSourceOccurrence, MAX_JSON_SOURCE_DOCUMENTS, Value, field, group, repeated, scalar,
    serialize_json_compact,
};

fn document(text: &str) -> JsonSourceDocument {
    JsonSourceDocument::parse("same.json", text.as_bytes()).unwrap()
}

#[test]
fn complete_literal_document_uses_exact_parents_and_all_matching_members() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../conformance/json-source/occurrences.json"
    ))
    .unwrap();
    let inputs = fixture["documents"].as_array().unwrap();
    let calls = RefCell::new(Vec::new());
    let requests = inputs
        .iter()
        .map(|input| Ok::<_, String>(input["request"].as_str().unwrap().to_owned()));
    let loaded = JsonSourceDocuments::new(
        requests,
        |request| {
            calls.borrow_mut().push(request.to_owned());
            let input = inputs
                .iter()
                .find(|input| input["request"] == request)
                .unwrap();
            JsonSourceDocument::parse(
                input["identity"].as_str().unwrap(),
                input["input"].as_str().unwrap().as_bytes(),
            )
            .map_err(|error| error.to_string())
        },
        3,
    )
    .unwrap();
    assert!(calls.borrow().is_empty());
    let mut documents = Vec::new();
    let mut retained = Vec::new();
    for root in loaded {
        let root = root.unwrap();
        let mut rows = Vec::new();
        for array in root.members_named("rows") {
            for row in array.items() {
                for value in row.members_named("value") {
                    let Some(value) = value.of_kind(JsonSourceKind::String) else {
                        continue;
                    };
                    let owner = value.parent().unwrap();
                    assert!(owner.same_occurrence(&row));
                    let name = owner.members_named("owner").next().unwrap();
                    rows.push(group([
                        field(
                            "owner",
                            scalar(Value::String(name.value().as_str().unwrap().into())),
                        ),
                        field(
                            "value",
                            scalar(Value::String(value.value().as_str().unwrap().into())),
                        ),
                    ]));
                    retained.push(value);
                }
            }
        }
        documents.push(group([
            field(
                "identity",
                scalar(Value::String(root.document().identity().into())),
            ),
            field("rows", repeated(rows)),
        ]));
    }
    let output = group([field("documents", repeated(documents))]);
    assert_eq!(
        serialize_json_compact(&fixture["schema"].to_string(), &output).unwrap(),
        fixture["expected"].as_str().unwrap()
    );
    assert_eq!(*calls.borrow(), ["z-first", "a-second", "last"]);
    // A later load and dropping the iterator cannot retire a retained parent.
    assert_eq!(retained[0].value().as_str(), Some("a"));
    assert!(!retained[0].same_occurrence(&retained[4]));
    assert_eq!(
        retained[0].document().identity(),
        retained[4].document().identity()
    );
}

#[test]
fn equal_labels_equal_values_and_reselection_do_not_confuse_occurrences() {
    let parsed = document(r#"{"x":"same","x":"same","a":["same","same"]}"#);
    let first = JsonSourceOccurrence::new(parsed.clone());
    let second = JsonSourceOccurrence::new(parsed);
    assert!(!first.same_occurrence(&second));
    assert!(first.parent().is_none());
    let values: Vec<_> = first.members_named("x").collect();
    assert!(!values[0].same_occurrence(&values[1]));
    assert!(values[0].same_occurrence(&first.members_named("x").next().unwrap()));
    assert!(values[0].same_occurrence(&values[0].of_kind(JsonSourceKind::String).unwrap()));
    assert!(values[0].of_kind(JsonSourceKind::Number).is_none());
    assert!(values[0].parent().unwrap().same_occurrence(&first));
    assert_eq!(values[0].items().count(), 0);
    assert_eq!(first.members_named("X").count(), 0);
    let array = first.members_named("a").next().unwrap();
    let items: Vec<_> = array.items().collect();
    assert!(!items[0].same_occurrence(&items[1]));
    assert!(items[1].parent().unwrap().same_occurrence(&array));
    assert_eq!(array.members_named("x").count(), 0);
    let loaded = JsonSourceDocuments::new(
        [Ok::<_, &str>("same".into()), Ok("same".into())].into_iter(),
        |_| Ok(first.document().clone()),
        2,
    )
    .unwrap()
    .collect::<Result<Vec<_>, _>>()
    .unwrap();
    assert!(!loaded[0].same_occurrence(&loaded[1]));
}

#[test]
fn reached_loading_stops_permanently_on_host_or_enumeration_error() {
    for failure in ["syntax", "validation", "cleanup", "enumeration"] {
        let events = RefCell::new(Vec::new());
        let requests = ["good", failure, "unreached"].into_iter().map(|request| {
            events.borrow_mut().push(format!("enumerate:{request}"));
            if request == "enumeration" {
                Err(request.to_owned())
            } else {
                Ok(request.to_owned())
            }
        });
        let mut loaded = JsonSourceDocuments::new(
            requests,
            |request| {
                events.borrow_mut().push(format!("load:{request}"));
                let parsed = if request == "syntax" {
                    JsonSourceDocument::parse("same.json", b"{").map_err(|_| "syntax".to_owned())?
                } else {
                    document("null")
                };
                if request != "good" {
                    return Err(request.to_owned());
                }
                events.borrow_mut().push("ready:good".into());
                Ok(parsed)
            },
            3,
        )
        .unwrap();
        assert!(events.borrow().is_empty());
        assert_eq!(
            loaded.next().unwrap().unwrap().value().kind(),
            JsonSourceKind::Null
        );
        assert_eq!(
            &*events.borrow(),
            &["enumerate:good", "load:good", "ready:good"]
        );
        assert!(
            matches!(loaded.next(), Some(Err(JsonSourceLoadError::Host(error))) if error == failure)
        );
        let after_error = events.borrow().clone();
        assert!(loaded.next().is_none());
        assert!(loaded.next().is_none());
        assert_eq!(*events.borrow(), after_error);
        let mut expected = vec![
            "enumerate:good".to_owned(),
            "load:good".into(),
            "ready:good".into(),
            format!("enumerate:{failure}"),
        ];
        if failure != "enumeration" {
            expected.push(format!("load:{failure}"));
        }
        assert_eq!(*events.borrow(), expected);
    }
}

#[test]
fn load_count_limits_and_early_stop_do_not_load_extra_documents() {
    let calls = Cell::new(0);
    let load = |_: &str| {
        calls.set(calls.get() + 1);
        Ok::<_, &str>(document("null"))
    };
    let requests = || [Ok("a".into()), Ok("b".into())].into_iter();
    let mut limited = JsonSourceDocuments::new(requests(), load, 1).unwrap();
    assert!(limited.next().unwrap().is_ok());
    assert!(matches!(
        limited.next(),
        Some(Err(JsonSourceLoadError::Documents { maximum: 1 }))
    ));
    assert!(limited.next().is_none());
    assert_eq!(calls.get(), 1);
    let mut zero = JsonSourceDocuments::new(requests(), load, 0).unwrap();
    assert!(matches!(
        zero.next(),
        Some(Err(JsonSourceLoadError::Documents { maximum: 0 }))
    ));
    assert_eq!(calls.get(), 1);
    assert!(
        JsonSourceDocuments::new(std::iter::empty(), load, 0)
            .unwrap()
            .next()
            .is_none()
    );
    assert!(matches!(
        JsonSourceDocuments::new(requests(), load, MAX_JSON_SOURCE_DOCUMENTS + 1),
        Err(JsonSourceLoadError::InvalidLimit)
    ));
    assert_eq!(calls.get(), 1);
    let mut early = JsonSourceDocuments::new(requests(), load, 2).unwrap();
    let retained = early.next().unwrap().unwrap();
    drop(early);
    assert_eq!(calls.get(), 2);
    assert_eq!(retained.document().identity(), "same.json");
    let mut exact = JsonSourceDocuments::new(requests(), load, 2).unwrap();
    assert!(exact.next().unwrap().is_ok());
    assert!(exact.next().unwrap().is_ok());
    assert!(exact.next().is_none());
    assert_eq!(calls.get(), 4);
}
