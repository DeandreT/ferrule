//! Independent complete literals. Helpers only build listed fields; none
//! tokenize, filter, reduce or group input data.
use super::*;

fn scalar(value: Value) -> Instance {
    Instance::Scalar(value)
}
fn text(value: &str) -> Instance {
    scalar(Value::String(value.into()))
}
fn int(value: i64) -> Instance {
    scalar(Value::Int(value))
}
fn float(value: f64) -> Instance {
    scalar(Value::Float(value))
}
fn null() -> Instance {
    scalar(Value::Null)
}
fn boolean(value: bool) -> Instance {
    scalar(Value::Bool(value))
}
fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn repeated(items: Vec<Instance>) -> Instance {
    Instance::Repeated(items)
}
fn row(code: &str, label: &str, keep: bool) -> Instance {
    group(vec![
        ("Code", text(code)),
        ("Label", text(label)),
        ("Keep", boolean(keep)),
    ])
}
fn endpoint(marker: &str, code: &str, list: &str, count: i64, rows: Vec<Instance>) -> Instance {
    group(vec![
        ("Marker", text(marker)),
        ("Code", text(code)),
        ("List", text(list)),
        ("Count", int(count)),
        ("Rows", repeated(rows)),
    ])
}
pub fn source() -> Instance {
    endpoint(
        "P.root",
        "Q",
        "x|Q|M",
        2,
        vec![
            row("Q", "p1", true),
            row("Z", "p2", false),
            row("M", "p3", true),
        ],
    )
}
pub fn inputs() -> Vec<(String, Instance)> {
    vec![
        (
            projects::NAMED.into(),
            endpoint(
                "N.root",
                "M",
                "N|M",
                3,
                vec![row("M", "n1", true), row("Q", "n2", false)],
            ),
        ),
        (
            projects::EXACT.into(),
            endpoint("U.root", "U", "U|V", 1, vec![row("U", "u1", true)]),
        ),
    ]
}
fn ok(outputs: Vec<(Option<&str>, Instance)>) -> Json {
    json!({"Ok": outputs
        .into_iter()
        .map(|(name, tree)| json!([name, codec::encode(&tree)]))
        .collect::<Vec<_>>()})
}
fn audit() -> Instance {
    group(vec![("Count", int(1)), ("Primary", int(0))])
}
fn unicode() -> Instance {
    group(vec![("Joined", text("UUMVU")), ("Cross", int(0))])
}
fn exception(node: u32, message: &str) -> Json {
    json!({"Err":{"kind":"MappingException","node":node,"message":message}})
}
/// All, Primary and both exact named selections. Named targets are fixed
/// exact-owner routes, so a primary failure leaves them selectable.
fn case(profile: &'static str, primary: Result<Instance, Json>) -> Case {
    let (all, selected) = match primary {
        Ok(tree) => (
            ok(vec![
                (None, tree.clone()),
                (Some(projects::AUDIT), audit()),
                (Some(projects::UNICODE_TARGET), unicode()),
            ]),
            ok(vec![(None, tree)]),
        ),
        Err(error) => (error.clone(), error),
    };
    Case {
        id: profile,
        profile,
        source: source(),
        inputs: inputs(),
        calls: vec![
            json!({"selection":"All","expected":all}),
            json!({"selection":"Primary","expected":selected}),
            json!({"selection":projects::AUDIT,
                "expected":ok(vec![(Some(projects::AUDIT), audit())])}),
            json!({"selection":projects::UNICODE_TARGET,
                "expected":ok(vec![(Some(projects::UNICODE_TARGET), unicode())])}),
        ],
        native: true,
    }
}
fn value(tree: Instance) -> Instance {
    group(vec![("Value", tree)])
}
fn named_ints(fields: &[(&str, i64)]) -> Instance {
    group(
        fields
            .iter()
            .map(|(name, value)| (*name, int(*value)))
            .collect(),
    )
}
fn firsts(field: &str, values: &[&str]) -> Instance {
    group(vec![(
        field,
        repeated(
            values
                .iter()
                .map(|value| group(vec![("First", text(value))]))
                .collect(),
        ),
    )])
}

pub fn cases() -> Vec<Case> {
    let n01 = |label: &str, position: i64, count: i64, labels: &str| {
        group(vec![
            ("Label", text(label)),
            ("Position", int(position)),
            ("Count", int(count)),
            ("Labels", text(labels)),
        ])
    };
    let inner =
        |label: &str, count: i64| group(vec![("Label", text(label)), ("Count", int(count))]);
    let outer = |label: &str, items: Vec<Instance>| {
        group(vec![("Label", text(label)), ("Inner", repeated(items))])
    };
    let member = |label: &str, count: i64, ordinal: i64| {
        group(vec![
            ("Label", text(label)),
            ("Count", int(count)),
            ("Ordinal", int(ordinal)),
        ])
    };
    let grouped = |first: &str, count: i64, ordinal: i64, members: Vec<Instance>| {
        group(vec![
            ("First", text(first)),
            ("Count", int(count)),
            ("Ordinal", int(ordinal)),
            ("Members", repeated(members)),
        ])
    };
    let aggregate_error =
        |kind: &str, function: &str| json!({"Err":{"kind":kind,"function":function}});
    vec![
        case(
            "S01-colliding",
            Ok(named_ints(&[
                ("Primary", 1),
                ("Named", 3),
                ("Unicode", 2),
                ("Legacy", 1),
            ])),
        ),
        case(
            "S02-phases",
            Ok(group(vec![
                ("Joined", text("NUQMU")),
                ("Summed", int(12)),
                ("Picked", text("x")),
            ])),
        ),
        case(
            "S03-reducers",
            Ok(group(vec![
                ("Count", int(2)),
                ("Sum", int(4)),
                ("Avg", float(2.0)),
                ("Min", int(1)),
                ("Max", int(3)),
                ("Join", text("1M3")),
                ("ItemAt", int(3)),
            ])),
        ),
        case(
            "S04-producers",
            Ok(group(vec![
                ("ByLength", text("N|UM")),
                ("RegexNoFlags", int(2)),
                ("RegexFlags", text("NQM")),
                ("Range", int(5)),
            ])),
        ),
        case(
            "S05-empty",
            Ok(group(vec![
                ("Count", int(0)),
                ("Sum", int(0)),
                ("Avg", null()),
                ("Min", null()),
                ("Max", null()),
                ("Join", text("")),
                ("ItemAt", null()),
                ("EmptyText", int(1)),
                ("Rejected", int(0)),
                ("Unreached", int(0)),
            ])),
        ),
        case(
            "S06-numeric",
            Ok(group(vec![
                ("Sum", int(3)),
                ("Avg", float(1.5)),
                ("NoNumbers", null()),
                ("MinMixed", float(1.5)),
                ("MaxMixed", int(2)),
            ])),
        ),
        case("S07-lazy", Ok(value(text("cMd")))),
        case(
            "N01-rows",
            Ok(group(vec![(
                "Rows",
                repeated(vec![
                    n01("p1", 1, 1, "Qp111MMp121"),
                    n01("p2", 2, 0, "Qp212MMp222"),
                    n01("p3", 3, 1, "Qp313MMp323"),
                ]),
            )])),
        ),
        case(
            "N02-nested-owners",
            Ok(group(vec![(
                "Outer",
                repeated(vec![
                    outer("p1", vec![inner("n1", 0), inner("n2", 1)]),
                    outer("p2", vec![inner("n1", 0), inner("n2", 0)]),
                    outer("p3", vec![inner("n1", 1), inner("n2", 0)]),
                ]),
            )])),
        ),
        case(
            "G01-group-by",
            Ok(group(vec![(
                "Groups",
                repeated(vec![
                    grouped("p1", 0, 0, vec![member("p1", 0, 0), member("p3", 1, 1)]),
                    grouped("p2", 2, 1, vec![member("p2", 2, 0)]),
                ]),
            )])),
        ),
        case("G02-starting", Ok(firsts("Groups", &["p1", "p2"]))),
        case("G03-ending", Ok(firsts("Groups", &["p1", "p3"]))),
        case("F01-filter", Ok(firsts("Rows", &["p3"]))),
        case("I01-item-at", Ok(value(int(3)))),
        case("I02-nested-reducers", Ok(value(int(6)))),
        case(
            "L01-legacy-island",
            Ok(group(vec![(
                "Legacy",
                repeated(vec![
                    named_ints(&[("Count", 0)]),
                    named_ints(&[("Count", 2)]),
                    named_ints(&[("Count", 0)]),
                ]),
            )])),
        ),
        case("E01-expression-first", Err(exception(51, "expression"))),
        case("E02-predicate-first", Err(exception(50, "predicate"))),
        case("E03-argument-last", Err(exception(52, "argument"))),
        case("E04-producer-error", Err(exception(30, "producer"))),
        case(
            "E05-empty-argument",
            Err(
                json!({"Err":{"kind":"OwnedSource","node":301,"cause":"SourceUnavailable",
                "owner":projects::UNUSED,"native_scope":null}}),
            ),
        ),
        case(
            "E06-not-bool",
            Err(json!({"Err":{"kind":"NotABool","node":101,"found":"string"}})),
        ),
        case(
            "E07-overflow",
            Err(aggregate_error("AggregateIntegerOverflow", "Sum")),
        ),
        case(
            "E08-non-finite",
            Err(aggregate_error("AggregateNonFinite", "Avg")),
        ),
        case(
            "E09-missing-owner",
            Err(
                json!({"Err":{"kind":"OwnedSource","node":301,"cause":"SourceUnavailable",
                "owner":projects::UNUSED,"native_scope":null}}),
            ),
        ),
    ]
}
