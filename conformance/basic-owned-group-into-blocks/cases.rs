//! Literal inputs and expected outputs. Helpers only expand listed fields and
//! members; none partition, sort, filter, window or execute input data.
use super::*;

pub fn scalar(value: Value) -> Instance {
    Instance::Scalar(value)
}
pub fn text(s: &str) -> Instance {
    scalar(Value::String(s.into()))
}
pub fn integer(n: i64) -> Instance {
    scalar(Value::Int(n))
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
fn row(label: &str, keep: bool, rank: i64) -> Instance {
    group(vec![
        ("Label", text(label)),
        ("Keep", scalar(Value::Bool(keep))),
        ("Rank", integer(rank)),
        ("Limit", integer(0)),
        ("Key", text("A")),
    ])
}
fn endpoint(marker: &str, limit: i64, rows: Vec<Instance>) -> Instance {
    group(vec![
        ("Marker", text(marker)),
        ("Limit", integer(limit)),
        ("Rows", repeated(rows)),
    ])
}
pub fn source() -> Instance {
    endpoint(
        "P.root",
        2,
        vec![
            row("p0", false, 30),
            row("p1", true, 10),
            row("p2", false, 20),
            row("p3", false, 25),
            row("p4", true, 40),
            row("p5", false, 50),
            row("p6", true, 60),
        ],
    )
}
pub fn inputs() -> Vec<(String, Instance)> {
    vec![
        (
            "Rows".into(),
            endpoint(
                "N.root",
                1,
                vec![
                    row("n0", false, 300),
                    row("n1", true, 100),
                    row("n2", false, 200),
                    row("n3", false, 250),
                    row("n4", true, 400),
                    row("n5", false, 500),
                    row("n6", true, 600),
                ],
            ),
        ),
        (
            projects::EXACT.into(),
            endpoint("U.root", 2, vec![row("u0", true, 7)]),
        ),
    ]
}
/// One expected group: ordinal, representative, root marker and member labels.
pub fn g(position: i64, first: &str, marker: &str, labels: &[&str]) -> Instance {
    group(vec![
        ("First", text(first)),
        ("GroupPosition", integer(position)),
        ("RootMarker", text(marker)),
        (
            "Members",
            repeated(
                labels
                    .iter()
                    .enumerate()
                    .map(|(i, label)| {
                        group(vec![
                            ("Label", text(label)),
                            ("MemberPosition", integer(i as i64 + 1)),
                            ("LegacyPosition", integer(i as i64 + 1)),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}
pub fn p(position: i64, labels: &[&str]) -> Instance {
    g(position, labels[0], "P.root", labels)
}
pub fn o(groups: Vec<Instance>) -> Instance {
    group(vec![("Groups", repeated(groups))])
}
/// N.root Limit is 1: every named block holds one member.
pub fn n_runs() -> Vec<Instance> {
    (0..7)
        .map(|i| {
            let label = format!("n{i}");
            g(i + 1, &label, "N.root", &[&label])
        })
        .collect()
}
pub fn u_runs() -> Vec<Instance> {
    vec![g(1, "u0", "U.root", &["u0"])]
}
fn ok(outputs: Vec<(Option<&str>, Instance)>) -> Json {
    json!({"Ok": outputs
        .into_iter()
        .map(|(name, tree)| json!([name, codec::encode(&tree)]))
        .collect::<Vec<_>>()})
}
fn raised(node: u32, message: &str) -> Json {
    json!({"Err":{"kind":"MappingException","node":node,"message":message}})
}
fn outer(first: &str, position: i64, inner: Vec<Instance>) -> Instance {
    group(vec![
        ("OuterFirst", text(first)),
        ("OuterPosition", integer(position)),
        ("Inner", o(inner)),
    ])
}

/// All, Primary and both named selections. Named targets always group their
/// own endpoints with their own size; a Primary failure fails All but not a
/// named selection.
fn case(id: &'static str, profile: &'static str, source: Instance, primary: Json) -> Case {
    let named = [
        (projects::AUDIT, o(n_runs())),
        (projects::UNICODE_TARGET, o(u_runs())),
    ];
    let all = match primary.get("Ok") {
        Some(outputs) => {
            let mut outputs = outputs.as_array().unwrap().clone();
            for (name, tree) in &named {
                outputs.push(json!([name, codec::encode(tree)]));
            }
            json!({ "Ok": outputs })
        }
        None => primary.clone(),
    };
    let mut calls = vec![
        json!({"selection":"All","expected":all}),
        json!({"selection":"Primary","expected":primary}),
    ];
    for (name, tree) in named {
        calls.push(json!({"selection":name,"expected":ok(vec![(Some(name), tree)])}));
    }
    Case {
        id,
        profile,
        source,
        inputs: inputs(),
        calls,
        native: true,
    }
}
fn success(profile: &'static str, tree: Instance) -> Case {
    case(profile, profile, source(), ok(vec![(None, tree)]))
}
fn failure(profile: &'static str, error: Json) -> Case {
    case(profile, profile, source(), error)
}

fn count_error(kind: &str, node: u32, found: Option<&str>) -> Json {
    json!({"Err":{"kind":kind,"node":node,"found":found}})
}
fn pairs() -> Instance {
    o(vec![
        p(1, &["p0", "p1"]),
        p(2, &["p2", "p3"]),
        p(3, &["p4", "p5"]),
        p(4, &["p6"]),
    ])
}
fn empty() -> Instance {
    endpoint("P.root", 2, vec![])
}

pub fn cases() -> Vec<Case> {
    vec![
        success("B01-base", pairs()),
        case(
            "B01-empty",
            "B01-base",
            empty(),
            ok(vec![(None, o(vec![]))]),
        ),
        success(
            "B02-size-three",
            o(vec![
                p(1, &["p0", "p1", "p2"]),
                p(2, &["p3", "p4", "p5"]),
                p(3, &["p6"]),
            ]),
        ),
        success(
            "B03-size-one",
            o(vec![
                p(1, &["p0"]),
                p(2, &["p1"]),
                p(3, &["p2"]),
                p(4, &["p3"]),
                p(5, &["p4"]),
                p(6, &["p5"]),
                p(7, &["p6"]),
            ]),
        ),
        success(
            "B04-size-whole",
            o(vec![p(1, &["p0", "p1", "p2", "p3", "p4", "p5", "p6"])]),
        ),
        // A finite float truncates like every item count.
        success("B05-size-float", pairs()),
        success(
            "B06-post",
            o(vec![
                p(1, &["p0", "p1"]),
                p(2, &["p4", "p5"]),
                p(3, &["p6"]),
            ]),
        ),
        success("B07-filter", o(vec![p(1, &["p1", "p4"]), p(2, &["p6"])])),
        success(
            "B08-sort",
            o(vec![
                p(1, &["p1", "p2"]),
                p(2, &["p3", "p0"]),
                p(3, &["p4", "p5"]),
                p(4, &["p6"]),
            ]),
        ),
        success(
            "B09-parent-window",
            o(vec![p(1, &["p0", "p1"]), p(2, &["p2", "p3"])]),
        ),
        success("B10-last", o(vec![p(1, &["p4", "p5"]), p(2, &["p6"])])),
        success(
            "B11-from-to",
            o(vec![p(1, &["p2", "p3"]), p(2, &["p4", "p5"])]),
        ),
        success(
            "B12-position-filter",
            o(vec![
                p(1, &["p1", "p2"]),
                p(2, &["p3", "p4"]),
                p(3, &["p5", "p6"]),
            ]),
        ),
        success("B13-first", group(vec![("Groups", p(1, &["p0", "p1"]))])),
        success(
            "B14-nested-owners",
            group(vec![(
                "Outer",
                repeated(vec![outer("p0", 1, n_runs()), outer("p2", 2, n_runs())]),
            )]),
        ),
        success(
            "B15-nested-members",
            group(vec![(
                "Outer",
                repeated(vec![
                    outer("p0", 1, vec![p(1, &["p0", "p1"]), p(2, &["p2"])]),
                    outer("p3", 2, vec![p(1, &["p3", "p4"]), p(2, &["p5"])]),
                    outer("p6", 3, vec![p(1, &["p6"])]),
                ]),
            )]),
        ),
        failure("B16-size-zero", count_error("InvalidBlockSize", 910, None)),
        // Even an empty cohort evaluates its size.
        case(
            "B16-empty",
            "B16-size-zero",
            empty(),
            count_error("InvalidBlockSize", 910, None),
        ),
        failure(
            "B17-size-negative",
            count_error("InvalidBlockSize", 914, None),
        ),
        failure(
            "B18-size-null",
            count_error("NotAnItemCount", 915, Some("null")),
        ),
        failure(
            "B19-size-string",
            count_error("NotAnItemCount", 916, Some("string")),
        ),
        failure("B20-size-raise", raised(103, "size")),
        failure("B21-bound-before-size", raised(104, "bound")),
        failure(
            "B22-size-before-filter",
            count_error("InvalidBlockSize", 910, None),
        ),
        failure("B23-post-raise", raised(105, "post")),
        failure(
            "B24-item-size",
            json!({"Err":{"kind":"OwnedSource","node":4,"cause":"InactiveSourceFrame",
                "owner":"Primary","path":[],"native_path":[],"segment":null,"found":null,
                "collection":["Rows"],"native_scope":null}}),
        ),
        success("B25-unselected-targets", o(vec![])),
    ]
}
