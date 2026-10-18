//! Independent literal recipes. No helper runs a producer or computes selection.
use super::*;
pub fn scalar(value: Value) -> Instance {
    Instance::Scalar(value)
}
pub fn text(value: &str) -> Instance {
    scalar(Value::String(value.into()))
}
pub fn integer(value: i64) -> Instance {
    scalar(Value::Int(value))
}
pub fn boolean(value: bool) -> Instance {
    scalar(Value::Bool(value))
}
pub fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}
pub fn repeated(items: Vec<Instance>) -> Instance {
    Instance::Repeated(items)
}
pub fn result(value: Instance) -> Instance {
    group(vec![("Result", value)])
}
pub fn source() -> Instance {
    endpoint(
        "P-a|P-b|P-c",
        1,
        vec![row("P", 1, 2, false), row("P", 2, 1, false)],
    )
}
fn row(key: &str, index: i64, rank: i64, reject: bool) -> Instance {
    group(vec![
        ("Key", text(key)),
        ("Index", integer(index)),
        ("Rank", integer(rank)),
        ("Reject", boolean(reject)),
    ])
}
fn endpoint(input: &str, index: i64, rows: Vec<Instance>) -> Instance {
    group(vec![
        (
            "Cfg",
            group(vec![
                ("Index", integer(index)),
                ("Text", text(input)),
                ("Delimiter", text("|")),
                ("From", integer(10)),
                ("To", integer(12)),
                ("Width", integer(2)),
                ("Pattern", text("[,;]")),
                ("Flags", text("")),
            ]),
        ),
        ("GlobalReject", boolean(false)),
        ("Rows", repeated(rows)),
    ])
}
pub fn inputs() -> Vec<(String, Instance)> {
    vec![
        (
            "Cfg".into(),
            endpoint(
                "N-a|N-b|N-c",
                3,
                vec![
                    row("A", 3, 30, false),
                    row("A", 1, 10, false),
                    row("B", 2, 20, false),
                ],
            ),
        ),
        (projects::EXACT.into(), endpoint("U-a|U-b|U-c", 2, vec![])),
        ("Legacy".into(), group(vec![("Text", text("L"))])),
    ]
}
pub fn replace(value: &mut Instance, name: &str, new: Instance) {
    let Instance::Group(fields) = value else {
        panic!("literal group")
    };
    fields.iter_mut().find(|(key, _)| key == name).unwrap().1 = new;
}
fn cfg(case: &mut Case, name: &str, value: Instance) {
    let root = &mut case
        .inputs
        .iter_mut()
        .find(|(name, _)| name == "Cfg")
        .unwrap()
        .1;
    let Instance::Group(fields) = root else {
        unreachable!()
    };
    replace(
        &mut fields.iter_mut().find(|(name, _)| name == "Cfg").unwrap().1,
        name,
        value,
    );
}
fn erase_index(case: &mut Case) {
    let root = &mut case.inputs[0].1;
    let Instance::Group(fields) = root else {
        unreachable!()
    };
    let Instance::Group(cfg) = &mut fields.iter_mut().find(|(name, _)| name == "Cfg").unwrap().1
    else {
        unreachable!()
    };
    cfg.retain(|(name, _)| name != "Index");
}
fn ok(outputs: Vec<(Option<&str>, Instance)>) -> Json {
    json!({"Ok":outputs.iter().map(|(name,tree)|json!([name,codec::encode(tree)])).collect::<Vec<_>>()})
}
fn expected(tree: Instance) -> Json {
    ok(vec![(None, tree)])
}
fn error(kind: &str) -> Json {
    json!({"Err":{"kind":kind}})
}
fn raised(node: u32, message: &str) -> Json {
    json!({"Err":{"kind":"MappingException","node":node,"message":message}})
}
fn function(kind: &str, name: &str, detail: &str) -> Json {
    json!({"Err":{"kind":kind,"function":name,"detail":detail}})
}
fn too_large(requested: &str) -> Json {
    json!({"Err":{"kind":"GeneratedSequenceTooLarge","requested":requested,"maximum":"1000000"}})
}
fn owned(
    node: u32,
    cause: &str,
    path: &[&str],
    segment: Option<usize>,
    found: Option<&str>,
    collection: Option<&[&str]>,
) -> Json {
    json!({"Err":{"kind":"OwnedSource","node":node,"cause":cause,"owner":"Cfg","path":path,"native_path":path,"segment":segment,"found":found,"collection":collection,"native_scope":null}})
}
fn normal(id: &'static str, profile: &'static str, outcome: Json) -> Case {
    let all = if let Some(outputs) = outcome.get("Ok") {
        let mut outputs = outputs.as_array().unwrap().clone();
        outputs.push(json!([
            projects::AUDIT,
            codec::encode(&result(text("u-b")))
        ]));
        outputs.push(json!(["legacy", codec::encode(&result(text("L")))]));
        json!({"Ok":outputs})
    } else {
        outcome.clone()
    };
    Case {
        id,
        profile,
        source: source(),
        inputs: inputs(),
        native: true,
        calls: vec![
            json!({"selection":"All","expected":all}),
            json!({"selection":"Primary","expected":outcome}),
            json!({"selection":projects::AUDIT,"expected":ok(vec![(Some(projects::AUDIT),result(text("u-b")))])}),
        ],
    }
}
fn value(id: &'static str, profile: &'static str, value: Instance) -> Case {
    normal(id, profile, expected(result(value)))
}
fn rows(values: &[&str]) -> Instance {
    group(vec![(
        "Rows",
        repeated(values.iter().map(|value| result(text(value))).collect()),
    )])
}
fn group_member(result_: &str, position: &str) -> Instance {
    group(vec![
        ("Result", text(result_)),
        ("ByPosition", text(position)),
    ])
}
pub fn cases() -> Vec<Case> {
    let mut cases = vec![
        value("A01-named-index-collision", "literal", text("終")),
        value("A02-exact-spaces-unicode-owner", "exact", text("β")),
        value("A03-owned-producer-and-index", "owned-text", text("N-c")),
        value("A04-range-retains-int-tag", "generate", integer(12)),
        value(
            "A04-target-adaptation-separate",
            "float-target",
            scalar(Value::Float(12.0)),
        ),
        value("A07-shared-to-and-index-edges", "shared-node", integer(3)),
        value("A-nested-index", "nested-index", scalar(Value::Null)),
        value("A-nested-producer", "nested-producer", text("f")),
        value(
            "A-If-ValueMap-and-legacy-branch",
            "mixed-legacy",
            text("N-b"),
        ),
        normal(
            "A08-parent-row-index",
            "rows-index",
            expected(rows(&["c", "a", "b"])),
        ),
        normal(
            "A09-parent-owned-position",
            "rows-position",
            expected(rows(&["a", "b", "c"])),
        ),
        normal(
            "A08-immutable-root-index-in-rows",
            "rows-root-pin",
            expected(rows(&["c", "c", "c"])),
        ),
        normal(
            "A10-group-representatives-and-members",
            "grouped",
            expected(group(vec![(
                "Groups",
                repeated(vec![
                    group(vec![
                        ("Result", text("z")),
                        ("ByPosition", text("x")),
                        (
                            "Members",
                            repeated(vec![group_member("z", "x"), group_member("x", "y")]),
                        ),
                    ]),
                    group(vec![
                        ("Result", text("y")),
                        ("ByPosition", text("y")),
                        ("Members", repeated(vec![group_member("y", "x")])),
                    ]),
                ]),
            )])),
        ),
        normal(
            "A-parent-cross-owner-pins",
            "cross-owner",
            expected(group(vec![(
                "Parents",
                repeated(vec![
                    group(vec![
                        ("OuterResult", text("a")),
                        (
                            "Rows",
                            repeated(vec![
                                result(text("c")),
                                result(text("a")),
                                result(text("b")),
                            ]),
                        ),
                    ]),
                    group(vec![
                        ("OuterResult", text("b")),
                        (
                            "Rows",
                            repeated(vec![
                                result(text("c")),
                                result(text("a")),
                                result(text("b")),
                            ]),
                        ),
                    ]),
                ]),
            )])),
        ),
        normal(
            "A-inherited-filter-sort-By-post-window",
            "controls",
            expected(rows(&["b", "c"])),
        ),
        normal(
            "E03-empty-production-still-index",
            "empty-index-raise",
            raised(30, "INDEX"),
        ),
        normal(
            "E06-producer-prevents-index",
            "input-raise",
            raised(10, "PRODUCER"),
        ),
        normal(
            "E-inactive-foreign-pin",
            "inactive-index",
            owned(30, "InactiveSourceFrame", &[], None, None, Some(&["Rows"])),
        ),
    ];
    // Literal conversion table. No selected-index helper is invoked here.
    for (id, index, selected) in [
        ("E01-zero", Value::Int(0), None),
        ("E01-negative", Value::Int(-1), None),
        ("E01-past-end", Value::Int(4), None),
        ("E01-max-int", Value::Int(i64::MAX), None),
        ("E01-bool", Value::Bool(true), None),
        ("E01-null", Value::Null, None),
        ("E01-json-null", Value::json_null(), None),
        ("E01-xml-nil", Value::xml_nil(), None),
        ("E01-decimal-string", Value::String("2.0".into()), None),
        ("E01-exponent-string", Value::String("2e0".into()), None),
        (
            "E02-trimmed-int-string",
            Value::String(" 2 ".into()),
            Some("β"),
        ),
        ("E02-below-half", Value::Float(2.49), Some("β")),
        ("E02-positive-half", Value::Float(2.5), Some("終")),
        ("E02-half-to-one", Value::Float(0.5), Some("a")),
        ("E02-negative-half", Value::Float(-0.5), None),
        (
            "E02-payload-NaN",
            Value::Float(f64::from_bits(0x7ff8_0000_0000_0041)),
            None,
        ),
        (
            "E02-positive-infinity",
            Value::Float(f64::from_bits(0x7ff0_0000_0000_0000)),
            None,
        ),
        (
            "E02-negative-infinity",
            Value::Float(f64::from_bits(0xfff0_0000_0000_0000)),
            None,
        ),
    ] {
        let mut case = value(
            id,
            "literal",
            selected.map(text).unwrap_or_else(|| scalar(Value::Null)),
        );
        cfg(&mut case, "Index", scalar(index));
        cases.push(case);
    }
    finish(cases)
}
fn finish(mut cases: Vec<Case>) -> Vec<Case> {
    let mut unicode = value("A05-Unicode-scalars-not-UTF16", "by-length", text("βz"));
    cfg(&mut unicode, "Text", text("a💠βzQ"));
    cfg(&mut unicode, "Index", integer(2));
    cases.push(unicode);
    let mut trunc = value(
        "A05-width-truncation-not-ItemAt-rounding",
        "by-length",
        text("🙂z"),
    );
    cfg(&mut trunc, "Text", text("e\u{301}🙂z!"));
    cfg(&mut trunc, "Width", scalar(Value::Float(2.9)));
    cfg(&mut trunc, "Index", integer(2));
    cases.push(trunc);
    for (id, profile) in [
        ("A06-regex-no-flags", "regex-none"),
        ("A06-regex-explicit-empty-flags", "regex-flags"),
    ] {
        let mut case = value(id, profile, text("bb"));
        cfg(&mut case, "Text", text("a,bb;終"));
        cfg(&mut case, "Index", integer(2));
        cases.push(case);
    }
    let mut flags = value("A06-regex-ix", "regex-flags", text("GAMMA"));
    cfg(&mut flags, "Text", text("Alpha--beta---GAMMA"));
    cfg(&mut flags, "Pattern", text("-+ BETA -+"));
    cfg(&mut flags, "Flags", text("ix"));
    cfg(&mut flags, "Index", integer(2));
    cases.push(flags);
    for (id, profile, expect) in [
        ("E05-literal-empty-one-item", "owned-text", text("")),
        (
            "E05-regex-empty-no-items",
            "regex-none",
            scalar(Value::Null),
        ),
        ("E05-width-empty-no-items", "by-length", scalar(Value::Null)),
    ] {
        let mut case = value(id, profile, expect);
        cfg(&mut case, "Text", text(""));
        cfg(&mut case, "Index", integer(1));
        cases.push(case);
    }
    for (id, profile, argument) in [
        ("E04-null-input-skips-delimiter", "text-index-raise", "Text"),
        ("E04-null-from-skips-to", "range-index-raise", "From"),
        (
            "E04-null-regex-pattern-skips-flags",
            "regex-index-raise",
            "Pattern",
        ),
    ] {
        for (json_null, null) in [(false, Value::Null), (true, Value::json_null())] {
            let mut case = normal(id, profile, raised(30, "INDEX"));
            case.id = match (profile, json_null) {
                ("text-index-raise", true) => "E04-jsonnull-input-skips-delimiter",
                ("range-index-raise", true) => "E04-jsonnull-from-skips-to",
                ("regex-index-raise", true) => "E04-jsonnull-pattern-skips-flags",
                _ => id,
            };
            cfg(&mut case, argument, scalar(null));
            cases.push(case);
        }
    }
    let mut xml = normal(
        "E-xmlnil-is-not-producer-null",
        "owned-text",
        function("FunctionType", "tokenize", "xml nil"),
    );
    cfg(&mut xml, "Text", scalar(Value::xml_nil()));
    cases.push(xml);
    let mut bad = normal(
        "E06-empty-delimiter-before-missing-index",
        "owned-text",
        function(
            "FunctionInvalidArgument",
            "tokenize",
            "requires a non-empty delimiter",
        ),
    );
    cfg(&mut bad, "Delimiter", text(""));
    erase_index(&mut bad);
    cases.push(bad);
    let mut width = normal(
        "E-width-rejects-decimal-string",
        "by-length",
        function(
            "FunctionInvalidArgument",
            "tokenize-by-length",
            "requires a positive integer length",
        ),
    );
    cfg(&mut width, "Width", text("2.0"));
    cases.push(width);
    let mut default = value("A-range-default-from", "generate-default", integer(3));
    cfg(&mut default, "To", integer(3));
    cases.push(default);
    let mut explicit = value("A-range-explicit-numeric-coercions", "range", integer(0));
    cfg(&mut explicit, "From", text("-2.0"));
    cfg(&mut explicit, "To", scalar(Value::Float(0.0)));
    cases.push(explicit);
    let mut empty = value("E-explicit-null-from-empty", "range", scalar(Value::Null));
    cfg(&mut empty, "From", scalar(Value::Null));
    cases.push(empty);
    let mut over = normal(
        "E07-cap-precedes-zero-index",
        "generate-default",
        too_large("1000001"),
    );
    cfg(&mut over, "To", integer(1_000_001));
    cfg(&mut over, "Index", integer(0));
    cases.push(over);
    let mut exact = value(
        "A-exact-cap-last-item",
        "generate-default",
        integer(1_000_000),
    );
    cfg(&mut exact, "To", integer(1_000_000));
    cfg(&mut exact, "Index", integer(1_000_000));
    cases.push(exact);
    let mut no_owner = normal(
        "E08-unsupplied-index-owner",
        "literal",
        owned(30, "SourceUnavailable", &[], None, None, None),
    );
    no_owner.inputs.retain(|(name, _)| name != "Cfg");
    cases.push(no_owner);
    let mut no_producer = normal(
        "E-unsupplied-producer-owner",
        "owned-text",
        owned(10, "SourceUnavailable", &[], None, None, None),
    );
    no_producer.inputs.retain(|(name, _)| name != "Cfg");
    cases.push(no_producer);
    let mut missing = value(
        "E09-present-owner-absent-index",
        "literal",
        scalar(Value::Null),
    );
    erase_index(&mut missing);
    cases.push(missing);
    let mut empty_owner = normal(
        "E08-empty-vector-still-unsupplied-index",
        "empty",
        owned(30, "SourceUnavailable", &[], None, None, None),
    );
    empty_owner.inputs.retain(|(name, _)| name != "Cfg");
    cases.push(empty_owner);
    let mut null_flags = value(
        "E-regex-null-flags-empty-vector",
        "regex-flags",
        scalar(Value::Null),
    );
    cfg(&mut null_flags, "Flags", scalar(Value::Null));
    cases.push(null_flags);
    for (id, value, cause, segment, found) in [
        (
            "E10b-malformed-parent",
            integer(7),
            "CannotTraverse",
            Some(1),
            Some("scalar"),
        ),
        (
            "E10b-terminal-repetition",
            repeated(vec![integer(3)]),
            "UniteratedRepetition",
            Some(2),
            None,
        ),
    ] {
        let mut case = normal(
            id,
            "literal",
            owned(30, cause, &["Cfg", "Index"], segment, found, None),
        );
        if cause == "CannotTraverse" {
            replace(&mut case.inputs[0].1, "Cfg", value);
        } else {
            cfg(&mut case, "Index", value);
        }
        cases.push(case);
    }
    let mut producer = normal(
        "E06-producer-before-unavailable-index",
        "input-raise",
        raised(10, "PRODUCER"),
    );
    producer.inputs.retain(|(name, _)| name != "Cfg");
    cases.push(producer);
    for (id, profile, expect) in [
        (
            "E-regex-invalid-flags-before-index",
            "regex-flags",
            json!({"Err":{"kind":"InvalidTokenizeRegexFlags","flags":"q"}}),
        ),
        (
            "E-regex-zero-width-before-index",
            "regex-none",
            error("ZeroWidthTokenizeRegex"),
        ),
    ] {
        let mut case = normal(id, profile, expect);
        if profile == "regex-flags" {
            cfg(&mut case, "Flags", text("q"));
        } else {
            cfg(&mut case, "Pattern", text("a*"));
        }
        erase_index(&mut case);
        cases.push(case);
    }
    let mut global = normal(
        "E11-global-failure-preempts-all-targets",
        "failures",
        json!({"Err":{"kind":"MappingFailure","rule":1,"message":"global.reject"}}),
    );
    replace(&mut global.source, "GlobalReject", boolean(true));
    global.calls[2]["expected"] = global.calls[0]["expected"].clone();
    cases.push(global);
    let mut rowfail = normal(
        "E11-qualified-primary-row-failure",
        "failures",
        json!({"Err":{"kind":"MappingFailure","rule":2,"message":"row.reject"}}),
    );
    replace(
        &mut rowfail.source,
        "Rows",
        repeated(vec![row("P", 1, 1, true)]),
    );
    rowfail.calls[2]["expected"] = rowfail.calls[0]["expected"].clone();
    cases.push(rowfail);
    let mut selection = normal(
        "S-exact-target-before-unavailable-owner",
        "literal",
        expected(result(text("終"))),
    );
    selection.inputs.retain(|(name, _)| name != "Cfg");
    selection.calls = vec![
        json!({"selection":"audit💠","expected":{"Err":{"kind":"UnknownTarget","name":"audit💠"}}}),
        json!({"selection":projects::AUDIT,"expected":ok(vec![(Some(projects::AUDIT),result(text("u-b")))])}),
    ];
    cases.push(selection);
    cases
}
