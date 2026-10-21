//! Independently authored inputs and complete expected outcomes. Every case is
//! called through all outputs, the Primary target and the `audit` target.
use super::*;
use ir::{XML_MIXED_CONTENT_FIELD, XML_MIXED_CONTENT_VALUE_FIELD, XML_NODE_NAME_FIELD};
use projects::{AUDIT, FOREST, LOOKUP, PREDICATE, TREE};

pub fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn int(value: i64) -> Instance {
    Instance::Scalar(Value::Int(value))
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
fn rep(items: Vec<Instance>) -> Instance {
    Instance::Repeated(items)
}
/// An item with only a name.
fn f(name: &str) -> Instance {
    group(vec![("name", text(name))])
}
/// An item without a name.
fn nameless() -> Instance {
    group(vec![("pos", int(2))])
}
/// An item carrying its expected positions and innermost group name.
fn at(pos: i64, dirpos: i64, dir: &str) -> Instance {
    group(vec![
        ("pos", int(pos)),
        ("dirpos", int(dirpos)),
        ("dir", text(dir)),
    ])
}
/// An item carrying its expected batch Id.
fn fb(batch: &str) -> Instance {
    group(vec![("batch", text(batch))])
}
/// One directory; `None` children leaves the child field absent.
pub fn dir(name: &str, files: Vec<Instance>, children: Option<Vec<Instance>>) -> Instance {
    let mut fields = vec![("name", text(name)), ("file", rep(files))];
    if let Some(children) = children {
        fields.push(("Dir", rep(children)));
    }
    group(fields)
}
fn endpoint(suffix: &str, dirs: Vec<Instance>, batches: Vec<Instance>) -> Instance {
    group(vec![
        ("Suffix", text(suffix)),
        ("Dir", rep(dirs)),
        ("Batches", rep(batches)),
    ])
}
fn batch(id: &str, dirs: Vec<Instance>) -> Instance {
    group(vec![("Id", text(id)), ("Dir", rep(dirs))])
}
fn primary_dirs() -> Vec<Instance> {
    vec![
        dir(
            "p1",
            vec![f("a.keep"), f("b.lk")],
            Some(vec![dir(
                "p1a",
                vec![f("c.keep"), nameless()],
                Some(vec![]),
            )]),
        ),
        dir("p2", vec![], Some(vec![dir("p3", vec![f("d.lk")], None)])),
    ]
}
fn primary_batches() -> Vec<Instance> {
    vec![
        batch(
            "B1",
            vec![dir(
                "x",
                vec![fb("B1"), fb("B2")],
                Some(vec![dir("y", vec![fb("B2"), fb("B1")], None)]),
            )],
        ),
        batch("B2", vec![dir("z", vec![fb("B1"), fb("B2")], Some(vec![]))]),
    ]
}
pub fn primary() -> Instance {
    endpoint(".keep", primary_dirs(), primary_batches())
}
fn primary_with(dirs: Vec<Instance>) -> Instance {
    endpoint(".keep", dirs, primary_batches())
}
/// Lookup shares Primary's shape with different names and suffix.
fn lookup_dirs() -> Vec<Instance> {
    vec![dir(
        "l1",
        vec![f("a.keep"), f("b.lk")],
        Some(vec![dir("x.lk", vec![nameless(), f("e.lk")], None)]),
    )]
}
fn lookup() -> Instance {
    endpoint(".lk", lookup_dirs(), vec![])
}
fn tree() -> Instance {
    dir(
        "t",
        vec![f("t.keep"), f("t.lk")],
        Some(vec![dir("u", vec![f("u.keep")], None)]),
    )
}
fn forest() -> Instance {
    rep(vec![
        dir("f1", vec![f("1.keep"), f("2.lk")], None),
        dir("f2", vec![f("3.lk")], Some(vec![])),
    ])
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
/// A leaf directory below `levels` single-child wrappers.
fn nest(levels: usize) -> Instance {
    let mut directory = dir("leaf", vec![f("x.keep")], None);
    for _ in 0..levels {
        directory = group(vec![("name", text("n")), ("Dir", rep(vec![directory]))]);
    }
    directory
}
/// Deep trees exceed portable JSON nesting limits, so a deep case carries only
/// `{"deep": levels}` and every engine builds this exact Primary input.
pub fn deep_source(levels: usize) -> Instance {
    group(vec![
        ("Suffix", text(".keep")),
        ("Dir", rep(vec![nest(levels)])),
    ])
}
/// Every item of a deep tree is kept, so the Primary output is its directories.
pub fn deep_output(levels: usize) -> Instance {
    out(vec![nest(levels)])
}
fn entry(name: &str, value: Instance) -> Instance {
    group(vec![
        (XML_NODE_NAME_FIELD, text(name)),
        (XML_MIXED_CONTENT_VALUE_FIELD, value),
    ])
}
fn with_order(mut directory: Instance, entries: Vec<Instance>) -> Instance {
    if let Instance::Group(fields) = &mut directory {
        fields.push((XML_MIXED_CONTENT_FIELD.into(), rep(entries)));
    }
    directory
}

pub enum Outcome {
    /// The Primary output tree, with the default audit tree.
    Outputs(Instance),
    OutputsWithAudit(Instance, Instance),
    /// A Primary-target error; the audit target still succeeds.
    PrimaryError(Json, Instance),
    /// Each selection fails at its own target's filter.
    Unsupplied(&'static str),
    Failure(usize, &'static str),
    /// The complete deep Primary tree is kept, with the default audit tree.
    Deep(usize),
}
fn out(dirs: Vec<Instance>) -> Instance {
    group(vec![("Dir", rep(dirs))])
}
fn audit(dirs: Vec<Instance>) -> Instance {
    out(dirs)
}
fn default_audit() -> Instance {
    audit(vec![dir(
        "l1",
        vec![f("b.lk")],
        Some(vec![dir("x.lk", vec![f("e.lk")], None)]),
    )])
}
fn primary_filtered() -> Vec<Instance> {
    vec![
        dir(
            "p1",
            vec![f("a.keep")],
            Some(vec![dir("p1a", vec![f("c.keep")], Some(vec![]))]),
        ),
        dir("p2", vec![], Some(vec![dir("p3", vec![], None)])),
    ]
}
fn unavailable(owner: &str, target: &str) -> Json {
    json!({"kind":"OwnedSource","cause":"SourceUnavailable","owner":owner,"target":target})
}
fn requires_collection(field: &str, found: &str) -> Json {
    json!({"kind":"RecursiveFilterRequiresCollection","field":field,"found":found})
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
                Outcome::PrimaryError(_, audit) if selection == AUDIT => ok(&group(vec![]), audit),
                Outcome::PrimaryError(recipe, _) => json!({ "Err": recipe }),
                Outcome::Unsupplied(owner) => json!({"Err": unavailable(owner,
                    if selection == AUDIT { AUDIT } else { "Primary" })}),
                Outcome::Failure(rule, message) => {
                    json!({"Err":{"kind":"MappingFailure","rule":rule,"message":message}})
                }
                Outcome::Deep(_) if selection == AUDIT => ok(&group(vec![]), &default_audit()),
                Outcome::Deep(levels) => json!({"OkDeep": {"levels": levels,
                    "audit": (selection == "All").then(|| codec::encode(&default_audit()))}}),
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
        deep: None,
    }
}
fn deep(id: &'static str, levels: usize, outcome: Outcome) -> Case {
    Case {
        deep: Some(levels),
        ..case(
            id,
            "F01-primary-dirs",
            deep_source(levels),
            inputs(),
            outcome,
        )
    }
}

pub fn cases() -> Vec<Case> {
    let positions = endpoint(
        "-",
        vec![
            dir(
                "a",
                vec![at(1, 1, "a"), at(9, 1, "a"), at(3, 1, "a")],
                Some(vec![
                    dir(
                        "b",
                        vec![at(1, 1, "b"), at(2, 9, "b")],
                        Some(vec![dir(
                            "c",
                            vec![at(1, 1, "a"), at(2, 1, "c")],
                            Some(vec![]),
                        )]),
                    ),
                    dir("d", vec![at(1, 2, "d")], None),
                ]),
            ),
            dir("e", vec![at(1, 2, "e"), at(2, 1, "e")], None),
        ],
        vec![],
    );
    // Nameless items never satisfy Lookup's own suffix in the audit filter.
    let positions_audit = audit(vec![
        dir(
            "a",
            vec![],
            Some(vec![
                dir("b", vec![], Some(vec![dir("c", vec![], Some(vec![]))])),
                dir("d", vec![], None),
            ]),
        ),
        dir("e", vec![], None),
    ]);
    let nested = with_order(
        dir("n", vec![f("k.keep"), f("d.txt")], Some(vec![])),
        vec![entry("file", f("k.keep")), entry("file", f("d.txt"))],
    );
    let ordered = with_order(
        dir("r", vec![f("d.txt"), f("r.keep")], Some(vec![nested])),
        vec![
            entry("file", f("d.txt")),
            entry("Dir", text("stale")),
            entry("file", f("r.keep")),
        ],
    );
    let nested_filtered = with_order(
        dir("n", vec![f("k.keep")], Some(vec![])),
        vec![entry("file", f("k.keep"))],
    );
    let ordered_filtered = with_order(
        dir("r", vec![f("r.keep")], Some(vec![nested_filtered.clone()])),
        vec![entry("Dir", nested_filtered), entry("file", f("r.keep"))],
    );
    let boom = endpoint(
        ".lk",
        vec![dir("ok", vec![], None), dir("boom", vec![], None)],
        vec![],
    );
    let no_items = vec![dir("p", vec![], Some(vec![dir("q", vec![], None)]))];
    vec![
        case(
            "F01",
            "F01-primary-dirs",
            primary(),
            inputs(),
            Outcome::Outputs(out(primary_filtered())),
        ),
        // An empty or absent selected collection never borrows Lookup's tree.
        case(
            "F01-empty",
            "F01-primary-dirs",
            primary_with(vec![]),
            inputs(),
            Outcome::Outputs(out(vec![])),
        ),
        case(
            "F01-absent",
            "F01-primary-dirs",
            group(vec![("Suffix", text(".keep"))]),
            inputs(),
            Outcome::Outputs(out(vec![])),
        ),
        // Copied ordered XML metadata follows the filtered items and children.
        case(
            "F01-ordered-xml",
            "F01-primary-dirs",
            primary_with(vec![ordered]),
            inputs(),
            Outcome::Outputs(out(vec![ordered_filtered])),
        ),
        case(
            "F01-malformed-children",
            "F01-primary-dirs",
            primary_with(vec![dir(
                "p",
                vec![],
                Some(vec![group(vec![("name", text("c")), ("Dir", text("bad"))])]),
            )]),
            inputs(),
            Outcome::PrimaryError(requires_collection("Dir", "scalar"), default_audit()),
        ),
        case(
            "F01-malformed-items",
            "F01-primary-dirs",
            primary_with(vec![group(vec![("name", text("p")), ("file", f("x"))])]),
            inputs(),
            Outcome::PrimaryError(requires_collection("file", "group"), default_audit()),
        ),
        case(
            "F01-child-not-group",
            "F01-primary-dirs",
            primary_with(vec![group(vec![
                ("name", text("p")),
                ("Dir", rep(vec![text("leaf")])),
            ])]),
            inputs(),
            Outcome::PrimaryError(
                json!({"kind":"RecursiveFilterRequiresGroup","found":"scalar"}),
                default_audit(),
            ),
        ),
        // The deepest admitted child is 255 levels below the scope's group.
        deep("F01-depth-255", 255, Outcome::Deep(255)),
        deep(
            "F01-depth-256",
            256,
            Outcome::PrimaryError(
                json!({"kind":"RecursiveFilterDepth","limit":256}),
                default_audit(),
            ),
        ),
        // Lookup items are kept by the Primary suffix; the nameless item never
        // reads its group's matching name.
        case(
            "F02",
            "F02-named-dirs",
            primary(),
            inputs(),
            Outcome::Outputs(out(vec![dir(
                "l1",
                vec![f("a.keep")],
                Some(vec![dir("x.lk", vec![], None)]),
            )])),
        ),
        case(
            "F02-absent",
            "F02-named-dirs",
            primary(),
            replace(LOOKUP, Some(group(vec![("Suffix", text(".lk"))]))),
            Outcome::OutputsWithAudit(out(vec![]), audit(vec![])),
        ),
        case(
            "F02-unsupplied",
            "F02-named-dirs",
            primary(),
            replace(LOOKUP, None),
            Outcome::Unsupplied(LOOKUP),
        ),
        case(
            "F03",
            "F03-named-root-tree",
            primary(),
            inputs(),
            Outcome::Outputs(out(vec![dir(
                "t",
                vec![f("t.keep")],
                Some(vec![dir("u", vec![f("u.keep")], None)]),
            )])),
        ),
        case(
            "F03-unsupplied",
            "F03-named-root-tree",
            primary(),
            replace(TREE, None),
            Outcome::PrimaryError(unavailable(TREE, "Primary"), default_audit()),
        ),
        case(
            "F04",
            "F04-named-repeated-root",
            primary(),
            inputs(),
            Outcome::Outputs(out(vec![
                dir("f1", vec![f("1.keep")], None),
                dir("f2", vec![], Some(vec![])),
            ])),
        ),
        case(
            "F04-empty",
            "F04-named-repeated-root",
            primary(),
            replace(FOREST, Some(rep(vec![]))),
            Outcome::Outputs(out(vec![])),
        ),
        case(
            "F05",
            "F05-recursive-positions",
            primary(),
            replace(LOOKUP, Some(positions)),
            Outcome::OutputsWithAudit(
                out(vec![
                    dir(
                        "a",
                        vec![at(1, 1, "a"), at(3, 1, "a")],
                        Some(vec![
                            dir(
                                "b",
                                vec![at(1, 1, "b")],
                                Some(vec![dir("c", vec![at(2, 1, "c")], Some(vec![]))]),
                            ),
                            dir("d", vec![at(1, 2, "d")], None),
                        ]),
                    ),
                    dir("e", vec![at(1, 2, "e")], None),
                ]),
                positions_audit,
            ),
        ),
        case(
            "F06",
            "F06-nested-batches",
            primary(),
            inputs(),
            Outcome::Outputs(group(vec![(
                "Batch",
                rep(vec![
                    out(vec![dir(
                        "x",
                        vec![fb("B1")],
                        Some(vec![dir("y", vec![fb("B1")], None)]),
                    )]),
                    out(vec![dir("z", vec![fb("B2")], Some(vec![]))]),
                ]),
            )])),
        ),
        // A legacy name read falls back to the nameless item's group.
        case(
            "F07",
            "F07-legacy-fallback",
            primary(),
            inputs(),
            Outcome::Outputs(out(vec![dir(
                "l1",
                vec![f("b.lk")],
                Some(vec![dir("x.lk", vec![nameless(), f("e.lk")], None)]),
            )])),
        ),
        case(
            "F08",
            "F08-composed-companions",
            primary(),
            inputs(),
            // Bindings precede child scopes in a constructed group.
            Outcome::Outputs(group(vec![
                ("First", text("/l1/a.keep")),
                ("Dir", rep(primary_filtered())),
            ])),
        ),
        // Failure rules run before any target and end the run at the first row.
        case(
            "F08-failure",
            "F08-composed-companions",
            primary(),
            replace(LOOKUP, Some(boom)),
            Outcome::Failure(1, "boom"),
        ),
        case(
            "F09",
            "F09-not-a-bool",
            primary(),
            inputs(),
            Outcome::PrimaryError(
                json!({"kind":"NotABool","node":PREDICATE,"found":"string"}),
                default_audit(),
            ),
        ),
        case(
            "F09-no-items",
            "F09-not-a-bool",
            primary_with(no_items.clone()),
            inputs(),
            Outcome::Outputs(out(no_items)),
        ),
    ]
}
