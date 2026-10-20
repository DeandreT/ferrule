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
fn row(label: &str, end: bool, keep: bool, rank: i64) -> Instance {
    group(vec![
        ("End", scalar(Value::Bool(end))),
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
            row("p0", false, false, 30),
            row("p1", false, true, 10),
            row("p2", true, false, 20),
            row("p3", true, false, 25),
            row("p4", false, true, 40),
            row("p5", true, false, 50),
            row("p6", false, true, 60),
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
                    row("n0", true, false, 300),
                    row("n1", false, true, 100),
                    row("n2", false, false, 200),
                    row("n3", true, false, 250),
                    row("n4", false, true, 400),
                    row("n5", false, false, 500),
                    row("n6", true, true, 600),
                ],
            ),
        ),
        (
            projects::EXACT.into(),
            endpoint("U.root", 2, vec![row("u0", false, true, 7)]),
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
pub fn n_runs() -> Vec<Instance> {
    vec![
        g(1, "n0", "N.root", &["n0"]),
        g(2, "n1", "N.root", &["n1", "n2", "n3"]),
        g(3, "n4", "N.root", &["n4", "n5", "n6"]),
    ]
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
/// own endpoints; a Primary failure fails All but not a named selection.
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

pub fn cases() -> Vec<Case> {
    vec![
        success(
            "E01-base",
            o(vec![
                p(1, &["p0", "p1", "p2"]),
                p(2, &["p3"]),
                p(3, &["p4", "p5"]),
                p(4, &["p6"]),
            ]),
        ),
        case(
            "E01-empty",
            "E01-base",
            endpoint("P.root", 2, vec![]),
            ok(vec![(None, o(vec![]))]),
        ),
        success(
            "E02-all-false",
            o(vec![p(1, &["p0", "p1", "p2", "p3", "p4", "p5", "p6"])]),
        ),
        success(
            "E03-all-true",
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
            "E04-post",
            o(vec![
                p(1, &["p0", "p1", "p2"]),
                p(2, &["p4", "p5"]),
                p(3, &["p6"]),
            ]),
        ),
        success("E05-filter", o(vec![p(1, &["p0", "p1", "p4", "p6"])])),
        success(
            "E06-sort",
            o(vec![
                p(1, &["p1", "p2"]),
                p(2, &["p3"]),
                p(3, &["p0", "p4", "p5"]),
                p(4, &["p6"]),
            ]),
        ),
        success(
            "E07-parent-window",
            o(vec![p(1, &["p0", "p1", "p2"]), p(2, &["p3"])]),
        ),
        success("E08-last", o(vec![p(1, &["p4", "p5"]), p(2, &["p6"])])),
        success("E09-from-to", o(vec![p(1, &["p3"]), p(2, &["p4", "p5"])])),
        success(
            "E10-position-boundary",
            o(vec![
                p(1, &["p0", "p1"]),
                p(2, &["p2", "p3", "p4", "p5", "p6"]),
            ]),
        ),
        success(
            "E11-first",
            group(vec![("Groups", p(1, &["p0", "p1", "p2"]))]),
        ),
        success(
            "E12-nested-owners",
            group(vec![(
                "Outer",
                repeated(vec![outer("p0", 1, n_runs()), outer("p3", 2, n_runs())]),
            )]),
        ),
        success(
            "E13-nested-members",
            group(vec![(
                "Outer",
                repeated(vec![
                    outer("p0", 1, vec![p(1, &["p0", "p1"]), p(2, &["p2"])]),
                    outer("p3", 2, vec![p(1, &["p3"])]),
                    outer("p4", 3, vec![p(1, &["p4"]), p(2, &["p5"])]),
                    outer("p6", 4, vec![p(1, &["p6"])]),
                ]),
            )]),
        ),
        failure("E14-boundary-raise", raised(101, "boundary")),
        failure(
            "E15-boundary-not-bool",
            json!({"Err":{"kind":"NotABool","node":920,"found":"string"}}),
        ),
        failure("E16-post-raise", raised(105, "post")),
        failure("E17-last-boundary", raised(101, "boundary")),
        failure("E18-bound-before-boundary", raised(104, "bound")),
        success("E19-unselected-targets", o(vec![])),
    ]
}
