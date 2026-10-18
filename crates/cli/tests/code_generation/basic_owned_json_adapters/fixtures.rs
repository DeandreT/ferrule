//! Frozen before the new JSON adapters: genuine Projects, not constructed Programs.
use super::*;
use mapping::{NamedSource, NamedTarget, OwnedSourceField, OwnedSourceRef, SourceOwner};

pub(super) const UNICODE: &str = "résumé📦";
pub(super) const MODELS: usize = 3;
pub(super) const CASES: usize = 38;
pub(super) const PORTABLE_CALLS: usize = 101;
pub(super) const CSHARP_ONLY_CALLS: usize = 12;

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn owned(owner: SourceOwner, parts: &[&str], frame: Option<&[&str]>) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame: frame.map(path),
    })
}
fn scope(node: u32) -> Scope {
    Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node,
        }],
        ..Default::default()
    }
}
fn output(repeated: bool) -> SchemaNode {
    let value = string("Value")
        .with_string_length_range(ir::StringLengthRange::new(1, Some(8)).unwrap())
        .unwrap();
    let schema = SchemaNode::group("Output", vec![value]);
    if repeated { schema.repeating() } else { schema }
}
pub(super) fn projects() -> Vec<Project> {
    let reference = SourceOwner::Named { name: "Ref".into() };
    let rows = SchemaNode::group("Rows", vec![string("Code")])
        .repeating()
        .with_item_count_range(ir::ItemCountRange::new(0, Some(2)).unwrap())
        .unwrap();
    let project = Project {
        source: SchemaNode::group(
            "Input",
            vec![
                string("Code"),
                bool_("Fail"),
                int("Bound")
                    .with_numeric_range(ir::NumericRange::Integer(
                        ir::IntegerRange::new(Some(0), Some(2)).unwrap(),
                    ))
                    .unwrap(),
                // Deliberate legacy-alias shadow: owned Ref must never read this.
                SchemaNode::group("Ref", vec![string("Code")]),
            ],
        )
        .with_required_fields(vec!["Code".into()])
        .unwrap(),
        target: output(false),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![
            NamedSource {
                name: "Ref".into(),
                path: "unused/Ref.json".into(),
                schema: SchemaNode::group("Reference", vec![string("Code"), rows])
                    .with_required_fields(vec!["Code".into()])
                    .unwrap(),
                options: Default::default(),
                dynamic_path: None,
            },
            NamedSource {
                name: "Spare".into(),
                path: "unused/Spare.json".into(),
                schema: SchemaNode::group("Spare", vec![string("Code")]),
                options: Default::default(),
                dynamic_path: None,
            },
            NamedSource {
                name: "RequiredLegacy".into(),
                path: "unused/legacy.json".into(),
                schema: SchemaNode::group("Legacy", vec![string("Code")]),
                options: Default::default(),
                dynamic_path: None,
            },
        ],
        extra_targets: vec![
            NamedTarget {
                name: "named".into(),
                path: None,
                schema: output(false),
                options: Default::default(),
                root: scope(2),
            },
            NamedTarget {
                name: "legacy".into(),
                path: None,
                schema: output(false),
                options: Default::default(),
                root: scope(4),
            },
            NamedTarget {
                name: UNICODE.into(),
                path: None,
                schema: output(false),
                options: Default::default(),
                root: scope(1),
            },
        ],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (1, owned(SourceOwner::Primary, &["Code"], Some(&[]))),
                (2, owned(reference.clone(), &["Code"], Some(&[]))),
                (3, owned(reference, &["Rows", "Code"], Some(&["Rows"]))),
                (
                    4,
                    Node::SourceField {
                        path: path(&["RequiredLegacy", "Code"]),
                        frame: None,
                    },
                ),
                (5, owned(SourceOwner::Primary, &["Fail"], Some(&[]))),
                (6, Node::Raise { message: Some(7) }),
                (
                    7,
                    Node::Const {
                        value: Value::String("520 target poison".into()),
                    },
                ),
                (
                    8,
                    Node::If {
                        condition: 5,
                        then: 6,
                        else_: 1,
                    },
                ),
            ]),
        },
        root: scope(8),
    };
    let mut named = project.clone();
    named.root = scope(2);
    let mut repeated = project.clone();
    repeated.target = output(true);
    repeated.root = Scope {
        iteration: ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: SourceOwner::Named { name: "Ref".into() },
                path: path(&["Rows"]),
            },
            output_path: None,
        },
        ..scope(3)
    };
    vec![project, named, repeated]
}

pub(super) fn primary() -> Json {
    json!({"Code":"P.root","Fail":false,"Bound":1,"Ref":{"Code":"shadow"}})
}
pub(super) fn supplied() -> Json {
    json!([
        [
            "Ref",
            "{\"Code\":\"N.root\",\"Rows\":[{\"Code\":\"N1\"},{\"Code\":\"N2\"}]}"
        ],
        ["Spare", "{\"Code\":\"unused\"}"],
        ["RequiredLegacy", "{\"Code\":\"L.root\"}"]
    ])
}
fn document(value: &Json) -> String {
    serde_json::to_string(value).unwrap()
}
fn success(model: usize, selection: &str) -> Json {
    let primary = [
        "{\"Value\":\"P.root\"}",
        "{\"Value\":\"N.root\"}",
        "[{\"Value\":\"N1\"},{\"Value\":\"N2\"}]",
    ][model];
    let all = json!([
        [null, primary, if model == 2 { 2 } else { 1 }],
        ["named", "{\"Value\":\"N.root\"}", 1],
        ["legacy", "{\"Value\":\"L.root\"}", 1],
        [UNICODE, "{\"Value\":\"P.root\"}", 1]
    ]);
    let outputs = if selection == "All" {
        all
    } else {
        Json::Array(
            all.as_array()
                .unwrap()
                .iter()
                .filter(|row| {
                    if selection == "Primary" {
                        row[0].is_null()
                    } else {
                        row[0] == selection
                    }
                })
                .cloned()
                .collect(),
        )
    };
    json!({"Ok":outputs})
}
fn error(kind: &str) -> Json {
    json!({"Err":{"kind":kind}})
}
fn case(id: &str, model: usize, selections: &[&str], expected: Option<Json>) -> Json {
    json!({"id":id,"model":model,"source":document(&primary()),"inputs":supplied(),
        "limits":"generous","languages":"both","transports":["text","bytes"],
        "calls":selections.iter().map(|selection|json!({"selection":selection,
            "expected":expected.clone().unwrap_or_else(||success(model, selection))})).collect::<Vec<_>>()})
}
fn parse(id: &str, source: &str, marker: &str) -> Json {
    let mut item = case(
        id,
        0,
        &["Primary"],
        Some(json!({"Err":{
        "kind":"InvalidInput","rust_marker":marker,"csharp_marker":null}})),
    );
    item["source"] = source.into();
    item
}

/// Names, original byte payloads, ordering, selections and error recipes are
/// independent of native evaluation, lowering, emission and generated execution.
pub(super) fn controls() -> Json {
    let mut cases = Vec::new();
    for model in 0..MODELS {
        cases.push(case(
            &format!("compact-model{model}"),
            model,
            &["All", "Primary", "named", "legacy", UNICODE],
            None,
        ));
    }
    let mut item = case(
        "optional-omitted-primary-is-lazy",
        0,
        &["Primary", UNICODE],
        None,
    );
    item["inputs"] = json!([]);
    cases.push(item);
    let mut item = case(
        "optional-omitted-demand-is-source-unavailable",
        1,
        &["Primary", "named"],
        Some(json!({"Err":{"kind":"SourceUnavailable","name":"Ref","node":2}})),
    );
    item["inputs"] = json!([]);
    cases.push(item);
    let mut item = case(
        "missing-legacy-before-bad-primary",
        0,
        &["All", "legacy"],
        Some(json!({"Err":{"kind":"MissingNamedSource","name":"RequiredLegacy"}})),
    );
    item["source"] = "[".into();
    item["inputs"] = json!([]);
    cases.push(item);
    let mut item = case(
        "unselected-legacy-not-required",
        1,
        &["Primary", "named"],
        None,
    );
    item["inputs"].as_array_mut().unwrap().pop();
    cases.push(item);
    let mut item = case(
        "unknown-target-before-zero-policy-and-invalid-json",
        0,
        &["not-a-target"],
        Some(json!({"Err":{"kind":"UnknownTarget","name":"not-a-target"}})),
    );
    item["limits"] = "zero".into();
    item["source"] = "[".into();
    cases.push(item);
    let mut item = case(
        "known-target-roots-before-invalid-json",
        0,
        &["Primary"],
        Some(
            json!({"Err":{"kind":"MetadataLimit","resource":"Roots","used":0,"requested":4,"maximum":0}}),
        ),
    );
    item["limits"] = "no-roots".into();
    item["source"] = "[".into();
    cases.push(item);
    let mut item = case(
        "catalogue-name-bytes-before-input-inspection",
        0,
        &["Primary"],
        Some(
            json!({"Err":{"kind":"MetadataLimit","resource":"MetadataBytes","used":0,"requested":3,"maximum":0}}),
        ),
    );
    item["limits"] = "no-name-bytes".into();
    item["source"] = "[".into();
    cases.push(item);
    let mut item = case(
        "unknown-supplied-name-before-parse",
        0,
        &["Primary"],
        Some(json!({"Err":{"kind":"UnexpectedNamedSource","name":"alien"}})),
    );
    item["source"] = "[".into();
    item["inputs"] = json!([["alien", "["]]);
    cases.push(item);
    let mut item = case(
        "unknown-diagnostic-name-bytes-before-unexpected-name",
        0,
        &["Primary"],
        Some(
            json!({"Err":{"kind":"MetadataLimit","resource":"MetadataBytes","used":22,"requested":5,"maximum":22}}),
        ),
    );
    item["limits"] = "catalogue-name-bytes".into();
    item["inputs"] = json!([["alien", "["]]);
    cases.push(item);
    let mut item = case(
        "duplicate-name-before-primary-and-named-parse",
        0,
        &["Primary"],
        Some(json!({"Err":{"kind":"DuplicateNamedSource","name":"Ref"}})),
    );
    item["source"] = "[".into();
    item["inputs"] = json!([["Ref", "["], ["Ref", "{"]]);
    cases.push(item);
    let mut item = parse("primary-parse-before-all-supplied-documents", "[", "EOF");
    item["inputs"][0][1] = "42".into();
    cases.push(item);
    let mut item = parse(
        "unused-optional-document-is-still-parsed",
        &document(&primary()),
        "EOF",
    );
    item["inputs"] = json!([["Spare", "["]]);
    cases.push(item);
    let mut item = parse(
        "original-supplied-order-spare-before-ref",
        &document(&primary()),
        "EOF",
    );
    item["inputs"] = json!([["Spare", "["], ["Ref", "42"]]);
    cases.push(item);
    let mut item = parse(
        "original-supplied-order-ref-before-spare",
        &document(&primary()),
        "expected object",
    );
    item["inputs"] = json!([["Ref", "42"], ["Spare", "["]]);
    cases.push(item);
    cases.push(parse(
        "root-object-shape-rejects-number",
        "42",
        "expected object",
    ));
    cases.push(parse(
        "scalar-string-shape-rejects-object",
        "{\"Code\":{},\"Fail\":false}",
        "Code",
    ));
    cases.push(parse(
        "required-property-rejects-absence",
        "{\"Fail\":false}",
        "Code",
    ));
    cases.push(parse(
        "numeric-bound-below-zero",
        "{\"Code\":\"P.root\",\"Fail\":false,\"Bound\":-1}",
        "range [0, 2]",
    ));
    cases.push(parse(
        "numeric-bound-above-two",
        "{\"Code\":\"P.root\",\"Fail\":false,\"Bound\":3}",
        "range [0, 2]",
    ));
    let mut item = parse(
        "named-repeated-shape-rejects-scalar",
        &document(&primary()),
        "Rows",
    );
    item["inputs"] = json!([["Ref", "{\"Code\":\"N.root\",\"Rows\":42}"]]);
    cases.push(item);
    let mut item = parse(
        "named-repeated-item-bound-rejects-third",
        &document(&primary()),
        "Rows",
    );
    item["inputs"] = json!([[
        "Ref",
        "{\"Code\":\"N.root\",\"Rows\":[{\"Code\":\"1\"},{\"Code\":\"2\"},{\"Code\":\"3\"}]}"
    ]]);
    cases.push(item);
    let mut item = case(
        "mapping-error-has-no-partial-output",
        0,
        &["All", "Primary"],
        Some(json!({"Err":{"kind":"MappingException","node":6,"message":"520 target poison"}})),
    );
    let mut source = primary();
    source["Fail"] = true.into();
    item["source"] = document(&source).into();
    cases.push(item);
    let mut item = case(
        "unselected-primary-poison-is-lazy",
        0,
        &["named", "legacy", UNICODE],
        None,
    );
    item["source"] = document(&source).into();
    cases.push(item);
    let mut item = case(
        "late-output-serialization-failure-discards-all",
        0,
        &["All", "legacy"],
        Some(error("InvalidOutput")),
    );
    item["inputs"][2][1] = "{\"Code\":\"123456789\"}".into();
    cases.push(item);
    let mut item = case("unselected-invalid-output-is-lazy", 0, &["Primary"], None);
    item["inputs"][2][1] = "{\"Code\":\"123456789\"}".into();
    cases.push(item);
    for (id, bytes, offset, length) in [
        (
            "utf8-invalid-primary-after-multibyte-prefix",
            b"{\"Code\":\"\xe9\x9b\xaa\xff\"}".to_vec(),
            12,
            Some(1),
        ),
        (
            "utf8-truncated-primary",
            b"{\"Code\":\"\xe2\x82".to_vec(),
            9,
            None,
        ),
    ] {
        let decoder_bytes = if length.is_some() {
            vec![255_u8]
        } else {
            vec![226, 130]
        };
        let mut item = case(
            id,
            0,
            &["Primary"],
            Some(
                json!({"Err":{"kind":"Utf8","offset":offset,"error_length":length,"decoder_bytes":decoder_bytes}}),
            ),
        );
        item["source_bytes"] = json!(bytes);
        item["transports"] = json!(["bytes"]);
        cases.push(item);
    }
    let mut item = case(
        "utf8-unused-optional-retains-original-index",
        0,
        &["Primary"],
        Some(json!({"Err":{"kind":"Utf8","offset":12,"error_length":1,"decoder_bytes":[255]}})),
    );
    item["inputs"] = json!([["Spare", "placeholder"]]);
    item["input_bytes"] = json!([[
        123, 34, 67, 111, 100, 101, 34, 58, 34, 233, 155, 170, 255, 34, 125
    ]]);
    item["transports"] = json!(["bytes"]);
    cases.push(item);
    let mut item = case(
        "csharp-null-source-before-unknown-selection",
        0,
        &["not-a-target"],
        Some(json!({"Err":{"kind":"ArgumentNull","parameter":"source"}})),
    );
    item["source"] = Json::Null;
    item["languages"] = "csharp".into();
    cases.push(item);
    let mut item = case(
        "csharp-null-selection-before-inputs-and-policy",
        0,
        &["Null"],
        Some(json!({"Err":{"kind":"ArgumentNull","parameter":"selection"}})),
    );
    item["inputs"] = Json::Null;
    item["limits"] = "zero".into();
    item["languages"] = "csharp".into();
    cases.push(item);
    let mut item = case(
        "csharp-null-supplied-before-policy",
        0,
        &["Primary"],
        Some(json!({"Err":{"kind":"ArgumentNull","parameter":"inputs"}})),
    );
    item["inputs"] = Json::Null;
    item["limits"] = "zero".into();
    cases.push(item);
    cases.last_mut().unwrap()["languages"] = "csharp".into();
    let mut item = case(
        "csharp-policy-before-supplied-enumeration",
        0,
        &["Primary"],
        Some(
            json!({"Err":{"kind":"MetadataLimit","resource":"Roots","used":0,"requested":4,"maximum":0}}),
        ),
    );
    item["limits"] = "no-roots".into();
    item["languages"] = "csharp".into();
    item["list_probe"] = "reject-any-read".into();
    cases.push(item);
    let mut item = case(
        "csharp-capture-original-handles-once",
        0,
        &["Primary"],
        None,
    );
    item["languages"] = "csharp".into();
    item["list_probe"] = "exactly-once".into();
    cases.push(item);
    let mut item = case(
        "csharp-unknown-target-before-null-supplied",
        0,
        &["not-a-target"],
        Some(json!({"Err":{"kind":"UnknownTarget","name":"not-a-target"}})),
    );
    item["languages"] = "csharp".into();
    item["inputs"] = Json::Null;
    item["limits"] = "zero".into();
    item["source"] = "[".into();
    cases.push(item);
    let mut item = case(
        "reordered-valid-documents-use-name-schema-binding",
        1,
        &["Primary"],
        None,
    );
    item["inputs"] = json!([
        ["Spare", "{}"],
        [
            "Ref",
            "{\"Code\":\"N.root\",\"Rows\":[{\"Code\":\"N1\"},{\"Code\":\"N2\"}]}"
        ],
        ["RequiredLegacy", "{\"Code\":\"L.root\"}"]
    ]);
    cases.push(item);
    json!({"version":1,"models":MODELS,"cases":cases})
}

pub(super) fn counts(controls: &Json) -> (usize, usize) {
    let mut portable = 0;
    let mut csharp = 0;
    for case in controls["cases"].as_array().unwrap() {
        let calls =
            case["calls"].as_array().unwrap().len() * case["transports"].as_array().unwrap().len();
        if case["languages"] == "both" {
            portable += calls;
        } else {
            csharp += calls;
        }
    }
    (portable, csharp)
}
