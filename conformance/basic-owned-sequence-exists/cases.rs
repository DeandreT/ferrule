//! Independent complete literals. Helpers only build listed fields; none
//! tokenize, compare, filter or group input data.
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
fn flags(fields: &[(&str, bool)]) -> Instance {
    group(
        fields
            .iter()
            .map(|(name, value)| (*name, boolean(*value)))
            .collect(),
    )
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
    flags(&[("Found", true), ("Primary", false)])
}
fn unicode() -> Instance {
    flags(&[("Found", true), ("Cross", false)])
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
fn rows(fields: Vec<Instance>) -> Instance {
    group(vec![("Rows", repeated(fields))])
}

pub fn cases() -> Vec<Case> {
    let n01 = |label: &str, position: i64, matched: bool, named: bool| {
        group(vec![
            ("Label", text(label)),
            ("Position", int(position)),
            ("Match", boolean(matched)),
            ("NamedHit", boolean(named)),
        ])
    };
    let inner =
        |label: &str, found: bool| group(vec![("Label", text(label)), ("Found", boolean(found))]);
    let outer = |label: &str, items: Vec<Instance>| {
        group(vec![("Label", text(label)), ("Inner", repeated(items))])
    };
    let member = |label: &str, found: bool, ordinal: bool| {
        group(vec![
            ("Label", text(label)),
            ("Found", boolean(found)),
            ("Ordinal", boolean(ordinal)),
        ])
    };
    let grouped = |first: &str, found: bool, ordinal: bool, members: Vec<Instance>| {
        group(vec![
            ("First", text(first)),
            ("Found", boolean(found)),
            ("Ordinal", boolean(ordinal)),
            ("Members", repeated(members)),
        ])
    };
    let labels = |field: &str, values: &[&str]| {
        group(vec![(
            field,
            repeated(
                values
                    .iter()
                    .map(|value| {
                        group(vec![(
                            if field == "Groups" { "First" } else { "Label" },
                            text(value),
                        )])
                    })
                    .collect(),
            ),
        )])
    };
    vec![
        case(
            "S01-colliding",
            Ok(flags(&[
                ("Primary", false),
                ("Named", true),
                ("Unicode", true),
                ("Legacy", false),
            ])),
        ),
        case(
            "S02-producer-owners",
            Ok(flags(&[("Named", true), ("Unicode", false)])),
        ),
        case(
            "S03-generate",
            Ok(flags(&[("Omitted", true), ("Empty", false)])),
        ),
        case("S04-by-length", Ok(flags(&[("Found", true)]))),
        case(
            "S05-regex",
            Ok(flags(&[("NoFlags", false), ("Flags", true)])),
        ),
        case("S06-first-true", Ok(flags(&[("Found", true)]))),
        case(
            "S07-empty",
            Ok(flags(&[
                ("NullInput", false),
                ("EmptyText", true),
                ("Suppressed", false),
                ("Unreached", false),
            ])),
        ),
        case(
            "N01-rows",
            Ok(rows(vec![
                n01("p1", 1, true, false),
                n01("p2", 2, false, false),
                n01("p3", 3, true, true),
            ])),
        ),
        case(
            "N02-nested-owners",
            Ok(group(vec![(
                "Outer",
                repeated(vec![
                    outer("p1", vec![inner("n1", false), inner("n2", true)]),
                    outer("p2", vec![inner("n1", false), inner("n2", false)]),
                    outer("p3", vec![inner("n1", true), inner("n2", false)]),
                ]),
            )])),
        ),
        case(
            "G01-group-by",
            Ok(group(vec![(
                "Groups",
                repeated(vec![
                    grouped(
                        "p1",
                        false,
                        false,
                        vec![member("p1", false, false), member("p3", true, true)],
                    ),
                    grouped("p2", true, true, vec![member("p2", true, false)]),
                ]),
            )])),
        ),
        case("G02-starting", Ok(labels("Groups", &["p1", "p2"]))),
        case("G03-ending", Ok(labels("Groups", &["p1", "p3"]))),
        case("F01-filter", Ok(labels("Rows", &["p3"]))),
        case("I01-item-at", Ok(flags(&[("Found", true)]))),
        case("I02-nested-exists", Ok(flags(&[("Found", true)]))),
        case(
            "L01-legacy-island",
            Ok(group(vec![(
                "Legacy",
                repeated(vec![
                    flags(&[("Found", false)]),
                    flags(&[("Found", true)]),
                    flags(&[("Found", false)]),
                ]),
            )])),
        ),
        case("E01-later-error", Err(exception(50, "late"))),
        case("E02-producer-error", Err(exception(30, "producer"))),
        case(
            "E03-producer-type",
            Err(json!({"Err":{"kind":"FunctionType","function":"tokenize","detail":"xml nil"}})),
        ),
        case(
            "E04-not-bool",
            Err(json!({"Err":{"kind":"NotABool","node":101,"found":"string"}})),
        ),
        case(
            "E05-missing-owner",
            Err(
                json!({"Err":{"kind":"OwnedSource","node":301,"cause":"SourceUnavailable",
                "owner":projects::UNUSED,"native_scope":null}}),
            ),
        ),
        case(
            "E06-item-predicate",
            Err(json!({"Err":{"kind":"NotABool","node":90,"found":"string"}})),
        ),
    ]
}
