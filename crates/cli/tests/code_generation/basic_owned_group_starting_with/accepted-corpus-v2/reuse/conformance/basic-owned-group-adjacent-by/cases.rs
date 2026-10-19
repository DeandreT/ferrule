//! Literal complete trees/errors. This module never evaluates, partitions,
//! selects, sorts, filters, windows or consumes a generated sequence.
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
pub fn row(key: Value, label: &str, keep: bool, rank: i64) -> Instance {
    group(vec![
        ("Key", scalar(key)),
        ("Label", text(label)),
        ("Keep", boolean(keep)),
        ("Rank", integer(rank)),
        ("Limit", integer(0)),
    ])
}
fn r(key: &str, label: &str, keep: bool, rank: i64) -> Instance {
    row(Value::String(key.into()), label, keep, rank)
}
pub fn endpoint(marker: &str, limit: i64, rows: Vec<Instance>) -> Instance {
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
            r("A", "P-a1", false, 30),
            r("B", "P-b1", true, 10),
            r("A", "P-a2", false, 20),
            r("A", "P-a3", true, 25),
            r("C", "P-c1", false, 40),
            r("A", "P-a4", true, 50),
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
                    r("A", "N-a1", true, 300),
                    r("B", "N-b1", false, 100),
                    r("A", "N-a2", true, 200),
                    r("A", "N-a3", false, 250),
                    r("C", "N-c1", true, 400),
                    r("A", "N-a4", false, 500),
                ],
            ),
        ),
        (
            projects::EXACT.into(),
            endpoint("U.root", 2, vec![r("A", "U-only", true, 7)]),
        ),
    ]
}
pub fn replace(value: &mut Instance, name: &str, new: Instance) {
    let Instance::Group(fields) = value else {
        panic!("literal group")
    };
    fields.iter_mut().find(|(key, _)| key == name).unwrap().1 = new;
}
fn append(value: &mut Instance, name: &str, new: Instance) {
    let Instance::Group(fields) = value else {
        panic!("literal group")
    };
    fields.push((name.into(), new));
}
fn m(position: i64, label: &str) -> Instance {
    group(vec![
        ("Label", text(label)),
        ("MemberPosition", integer(position)),
        ("LegacyPosition", integer(position)),
    ])
}
fn members(labels: &[&str]) -> Instance {
    repeated(
        labels
            .iter()
            .enumerate()
            .map(|(i, label)| m(i as i64 + 1, label))
            .collect(),
    )
}
pub fn g(position: i64, first: &str, marker: &str, labels: &[&str]) -> Instance {
    group(vec![
        ("First", text(first)),
        ("GroupPosition", integer(position)),
        ("RootMarker", text(marker)),
        ("Members", members(labels)),
    ])
}
fn root_g(position: i64, first: &str, labels: &[&str]) -> Instance {
    group(vec![
        ("First", text(first)),
        ("GroupPosition", integer(position)),
        ("Members", members(labels)),
    ])
}
pub fn o(groups: Vec<Instance>) -> Instance {
    group(vec![("Groups", repeated(groups))])
}
pub fn p5() -> Vec<Instance> {
    vec![
        g(1, "P-a1", "P.root", &["P-a1"]),
        g(2, "P-b1", "P.root", &["P-b1"]),
        g(3, "P-a2", "P.root", &["P-a2", "P-a3"]),
        g(4, "P-c1", "P.root", &["P-c1"]),
        g(5, "P-a4", "P.root", &["P-a4"]),
    ]
}
fn n5() -> Vec<Instance> {
    vec![
        g(1, "N-a1", "N.root", &["N-a1"]),
        g(2, "N-b1", "N.root", &["N-b1"]),
        g(3, "N-a2", "N.root", &["N-a2", "N-a3"]),
        g(4, "N-c1", "N.root", &["N-c1"]),
        g(5, "N-a4", "N.root", &["N-a4"]),
    ]
}
fn post() -> Vec<Instance> {
    vec![
        g(1, "P-b1", "P.root", &["P-b1"]),
        g(2, "P-a2", "P.root", &["P-a2", "P-a3"]),
        g(3, "P-a4", "P.root", &["P-a4"]),
    ]
}
fn sorted_first_two() -> Instance {
    o(vec![
        g(1, "P-b1", "P.root", &["P-b1"]),
        g(2, "P-a2", "P.root", &["P-a2", "P-a3", "P-a1"]),
    ])
}
pub fn ok(outputs: Vec<(Option<&str>, Instance)>) -> Json {
    json!({"Ok":outputs.iter().map(|(name,tree)|json!([name,codec::encode(tree)])).collect::<Vec<_>>()})
}
pub fn expected(tree: Instance) -> Json {
    ok(vec![(None, tree)])
}
pub fn raised(node: u32, message: &str) -> Json {
    json!({"Err":{"kind":"MappingException","node":node,"message":message}})
}
pub fn owned(
    node: Option<u32>,
    cause: &str,
    name: &str,
    path: &[&str],
    native_path: &[&str],
    segment: Option<usize>,
    collection: Option<&[&str]>,
) -> Json {
    json!({"Err":{"kind":"OwnedSource","node":node,"cause":cause,"owner":name,"path":path,"native_path":native_path,"segment":segment,"found":null,"collection":collection,"native_scope":null}})
}
fn audit(profile: &str) -> Instance {
    group(vec![(
        "Marker",
        text(
            if profile.contains("-N")
                || profile.contains("Alias")
                || profile == "E15"
                || profile.starts_with("E16")
            {
                "audit"
            } else {
                "N.root"
            },
        ),
    )])
}
fn unicode() -> Instance {
    group(vec![("Marker", text("U.root"))])
}
fn normal(id: &'static str, profile: &'static str, outcome: Json) -> Case {
    let all = if let Some(outputs) = outcome.get("Ok") {
        let mut values = outputs.as_array().unwrap().clone();
        values.push(json!([projects::AUDIT, codec::encode(&audit(profile))]));
        values.push(json!([projects::UNICODE_TARGET, codec::encode(&unicode())]));
        json!({"Ok":values})
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
            json!({"selection":projects::AUDIT,"expected":ok(vec![(Some(projects::AUDIT),audit(profile))])}),
        ],
    }
}
fn success(profile: &'static str, tree: Instance) -> Case {
    normal(profile, profile, expected(tree))
}
fn empty(profile: &'static str) -> Case {
    success(profile, o(vec![]))
}
fn root_input() -> Instance {
    repeated(vec![
        group(vec![("Key", text("A")), ("Label", text("a1"))]),
        group(vec![("Key", text("B")), ("Label", text("b1"))]),
        group(vec![("Key", text("A")), ("Label", text("a2"))]),
        group(vec![("Key", text("A")), ("Label", text("a3"))]),
    ])
}
fn root_output() -> Instance {
    repeated(vec![
        root_g(1, "a1", &["a1"]),
        root_g(2, "b1", &["b1"]),
        root_g(3, "a2", &["a2", "a3"]),
    ])
}
fn outer(first: &str, position: i64, inner: Instance) -> Instance {
    group(vec![
        ("OuterFirst", text(first)),
        ("OuterPosition", integer(position)),
        ("Inner", inner),
    ])
}
fn batch_g(position: i64, first: &str, batch: &str, labels: &[&str]) -> Instance {
    let ms = labels
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let mut value = m(i as i64 + 1, label);
            append(&mut value, "BatchName", text(batch));
            value
        })
        .collect();
    group(vec![
        ("First", text(first)),
        ("GroupPosition", integer(position)),
        ("BatchName", text(batch)),
        ("Members", repeated(ms)),
    ])
}
fn batches_input(per_parent: bool) -> Instance {
    let x = if per_parent {
        vec![
            r("A", "x1", true, 0),
            r("B", "x2", true, 0),
            r("A", "x3", true, 0),
        ]
    } else {
        vec![r("A", "x1", true, 0), r("B", "x2", true, 0)]
    };
    let y = if per_parent {
        vec![
            r("B", "y1", true, 0),
            r("A", "y2", true, 0),
            r("B", "y3", true, 0),
        ]
    } else {
        vec![r("B", "y1", true, 0), r("A", "y2", true, 0)]
    };
    group(vec![
        ("Marker", text("P.root")),
        ("Limit", integer(0)),
        (
            "Batches",
            repeated(vec![
                group(vec![
                    ("BatchName", text("X")),
                    ("Limit", integer(1)),
                    ("Rows", repeated(x)),
                ]),
                group(vec![
                    ("BatchName", text("Y")),
                    ("Limit", integer(2)),
                    ("Rows", repeated(y)),
                ]),
            ]),
        ),
    ])
}
pub fn cases() -> Vec<Case> {
    let mut cases = vec![
        success("A01", o(p5())),
        success("A02", o(n5())),
        success("A03", o(vec![g(1, "U-only", "U.root", &["U-only"])])),
        success(
            "A04",
            o(vec![g(
                1,
                "P-a1",
                "P.root",
                &["P-a1", "P-b1", "P-a2", "P-a3", "P-c1", "P-a4"],
            )]),
        ),
        success("A05", o(post())),
        empty("A06"),
        success(
            "A07",
            o(vec![g(
                1,
                "P-a1",
                "P.root",
                &["P-a1", "P-a2", "P-a3", "P-a4"],
            )]),
        ),
        success(
            "A08",
            o(vec![
                g(1, "P-b1", "P.root", &["P-b1"]),
                g(2, "P-a2", "P.root", &["P-a2", "P-a3", "P-a1"]),
                g(3, "P-c1", "P.root", &["P-c1"]),
                g(4, "P-a4", "P.root", &["P-a4"]),
            ]),
        ),
        success(
            "A09",
            o(vec![
                g(1, "P-a2", "P.root", &["P-a2", "P-a3"]),
                g(2, "P-c1", "P.root", &["P-c1"]),
                g(3, "P-a4", "P.root", &["P-a4"]),
            ]),
        ),
        success(
            "A10",
            o(vec![
                g(1, "P-a3", "P.root", &["P-a3", "P-a1"]),
                g(2, "P-c1", "P.root", &["P-c1"]),
                g(3, "P-a4", "P.root", &["P-a4"]),
            ]),
        ),
        success("A11", o(vec![g(1, "P-a3", "P.root", &["P-a3", "P-a1"])])),
        success(
            "A12",
            o(vec![
                g(1, "P-b1", "P.root", &["P-b1", "P-a2", "P-a3"]),
                g(2, "P-a1", "P.root", &["P-a1", "P-c1", "P-a4"]),
            ]),
        ),
        success(
            "A13",
            o(vec![
                g(1, "P-a4", "P.root", &["P-a4"]),
                g(2, "P-c1", "P.root", &["P-c1"]),
                g(3, "P-a3", "P.root", &["P-a3", "P-a2"]),
                g(4, "P-b1", "P.root", &["P-b1"]),
                g(5, "P-a1", "P.root", &["P-a1"]),
            ]),
        ),
        success("A14", sorted_first_two()),
        success(
            "A15",
            o(vec![
                g(1, "P-c1", "P.root", &["P-c1"]),
                g(2, "P-a4", "P.root", &["P-a4"]),
            ]),
        ),
        success("A16", o(vec![g(1, "P-a4", "P.root", &["P-a4"])])),
        success(
            "A17",
            o(vec![
                g(1, "P-a2", "P.root", &["P-a2", "P-a3"]),
                g(2, "P-c1", "P.root", &["P-c1"]),
            ]),
        ),
        success(
            "A18",
            group(vec![("Groups", g(1, "P-a1", "P.root", &["P-a1"]))]),
        ),
        success("A19", group(vec![("Groups", group(vec![]))])),
        empty("A20"),
        success("A22-key", o(p5())),
        success("A22-post", o(p5())),
        success(
            "N01",
            repeated(vec![outer("P-a1", 1, o(n5())), outer("P-b1", 2, o(n5()))]),
        ),
        success(
            "N02",
            repeated(vec![outer("N-a1", 1, o(p5())), outer("N-b1", 2, o(p5()))]),
        ),
        success(
            "N03",
            repeated(vec![outer(
                "P-a2",
                1,
                o(vec![g(1, "P-a2", "P.root", &["P-a2", "P-a3"])]),
            )]),
        ),
    ];
    replace(
        &mut cases
            .iter_mut()
            .find(|c| c.profile == "A19")
            .unwrap()
            .source,
        "Rows",
        repeated(vec![]),
    );
    let mut pruned = normal(
        "A21-null-pruning",
        "A21",
        expected(o(vec![
            g(1, "a1", "P.root", &["a1", "a2"]),
            g(2, "b1", "P.root", &["b1"]),
        ])),
    );
    replace(
        &mut pruned.source,
        "Rows",
        repeated(vec![
            r("A", "a1", true, 0),
            scalar(Value::Null),
            r("A", "a2", true, 0),
            scalar(Value::json_null()),
            r("B", "b1", true, 0),
        ]),
    );
    cases.push(pruned);
    cases.push(success(
        "A23",
        group(vec![
            ("Groups", repeated(p5())),
            (
                "By",
                repeated(vec![
                    g(1, "P-a1", "P.root", &["P-a1", "P-a2", "P-a3", "P-a4"]),
                    g(2, "P-b1", "P.root", &["P-b1"]),
                    g(3, "P-c1", "P.root", &["P-c1"]),
                ]),
            ),
            ("Adjacent", repeated(p5())),
            (
                "Starting",
                repeated(vec![
                    g(1, "P-a1", "P.root", &["P-a1", "P-b1"]),
                    g(2, "P-a2", "P.root", &["P-a2"]),
                    g(3, "P-a3", "P.root", &["P-a3", "P-c1"]),
                    g(4, "P-a4", "P.root", &["P-a4"]),
                ]),
            ),
            (
                "Ending",
                repeated(vec![
                    g(1, "P-a1", "P.root", &["P-a1"]),
                    g(2, "P-b1", "P.root", &["P-b1", "P-a2"]),
                    g(3, "P-a3", "P.root", &["P-a3"]),
                    g(4, "P-c1", "P.root", &["P-c1", "P-a4"]),
                ]),
            ),
            (
                "Blocks",
                repeated(vec![
                    g(1, "P-a1", "P.root", &["P-a1", "P-b1"]),
                    g(2, "P-a2", "P.root", &["P-a2", "P-a3"]),
                    g(3, "P-c1", "P.root", &["P-c1", "P-a4"]),
                ]),
            ),
        ]),
    ));
    cases.push(success(
        "N04",
        repeated(vec![group(vec![
            ("OuterFirst", text("P-a2")),
            ("OuterPosition", integer(1)),
            ("Ordinary", members(&["P-a2", "P-a3"])),
            ("Inner", o(vec![g(1, "P-a2", "P.root", &["P-a2", "P-a3"])])),
        ])]),
    ));
    let mut flat = success(
        "N05",
        o(vec![
            batch_g(1, "x1", "X", &["x1"]),
            batch_g(2, "x2", "X", &["x2", "y1"]),
            batch_g(3, "y2", "Y", &["y2"]),
        ]),
    );
    flat.source = batches_input(false);
    cases.push(flat);
    let mut per = success(
        "N06",
        group(vec![(
            "Batches",
            repeated(vec![
                group(vec![
                    ("BatchName", text("X")),
                    ("Groups", repeated(vec![batch_g(1, "x1", "X", &["x1"])])),
                ]),
                group(vec![
                    ("BatchName", text("Y")),
                    (
                        "Groups",
                        repeated(vec![
                            batch_g(1, "y1", "Y", &["y1"]),
                            batch_g(2, "y2", "Y", &["y2"]),
                        ]),
                    ),
                ]),
            ]),
        )]),
    );
    per.source = batches_input(true);
    cases.push(per);
    for profile in [
        "N07-P",
        "N07-N",
        "N08-P",
        "N08-N",
        "N08-Alias",
        "N09-P-true",
        "N09-P-false",
        "N09-N-true",
        "N09-N-false",
        "N09-Alias-true",
        "N09-Alias-false",
        "N10",
        "N11",
    ] {
        let named = profile.contains("-N") || profile.contains("Alias");
        let input = if profile.starts_with("N07") {
            group(vec![("Key", text("A")), ("Label", text("single"))])
        } else {
            root_input()
        };
        let outcome = if profile.starts_with("N09") {
            owned(
                Some(10),
                "UniteratedRepetition",
                if profile.contains("Alias") {
                    "Alias"
                } else if named {
                    "Rows"
                } else {
                    "Primary"
                },
                &["Key"],
                &["Key"],
                Some(0),
                None,
            )
        } else if profile == "N11" {
            owned(
                Some(12),
                "UniteratedRepetition",
                "Primary",
                &["Key"],
                &["Key"],
                Some(0),
                None,
            )
        } else {
            expected(if profile.starts_with("N07") {
                repeated(vec![root_g(1, "single", &["single"])])
            } else if profile == "N10" {
                repeated(vec![
                    group(vec![
                        ("First", text("a1")),
                        ("GroupPosition", integer(1)),
                        ("Members", repeated(vec![m(1, "a1")])),
                    ]),
                    group(vec![
                        ("First", text("b1")),
                        ("GroupPosition", integer(2)),
                        ("Members", repeated(vec![m(2, "b1")])),
                    ]),
                    group(vec![
                        ("First", text("a2")),
                        ("GroupPosition", integer(3)),
                        ("Members", repeated(vec![m(3, "a2")])),
                    ]),
                    group(vec![
                        ("First", text("a3")),
                        ("GroupPosition", integer(4)),
                        ("Members", repeated(vec![m(4, "a3")])),
                    ]),
                ])
            } else {
                root_output()
            })
        };
        let mut case = normal(profile, profile, outcome);
        if named {
            case.source = group(vec![]);
            case.inputs[0].1 = input;
            if profile.contains("Alias") {
                case.inputs[0].0 = "Alias".into();
            }
        } else {
            case.source = input;
        }
        if profile == "N11" {
            case.inputs[0].1 = root_input();
            case.calls[2]["expected"] = ok(vec![(
                Some(projects::AUDIT),
                group(vec![("Marker", text("audit"))]),
            )]);
        }
        cases.push(case);
    }
    cases.push(normal(
        "N12-inactive",
        "N12-inactive",
        owned(
            Some(20),
            "InactiveSourceFrame",
            "Primary",
            &[],
            &[],
            None,
            Some(&[]),
        ),
    ));
    cases.push(success(
        "N12-bypass",
        o(vec![
            g(1, "P.root", "P.root", &["P-a1"]),
            g(2, "P.root", "P.root", &["P-b1"]),
            g(3, "P.root", "P.root", &["P-a2", "P-a3"]),
            g(4, "P.root", "P.root", &["P-c1"]),
            g(5, "P.root", "P.root", &["P-a4"]),
        ]),
    ));
    cases.push(normal(
        "N12-strict",
        "N12-strict",
        owned(
            Some(20),
            "UniteratedRepetition",
            "Primary",
            &["Rows", "Label"],
            &["Rows", "Label"],
            Some(1),
            None,
        ),
    ));
    let mut scalar_root = success("N13", repeated(vec![integer(1), text("1"), integer(1)]));
    scalar_root.source = repeated(vec![integer(1), text("1"), integer(1)]);
    cases.push(scalar_root);
    let mut details = normal(
        "N14",
        "N14",
        owned(
            Some(20),
            "UniteratedRepetition",
            "Primary",
            &["Rows", "Details", "Value"],
            &["Details", "Value"],
            Some(2),
            None,
        ),
    );
    let Instance::Group(fields) = &mut details.source else {
        unreachable!()
    };
    let Instance::Repeated(rows) = &mut fields[2].1 else {
        unreachable!()
    };
    append(
        &mut rows[0],
        "Details",
        repeated(vec![
            group(vec![("Value", text("first"))]),
            group(vec![("Value", text("second"))]),
        ]),
    );
    cases.push(details);
    tagged(&mut cases);
    errors(&mut cases);
    composition(&mut cases);
    cases
}
fn tagged(cases: &mut Vec<Case>) {
    let keys = vec![
        ("i1", Value::Int(1)),
        ("f1", Value::Float(f64::from_bits(0x3ff0000000000000))),
        ("i2", Value::Int(1)),
        ("zp", Value::Float(f64::from_bits(0))),
        ("zn", Value::Float(f64::from_bits(0x8000000000000000))),
        ("nan1", Value::Float(f64::from_bits(0x7ff8000000000042))),
        ("nan2", Value::Float(f64::from_bits(0x7ff8000000000042))),
        ("n1", Value::Null),
        ("jn", Value::json_null()),
        ("n2", Value::Null),
        ("sA1", Value::String("A".into())),
        ("sa", Value::String("a".into())),
        ("sA2", Value::String("A".into())),
    ];
    fn tg(position: i64, label: &str, key: Value, items: Vec<(&str, Value)>) -> Instance {
        let ms = items
            .into_iter()
            .enumerate()
            .map(|(i, (label, key))| {
                group(vec![
                    ("Key", scalar(key)),
                    ("Label", text(label)),
                    ("MemberPosition", integer(i as i64 + 1)),
                    ("LegacyPosition", integer(i as i64 + 1)),
                ])
            })
            .collect();
        group(vec![
            ("Key", scalar(key)),
            ("First", text(label)),
            ("GroupPosition", integer(position)),
            ("RootMarker", text("P.root")),
            ("Members", repeated(ms)),
        ])
    }
    let output = o(vec![
        tg(1, "i1", Value::Int(1), vec![("i1", Value::Int(1))]),
        tg(2, "f1", Value::Float(1.0), vec![("f1", Value::Float(1.0))]),
        tg(3, "i2", Value::Int(1), vec![("i2", Value::Int(1))]),
        tg(
            4,
            "zp",
            Value::Float(0.0),
            vec![
                ("zp", Value::Float(0.0)),
                ("zn", Value::Float(f64::from_bits(0x8000000000000000))),
            ],
        ),
        tg(
            5,
            "nan1",
            Value::Float(f64::from_bits(0x7ff8000000000042)),
            vec![("nan1", Value::Float(f64::from_bits(0x7ff8000000000042)))],
        ),
        tg(
            6,
            "nan2",
            Value::Float(f64::from_bits(0x7ff8000000000042)),
            vec![("nan2", Value::Float(f64::from_bits(0x7ff8000000000042)))],
        ),
        tg(7, "n1", Value::Null, vec![("n1", Value::Null)]),
        tg(
            8,
            "jn",
            Value::json_null(),
            vec![("jn", Value::json_null())],
        ),
        tg(9, "n2", Value::Null, vec![("n2", Value::Null)]),
        tg(
            10,
            "sA1",
            Value::String("A".into()),
            vec![("sA1", Value::String("A".into()))],
        ),
        tg(
            11,
            "sa",
            Value::String("a".into()),
            vec![("sa", Value::String("a".into()))],
        ),
        tg(
            12,
            "sA2",
            Value::String("A".into()),
            vec![("sA2", Value::String("A".into()))],
        ),
    ]);
    let mut tagged = success("T01", output);
    replace(
        &mut tagged.source,
        "Rows",
        repeated(
            keys.into_iter()
                .map(|(label, key)| row(key, label, true, 0))
                .collect(),
        ),
    );
    cases.push(tagged);
    let mut unicode = normal(
        "T02-canonical-equivalence-is-not-equality",
        "T01",
        expected(o(vec![
            tg(
                1,
                "1",
                Value::String("é".into()),
                vec![
                    ("1", Value::String("é".into())),
                    ("2", Value::String("é".into())),
                ],
            ),
            tg(
                2,
                "3",
                Value::String("e\u{301}".into()),
                vec![("3", Value::String("e\u{301}".into()))],
            ),
            tg(
                3,
                "4",
                Value::String("é".into()),
                vec![("4", Value::String("é".into()))],
            ),
        ])),
    );
    replace(
        &mut unicode.source,
        "Rows",
        repeated(vec![
            r("é", "1", true, 0),
            r("é", "2", true, 0),
            r("e\u{301}", "3", true, 0),
            r("é", "4", true, 0),
        ]),
    );
    cases.push(unicode);
}
fn errors(cases: &mut Vec<Case>) {
    for (profile, node, message) in [
        ("E01", 101, "key"),
        ("E02", 105, "post"),
        ("E03", 102, "filter"),
        ("E04", 103, "sort"),
        ("E05", 104, "bound"),
        ("E06", 101, "key"),
        ("E06-first", 101, "key"),
        ("E07", 105, "post"),
        ("E08", 105, "post"),
        ("E11", 104, "bound"),
    ] {
        cases.push(normal(profile, profile, raised(node, message)));
    }
    cases.push(normal(
        "E09",
        "E09",
        json!({"Err":{"kind":"NotABool","node":105,"found":"string"}}),
    ));
    let mut bound = normal(
        "E10",
        "E10",
        json!({"Err":{"kind":"NotAnItemCount","node":104,"found":"bool"}}),
    );
    replace(&mut bound.source, "Rows", repeated(vec![]));
    cases.push(bound);
    cases.extend([empty("E12"), empty("E13"), success("E14", o(post()))]);
    let mut absent = normal(
        "E15",
        "E15",
        owned(None, "SourceUnavailable", "Rows", &[], &[], None, None),
    );
    absent.inputs.remove(0);
    absent.calls[0]["expected"]["Err"]["native_scope"] = json!({"target":"Primary","children":[0]});
    absent.calls[1]["expected"]["Err"]["native_scope"] = json!({"target":"Primary","children":[0]});
    cases.push(absent);
    for profile in ["E16-window", "E16-post", "E16-reached"] {
        let mut case = if profile == "E16-reached" {
            normal(
                profile,
                profile,
                owned(Some(202), "SourceUnavailable", "Rows", &[], &[], None, None),
            )
        } else {
            empty(profile)
        };
        case.inputs.remove(0);
        cases.push(case);
    }
    for (profile, node) in [("E17-position", 205), ("E17-pin", 202)] {
        cases.push(normal(
            profile,
            profile,
            owned(
                Some(node),
                "InactiveSourceFrame",
                "Rows",
                &[],
                &[],
                None,
                Some(&["Rows"]),
            ),
        ));
    }
    cases.push(normal(
        "E17-unpinned",
        "E17-unpinned",
        owned(
            Some(20),
            "UniteratedRepetition",
            "Rows",
            &["Rows", "Label"],
            &["Rows", "Label"],
            Some(1),
            None,
        ),
    ));
    for (profile, message) in [("E18-row", "row.reject"), ("E18-global", "global.reject")] {
        let mut case = normal(
            profile,
            profile,
            json!({"Err":{"kind":"MappingFailure","rule":1,"message":message}}),
        );
        case.calls[2]["expected"] = case.calls[0]["expected"].clone();
        cases.push(case);
    }
    let mut unknown = normal(
        "E20-target-before-absent-and-invalid-policy",
        "A01",
        json!(null),
    );
    unknown.inputs.clear();
    unknown.calls = vec![
        json!({"selection":"audit💠","invalid_limits":true,"expected":{"Err":{"kind":"UnknownTarget","name":"audit💠"}}}),
    ];
    cases.push(unknown);
    let mut optional = normal("C01-absent-unreached-optional", "A01", expected(o(p5())));
    optional.inputs.clear();
    optional.calls = vec![json!({"selection":"Primary","expected":expected(o(p5()))})];
    cases.push(optional);
    cases.push(success("C02", o(p5())));
    cases.push(success("C03", o(p5())));
}
fn composition(cases: &mut Vec<Case>) {
    let mut key = success(
        "I01",
        o(vec![
            g(1, "P-a1", "P.root", &["P-a1"]),
            g(2, "P-b1", "P.root", &["P-b1"]),
            g(3, "P-a2", "P.root", &["P-a2"]),
        ]),
    );
    let mut a = r("A", "P-a1", false, 30);
    append(&mut a, "Index", integer(1));
    let mut b = r("B", "P-b1", true, 10);
    append(&mut b, "Index", integer(2));
    let mut c = r("A", "P-a2", false, 20);
    append(&mut c, "Index", integer(3));
    replace(&mut key.source, "Rows", repeated(vec![a, b, c]));
    cases.push(key);
    fn selected(
        position: i64,
        first: &str,
        value: Instance,
        ms: Vec<(&str, i64, Instance)>,
    ) -> Instance {
        group(vec![
            ("First", text(first)),
            ("GroupPosition", integer(position)),
            ("RootMarker", text("P.root")),
            ("Selected", value),
            (
                "Members",
                repeated(
                    ms.into_iter()
                        .map(|(label, pos, value)| {
                            let mut m = m(pos, label);
                            append(&mut m, "Selected", value);
                            m
                        })
                        .collect(),
                ),
            ),
        ])
    }
    cases.push(success(
        "I02",
        o(vec![
            selected(1, "P-a1", integer(10), vec![("P-a1", 1, integer(10))]),
            selected(2, "P-b1", integer(11), vec![("P-b1", 1, integer(10))]),
            selected(
                3,
                "P-a2",
                integer(12),
                vec![("P-a2", 1, integer(10)), ("P-a3", 2, integer(11))],
            ),
            selected(
                4,
                "P-c1",
                scalar(Value::Null),
                vec![("P-c1", 1, integer(10))],
            ),
            selected(
                5,
                "P-a4",
                scalar(Value::Null),
                vec![("P-a4", 1, integer(10))],
            ),
        ]),
    ));
    let mut bound = success("I03", sorted_first_two());
    append(&mut bound.source, "Index", integer(1));
    append(&mut bound.inputs[0].1, "Index", integer(2));
    cases.push(bound);
    for profile in ["I04-empty", "I04-null", "I04-producer"] {
        cases.push(normal(
            profile,
            profile,
            raised(
                if profile == "I04-producer" { 953 } else { 105 },
                if profile == "I04-producer" {
                    "key"
                } else {
                    "post"
                },
            ),
        ));
    }
    cases.push(success(
        "I05-generate",
        o(vec![g(1, "P-a2", "P.root", &["P-a2", "P-a3"])]),
    ));
    for profile in ["I05-tokenize", "I05-length", "I05-regex", "I05-nested"] {
        cases.push(success(
            profile,
            o(vec![
                g(1, "P-a1", "P.root", &["P-a1"]),
                g(2, "P-a2", "P.root", &["P-a2", "P-a3"]),
            ]),
        ));
    }
}
