//! Independently authored inputs and complete expected outcomes. Every case is
//! called through all outputs, the Primary target and the `audit` target.
use super::*;
use projects::{AUDIT, AUDIT_NODE, FIRST_NODE, FOREST, LOOKUP, TREE};

pub fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn null() -> Instance {
    Instance::Scalar(Value::Null)
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
/// One directory: its Note, leaf file Notes and child directories.
pub fn dir(note: &str, files: &[&str], children: Vec<Instance>) -> Instance {
    group(vec![
        ("Note", text(note)),
        (
            "file",
            Instance::Repeated(
                files
                    .iter()
                    .map(|file| group(vec![("Note", text(file))]))
                    .collect(),
            ),
        ),
        ("Dir", Instance::Repeated(children)),
    ])
}
fn batch(id: &str, dirs: Vec<Instance>) -> Instance {
    group(vec![("Id", text(id)), ("Dir", Instance::Repeated(dirs))])
}
fn endpoint(
    note: &str,
    sep: Instance,
    pick: i64,
    dirs: Vec<Instance>,
    batches: Vec<Instance>,
) -> Instance {
    group(vec![
        ("Note", text(note)),
        ("Sep", sep),
        ("Pick", Instance::Scalar(Value::Int(pick))),
        ("Dir", Instance::Repeated(dirs)),
        ("Rows", Instance::Repeated(adjacency_rows())),
        ("Batches", Instance::Repeated(batches)),
    ])
}
/// Adjacency rows: a and c parent under the walk values /n/m and /p/a.
fn adjacency_rows() -> Vec<Instance> {
    [("a", "/n/m"), ("b", "a"), ("c", "/p/a")]
        .into_iter()
        .map(|(key, parent)| group(vec![("Key", text(key)), ("Parent", text(parent))]))
        .collect()
}
fn node(name: &str, children: Vec<Instance>) -> Instance {
    group(vec![
        ("name", text(name)),
        ("children", Instance::Repeated(children)),
    ])
}
/// Primary walks to /p/a, /p/sub/b, /q/c.
pub fn primary_dirs() -> Vec<Instance> {
    vec![
        dir("p", &["a"], vec![dir("sub", &["b"], vec![])]),
        dir("q", &["c"], vec![]),
    ]
}
fn primary_batches() -> Vec<Instance> {
    vec![
        batch("B1", vec![dir("b1", &["x1"], vec![])]),
        batch(
            "B2",
            vec![dir("b2", &["x2", "y2"], vec![dir("deep", &["z2"], vec![])])],
        ),
    ]
}
pub fn primary() -> Instance {
    endpoint("P.note", text("|"), 2, primary_dirs(), primary_batches())
}
fn primary_with(dirs: Vec<Instance>) -> Instance {
    endpoint("P.note", text("|"), 2, dirs, primary_batches())
}
/// Lookup walks to /n/m, /n/o/k; its batches to /l1/u and /l2/v.
pub fn lookup() -> Instance {
    endpoint(
        "N.note",
        text("#"),
        1,
        vec![dir("n", &["m"], vec![dir("o", &["k"], vec![])])],
        vec![
            batch("L1", vec![dir("l1", &["u"], vec![])]),
            batch("L2", vec![dir("l2", &["v"], vec![])]),
        ],
    )
}
fn tree() -> Instance {
    dir("t", &["root.txt"], vec![dir("c", &["x"], vec![])])
}
fn forest() -> Instance {
    Instance::Repeated(vec![dir("f1", &["a"], vec![]), dir("f2", &["b"], vec![])])
}
fn inputs() -> Vec<(String, Instance)> {
    vec![
        (LOOKUP.into(), lookup()),
        (TREE.into(), tree()),
        (FOREST.into(), forest()),
    ]
}
fn replace(name: &str, value: Option<Instance>) -> Vec<(String, Instance)> {
    inputs()
        .into_iter()
        .filter_map(|(input, tree)| match input == name {
            true => value.clone().map(|value| (input, value)),
            false => Some((input, tree)),
        })
        .collect()
}

pub enum Outcome {
    /// The Primary output tree, with the default audit value.
    Outputs(Instance),
    OutputsWithAudit(Instance, Instance),
    /// A Primary-target error; the audit target still succeeds.
    PrimaryError(Json, Instance),
    /// Each selection fails at its own first reached consumer.
    Unsupplied(&'static str),
    Failure(usize, &'static str),
}
fn audit(first: Instance) -> Instance {
    group(vec![("First", first)])
}
fn default_audit() -> Instance {
    audit(text("/n/m"))
}
fn values(values: &[Option<&str>]) -> Instance {
    group(
        ["V1", "V2", "V3", "V4", "V5"]
            .into_iter()
            .zip(values)
            .map(|(name, value)| (name, value.map_or_else(null, text)))
            .collect(),
    )
}
fn unavailable(owner: &str, node: u32) -> Json {
    json!({"kind":"OwnedSource","cause":"SourceUnavailable","owner":owner,"node":node})
}
fn type_mismatch() -> Json {
    json!({"kind":"FunctionType","function":"recursive-collect","detail":"null"})
}
fn calls(outcome: &Outcome) -> Vec<Json> {
    ["All", "Primary", AUDIT]
        .into_iter()
        .map(|selection| {
            let ok = |primary: &Instance, audit: &Instance| {
                let primary = json!([null, codec::encode(primary)]);
                let audit = json!([AUDIT, codec::encode(audit)]);
                match selection {
                    "All" => json!({"Ok":[primary, audit]}),
                    "Primary" => json!({"Ok":[primary]}),
                    _ => json!({"Ok":[audit]}),
                }
            };
            let expected = match outcome {
                Outcome::Outputs(primary) => ok(primary, &default_audit()),
                Outcome::OutputsWithAudit(primary, audit) => ok(primary, audit),
                Outcome::PrimaryError(recipe, audit) if selection == AUDIT => {
                    ok(&group(vec![]), audit)
                }
                Outcome::PrimaryError(recipe, _) => json!({ "Err": recipe }),
                Outcome::Unsupplied(owner) => json!({"Err": unavailable(owner,
                    if selection == AUDIT { AUDIT_NODE } else { FIRST_NODE })}),
                Outcome::Failure(rule, message) => {
                    json!({"Err":{"kind":"MappingFailure","rule":rule,"message":message}})
                }
            };
            json!({"selection":selection,"expected":expected})
        })
        .collect()
}
fn case(
    id: &'static str,
    profile: &'static str,
    source: Instance,
    inputs: Vec<(String, Instance)>,
    outcome: Outcome,
) -> Case {
    Case {
        id,
        profile,
        calls: calls(&outcome),
        source,
        inputs,
    }
}

pub fn cases() -> Vec<Case> {
    let no_note = group(vec![
        (
            "file",
            Instance::Repeated(vec![group(vec![("Note", text("hidden"))])]),
        ),
        (
            "Dir",
            Instance::Repeated(vec![dir("inner", &["i"], vec![])]),
        ),
    ]);
    let null_leaf = group(vec![
        ("Note", text("d")),
        (
            "file",
            Instance::Repeated(vec![group(vec![("Note", null())])]),
        ),
    ]);
    let preorder = dir(
        "r",
        &["1", "2"],
        vec![
            dir("s", &["3"], vec![dir("t", &["4"], vec![])]),
            dir("u", &["5"], vec![]),
        ],
    );
    let no_lookup_note = group(vec![
        ("Sep", text("#")),
        ("Pick", Instance::Scalar(Value::Int(1))),
        ("Dir", Instance::Repeated(vec![dir("n", &["m"], vec![])])),
    ]);
    let boom = endpoint(
        "N.note",
        text("#"),
        3,
        vec![dir("ok", &[], vec![]), dir("boom", &[], vec![])],
        vec![],
    );
    vec![
        case(
            "R01",
            "R01-primary-dirs",
            primary(),
            inputs(),
            Outcome::Outputs(values(&[
                Some("/p/a"),
                Some("/p/sub/b"),
                Some("/q/c"),
                None,
                None,
            ])),
        ),
        // Parent leaves precede each child subtree, in source order.
        case(
            "R01-preorder",
            "R01-primary-dirs",
            primary_with(vec![preorder]),
            inputs(),
            Outcome::Outputs(values(&[
                Some("/r/1"),
                Some("/r/2"),
                Some("/r/s/3"),
                Some("/r/s/t/4"),
                Some("/r/u/5"),
            ])),
        ),
        // An empty or absent selected collection never borrows Lookup's tree.
        case(
            "R01-empty",
            "R01-primary-dirs",
            primary_with(vec![]),
            inputs(),
            Outcome::Outputs(values(&[None; 5])),
        ),
        case(
            "R01-absent",
            "R01-primary-dirs",
            group(vec![("Note", text("P.note"))]),
            inputs(),
            Outcome::Outputs(values(&[None; 5])),
        ),
        // A group without its descent value is skipped with its subtree.
        case(
            "R01-missing-descent",
            "R01-primary-dirs",
            primary_with(vec![no_note, dir("q", &["c"], vec![])]),
            inputs(),
            Outcome::Outputs(values(&[Some("/q/c"), None, None, None, None])),
        ),
        case(
            "R01-null-leaf",
            "R01-primary-dirs",
            primary_with(vec![null_leaf]),
            inputs(),
            Outcome::PrimaryError(type_mismatch(), default_audit()),
        ),
        case(
            "R02",
            "R02-named-dirs",
            primary(),
            inputs(),
            Outcome::Outputs(values(&[Some("/n/m"), Some("/n/o/k"), None, None, None])),
        ),
        case(
            "R02-absent",
            "R02-named-dirs",
            primary(),
            replace(LOOKUP, Some(group(vec![("Note", text("N.note"))]))),
            Outcome::OutputsWithAudit(values(&[None; 5]), audit(null())),
        ),
        case(
            "R02-unsupplied",
            "R02-named-dirs",
            primary(),
            replace(LOOKUP, None),
            Outcome::Unsupplied(LOOKUP),
        ),
        case(
            "R03",
            "R03-named-root-tree",
            primary(),
            inputs(),
            Outcome::Outputs(values(&[
                Some("/t/root.txt"),
                Some("/t/c/x"),
                None,
                None,
                None,
            ])),
        ),
        case(
            "R03-unsupplied",
            "R03-named-root-tree",
            primary(),
            replace(TREE, None),
            Outcome::PrimaryError(unavailable(TREE, FIRST_NODE), default_audit()),
        ),
        case(
            "R04",
            "R04-named-repeated-root",
            primary(),
            inputs(),
            Outcome::Outputs(values(&[Some("/f1/a"), Some("/f2/b"), None, None, None])),
        ),
        case(
            "R04-empty",
            "R04-named-repeated-root",
            primary(),
            replace(FOREST, Some(Instance::Repeated(vec![]))),
            Outcome::Outputs(values(&[None; 5])),
        ),
        case(
            "R05",
            "R05-owned-parent-arguments",
            primary(),
            inputs(),
            Outcome::Outputs(group(vec![
                ("V1", text("N.note|p|a")),
                ("V2", text("N.note|p|sub|b")),
                ("V3", text("N.note|q|c")),
            ])),
        ),
        // Absent and null parent arguments are empty text.
        case(
            "R05-null-arguments",
            "R05-owned-parent-arguments",
            endpoint("P.note", null(), 2, primary_dirs(), vec![]),
            replace(LOOKUP, Some(no_lookup_note)),
            Outcome::Outputs(group(vec![
                ("V1", text("pa")),
                ("V2", text("psubb")),
                ("V3", text("qc")),
            ])),
        ),
        case(
            "R06",
            "R06-owned-index",
            primary(),
            inputs(),
            Outcome::Outputs(group(vec![("V1", text("/p/a")), ("V2", text("/n/o/k"))])),
        ),
        case(
            "R06-out-of-range",
            "R06-owned-index",
            endpoint("P.note", text("|"), 9, primary_dirs(), vec![]),
            inputs(),
            Outcome::Outputs(group(vec![("V1", text("/p/a")), ("V2", null())])),
        ),
        case(
            "R07",
            "R07-nested-pinned",
            primary(),
            inputs(),
            Outcome::Outputs(group(vec![(
                "Batches",
                Instance::Repeated(vec![
                    group(vec![
                        ("Id", text("B1")),
                        ("Own1", text("/b1/x1")),
                        ("Own2", null()),
                        ("Other1", text("/l1/u")),
                        ("Other2", text("/l2/v")),
                        ("Top", text("/p/a")),
                    ]),
                    group(vec![
                        ("Id", text("B2")),
                        ("Own1", text("/b2/x2")),
                        ("Own2", text("/b2/y2")),
                        ("Other1", text("/l1/u")),
                        ("Other2", text("/l2/v")),
                        ("Top", text("/p/a")),
                    ]),
                ]),
            )])),
        ),
        case(
            "R08",
            "R08-failure-and-scalar-producers",
            primary(),
            inputs(),
            Outcome::Outputs(group(vec![("V1", text("/p/a")), ("V2", null())])),
        ),
        case(
            "R08-range",
            "R08-failure-and-scalar-producers",
            primary(),
            replace(
                LOOKUP,
                Some(endpoint(
                    "N.note",
                    text("#"),
                    3,
                    vec![dir("ok", &[], vec![])],
                    vec![],
                )),
            ),
            Outcome::OutputsWithAudit(
                group(vec![
                    ("V1", text("/p/a")),
                    ("V2", Instance::Scalar(Value::Int(2))),
                ]),
                audit(null()),
            ),
        ),
        // The tree's root is the first Lookup walk value; rows stay Primary's.
        case(
            "R09",
            "R09-adjacency-tree-root",
            primary(),
            inputs(),
            Outcome::Outputs(group(vec![("Tree", node("a", vec![node("b", vec![])]))])),
        ),
        case(
            "R09-other-root",
            "R09-adjacency-tree-root",
            primary(),
            replace(
                LOOKUP,
                Some(endpoint(
                    "N.note",
                    text("#"),
                    1,
                    vec![dir("p", &["a"], vec![])],
                    vec![],
                )),
            ),
            Outcome::OutputsWithAudit(
                group(vec![("Tree", node("c", vec![]))]),
                audit(text("/p/a")),
            ),
        ),
        case(
            "R09-unsupplied",
            "R09-adjacency-tree-root",
            primary(),
            replace(LOOKUP, None),
            Outcome::Unsupplied(LOOKUP),
        ),
        // Failure rules run before any target and end the run at the first row.
        case(
            "R08-failure",
            "R08-failure-and-scalar-producers",
            primary(),
            replace(LOOKUP, Some(boom)),
            Outcome::Failure(1, "boom"),
        ),
    ]
}
