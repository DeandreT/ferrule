//! Hand-derived ordered trees. Helpers render literal lists; none groups inputs.
use super::*;

pub const CASE_COUNT: usize = 59;
pub const CALL_COUNT: usize = 169;
pub fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
pub fn integer(value: i64) -> Instance {
    Instance::Scalar(Value::Int(value))
}
pub fn boolean(value: bool) -> Instance {
    Instance::Scalar(Value::Bool(value))
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
fn replace(mut value: Instance, name: &str, field: Instance) -> Instance {
    let Instance::Group(fields) = &mut value else {
        panic!("authored Group")
    };
    fields.iter_mut().find(|(key, _)| key == name).unwrap().1 = field;
    value
}
fn row(key: Value, label: &str, keep: bool, rank: i64) -> Instance {
    group(vec![
        ("Key", Instance::Scalar(key)),
        ("Label", text(label)),
        ("Keep", boolean(keep)),
        ("Rank", integer(rank)),
        ("Limit", integer(0)),
        ("KeyFail", boolean(false)),
        ("PostFail", boolean(false)),
        ("FilterFail", boolean(false)),
        ("SortFail", boolean(false)),
        ("TargetFail", boolean(false)),
        ("Reject", boolean(false)),
        (
            "Details",
            repeated(vec![group(vec![("Code", text("detail"))])]),
        ),
    ])
}
fn rows(prefix: &str, named: bool) -> Vec<Instance> {
    [
        ("A", "a1", false, 30),
        ("B", "b1", !named, 10),
        ("A", "a2", true, 20),
        ("C", "c1", false, 40),
        ("B", "b2", named, 15),
    ]
    .into_iter()
    .map(|(key, label, keep, rank)| {
        row(
            Value::String(key.into()),
            &format!("{prefix}-{label}"),
            keep,
            if named { rank * 10 } else { rank },
        )
    })
    .collect()
}
fn endpoint(prefix: &str, named: bool) -> Instance {
    group(vec![
        ("Marker", text(&format!("{prefix}.root"))),
        ("Limit", integer(1)),
        ("GlobalReject", boolean(false)),
        ("Rows", repeated(rows(prefix, named))),
        ("Batch", repeated(vec![])),
    ])
}
pub fn source() -> Instance {
    endpoint("P", false)
}
pub fn inputs() -> Vec<(String, Instance)> {
    vec![
        ("Rows".into(), endpoint("N", true)),
        (projects::EXACT.into(), endpoint("U", true)),
        ("RequiredLegacy".into(), endpoint("L", false)),
    ]
}
fn change_row(source: Instance, index: usize, name: &str, value: Instance) -> Instance {
    let Instance::Group(fields) = &source else {
        panic!("source Group")
    };
    let Instance::Repeated(items) = &fields.iter().find(|(name, _)| name == "Rows").unwrap().1
    else {
        panic!("Rows")
    };
    let mut items = items.clone();
    items[index] = replace(items[index].clone(), name, value);
    replace(source, "Rows", repeated(items))
}
fn marker(prefix: &str) -> String {
    format!("{prefix}.root")
}
pub fn bucket(first: &str, ordinal: i64, root: &str, members: &[&str]) -> Instance {
    group(vec![
        ("First", text(first)),
        ("GroupPosition", integer(ordinal)),
        ("RootMarker", text(root)),
        (
            "Members",
            repeated(
                members
                    .iter()
                    .enumerate()
                    .map(|(index, label)| {
                        group(vec![
                            ("Label", text(label)),
                            ("MemberPosition", integer(index as i64 + 1)),
                            ("RootMarker", text(root)),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}
fn buckets(prefix: &str) -> Vec<Instance> {
    vec![
        bucket(
            &format!("{prefix}-a1"),
            1,
            &marker(prefix),
            &[&format!("{prefix}-a1"), &format!("{prefix}-a2")],
        ),
        bucket(
            &format!("{prefix}-b1"),
            2,
            &marker(prefix),
            &[&format!("{prefix}-b1"), &format!("{prefix}-b2")],
        ),
        bucket(
            &format!("{prefix}-c1"),
            3,
            &marker(prefix),
            &[&format!("{prefix}-c1")],
        ),
    ]
}
fn output(items: Vec<Instance>) -> Instance {
    group(vec![("Groups", repeated(items))])
}
pub fn standard(prefix: &str) -> Instance {
    output(buckets(prefix))
}
fn ok(outputs: Vec<(Option<String>, Instance)>) -> Json {
    json!({"Ok":outputs.iter().map(|(name,tree)|json!([name,codec::encode(tree)])).collect::<Vec<_>>()})
}
fn primary(tree: Instance) -> Json {
    ok(vec![(None, tree)])
}
fn audit(tree: Instance) -> Json {
    ok(vec![(Some(projects::AUDIT.into()), tree)])
}
fn raise(node: u32, message: &str) -> Json {
    json!({"Err":{"kind":"MappingException","node":node,"message":message}})
}
fn owned(
    node: Option<u32>,
    cause: &str,
    owner: &str,
    path: &[&str],
    native_path: &[&str],
    segment: Option<usize>,
    found: Option<&str>,
) -> Json {
    json!({"Err":{"kind":"OwnedSource","node":node,"cause":cause,"owner":owner,"path":path,"native_path":native_path,
        "segment":segment,"found":found,"collection":null,"native_scope":null}})
}
fn ordinary_case(
    id: &'static str,
    profile: &'static str,
    source: Instance,
    expected: Json,
) -> Case {
    let all = if let Some(rows) = expected.get("Ok") {
        let mut rows = rows.as_array().unwrap().clone();
        rows.push(json!([projects::AUDIT, codec::encode(&standard("N"))]));
        rows.push(json!([
            "legacy",
            codec::encode(&group(vec![("Marker", text("L.root"))]))
        ]));
        json!({"Ok":rows})
    } else {
        expected.clone()
    };
    Case {
        id,
        profile,
        source,
        inputs: inputs(),
        native: true,
        calls: vec![
            json!({"selection":"All","expected":all}),
            json!({"selection":"Primary","expected":expected}),
            json!({"selection":projects::AUDIT,"expected":audit(standard("N"))}),
        ],
    }
}
fn success(id: &'static str, profile: &'static str, source: Instance, expected: Instance) -> Case {
    ordinary_case(id, profile, source, primary(expected))
}
fn root_case(
    id: &'static str,
    profile: &'static str,
    source: Instance,
    expected: Instance,
) -> Case {
    Case {
        id,
        profile,
        source,
        inputs: vec![],
        native: true,
        calls: vec![
            json!({"selection":"All","expected":primary(expected.clone())}),
            json!({"selection":"Primary","expected":primary(expected)}),
        ],
    }
}
fn nested_bucket(
    mut outer: Instance,
    inner: Vec<Instance>,
    outer_label: &str,
    outer_position: i64,
    annotate: bool,
) -> Instance {
    let inner = inner
        .into_iter()
        .map(|mut inner| {
            if annotate {
                let Instance::Group(fields) = &mut inner else {
                    unreachable!()
                };
                let members = fields
                    .iter_mut()
                    .find(|(name, _)| name == "Members")
                    .unwrap();
                let Instance::Repeated(items) = &mut members.1 else {
                    unreachable!()
                };
                for member in items {
                    let Instance::Group(fields) = member else {
                        unreachable!()
                    };
                    fields.push(("OuterFirst".into(), text(outer_label)));
                    fields.push(("OuterPosition".into(), integer(outer_position)));
                }
                fields.insert(3, ("OuterFirst".into(), text(outer_label)));
                fields.insert(4, ("OuterPosition".into(), integer(outer_position)));
            }
            inner
        })
        .collect();
    let Instance::Group(fields) = &mut outer else {
        unreachable!()
    };
    fields.push(("Nested".into(), repeated(inner)));
    outer
}
fn nested_expected(outer: &str, inner: &str) -> Instance {
    output(
        buckets(outer)
            .into_iter()
            .zip([("a1", 1), ("b1", 2), ("c1", 3)])
            .map(|(tree, (label, position))| {
                nested_bucket(
                    tree,
                    buckets(inner),
                    &format!("{outer}-{label}"),
                    position,
                    true,
                )
            })
            .collect(),
    )
}
fn prefix_bucket(
    first: &str,
    ordinal: i64,
    prefix: &str,
    prefix_position: i64,
    members: &[&str],
) -> Instance {
    let mut tree = bucket(first, ordinal, "P.root", members);
    let Instance::Group(fields) = &mut tree else {
        unreachable!()
    };
    fields.insert(3, ("Prefix".into(), text(prefix)));
    fields.insert(4, ("PrefixPosition".into(), integer(prefix_position)));
    let Instance::Repeated(items) = &mut fields
        .iter_mut()
        .find(|(name, _)| name == "Members")
        .unwrap()
        .1
    else {
        unreachable!()
    };
    for member in items {
        let Instance::Group(fields) = member else {
            unreachable!()
        };
        fields.push(("Prefix".into(), text(prefix)));
        fields.push(("PrefixPosition".into(), integer(prefix_position)));
    }
    tree
}
fn batch_source() -> Instance {
    let one = group(vec![
        ("Id", text("B1")),
        ("Limit", integer(1)),
        (
            "Rows",
            repeated(vec![
                row(Value::String("A".into()), "B1-a1", true, 1),
                row(Value::String("B".into()), "B1-b1", true, 2),
            ]),
        ),
    ]);
    let two = group(vec![
        ("Id", text("B2")),
        ("Limit", integer(2)),
        (
            "Rows",
            repeated(vec![
                row(Value::String("A".into()), "B2-a1", true, 1),
                row(Value::String("B".into()), "B2-b1", true, 2),
                row(Value::String("C".into()), "B2-c1", true, 3),
            ]),
        ),
    ]);
    replace(
        replace(source(), "Limit", integer(0)),
        "Batch",
        repeated(vec![one, two]),
    )
}
pub(super) fn cases() -> Vec<Case> {
    let mut cases = vec![
        success(
            "G01-primary-collision",
            "basic-primary",
            source(),
            standard("P"),
        ),
        success(
            "G02-named-collision",
            "basic-named",
            source(),
            standard("N"),
        ),
        success(
            "G02-exact-space-unicode-owner",
            "basic-unicode",
            source(),
            standard("U"),
        ),
        success(
            "G01-legacy-empty-members",
            "legacy-members",
            source(),
            standard("P"),
        ),
        success(
            "G03-any-member-primary",
            "post-primary",
            source(),
            output(buckets("P")[..2].to_vec()),
        ),
        success(
            "G03-any-member-named",
            "post-named",
            source(),
            output(buckets("N")[..2].to_vec()),
        ),
    ];
    // 7: A is all false; B's first false and later true still retain both B members.
    let changed = change_row(
        change_row(source(), 2, "Keep", boolean(false)),
        4,
        "Keep",
        boolean(true),
    );
    let changed = change_row(changed, 1, "Keep", boolean(false));
    cases.push(success(
        "G03-only-later-B-true",
        "post-primary",
        changed,
        output(vec![bucket("P-b1", 1, "P.root", &["P-b1", "P-b2"])]),
    ));
    let mut all_false = source();
    for index in 0..5 {
        all_false = change_row(all_false, index, "Keep", boolean(false));
    }
    cases.push(success(
        "G03-all-false",
        "post-primary",
        all_false,
        output(vec![]),
    ));
    cases.push(success(
        "G03-empty",
        "post-primary",
        replace(source(), "Rows", repeated(vec![])),
        output(vec![]),
    ));
    cases.push(success(
        "G04-sorted-post-ranks",
        "sorted-post",
        source(),
        output(vec![
            bucket("P-a2", 1, "P.root", &["P-a2", "P-a1"]),
            bucket("P-c1", 2, "P.root", &["P-c1"]),
        ]),
    ));
    cases.push(success(
        "G04-parent-last-final-ordinal",
        "sorted-post-last",
        source(),
        output(vec![bucket("P-c1", 1, "P.root", &["P-c1"])]),
    ));
    cases.push(success(
        "G05-filter-then-sort",
        "filter-before-sort",
        source(),
        output(vec![
            bucket("P-b1", 1, "P.root", &["P-b1", "P-b2"]),
            bucket("P-a2", 2, "P.root", &["P-a2"]),
        ]),
    ));
    cases.push(success(
        "G05-sort-then-filter",
        "sort-before-filter",
        source(),
        output(vec![bucket("P-a2", 1, "P.root", &["P-a2", "P-a1"])]),
    ));
    cases.push(success(
        "G05-key-observes-sorted-rank",
        "sorted-position-key",
        source(),
        output(vec![
            bucket("P-b1", 1, "P.root", &["P-b1", "P-b2"]),
            bucket("P-a2", 2, "P.root", &["P-a2", "P-a1", "P-c1"]),
        ]),
    ));
    cases.push(success(
        "G06-primary-then-named",
        "nested-P-N",
        source(),
        nested_expected("P", "N"),
    ));
    cases.push(success(
        "G06-named-then-primary",
        "nested-N-P",
        source(),
        nested_expected("N", "P"),
    ));
    let regroup = output(
        buckets("P")
            .into_iter()
            .zip([
                bucket("P-a1", 1, "P.root", &["P-a1", "P-a2"]),
                bucket("P-b1", 1, "P.root", &["P-b1", "P-b2"]),
                bucket("P-c1", 1, "P.root", &["P-c1"]),
            ])
            .map(|(outer, inner)| nested_bucket(outer, vec![inner], "unused", 0, false))
            .collect(),
    );
    cases.push(success(
        "G06-same-owner-regroup-members",
        "regroup-P",
        source(),
        regroup,
    ));
    let parent_rows = output(
        [
            ("P-a1", 1),
            ("P-b1", 2),
            ("P-a2", 3),
            ("P-c1", 4),
            ("P-b2", 5),
        ]
        .into_iter()
        .map(|(label, position)| {
            let mut outer = bucket(label, position, "P.root", &[label]);
            let Instance::Group(fields) = &mut outer else {
                unreachable!()
            };
            let Instance::Repeated(members) = &mut fields
                .iter_mut()
                .find(|(name, _)| name == "Members")
                .unwrap()
                .1
            else {
                unreachable!()
            };
            members[0] = replace(members[0].clone(), "MemberPosition", integer(position));
            nested_bucket(outer, buckets("N"), label, position, true)
        })
        .collect(),
    );
    cases.push(success(
        "G06-ordinary-parent-row-named-groups",
        "parent-row-N",
        source(),
        parent_rows,
    ));
    let per_batch = group(vec![(
        "Batches",
        repeated(vec![
            group(vec![
                ("Parent", text("B1")),
                ("ParentPosition", integer(1)),
                (
                    "Groups",
                    repeated(vec![prefix_bucket("B1-a1", 1, "B1", 1, &["B1-a1"])]),
                ),
            ]),
            group(vec![
                ("Parent", text("B2")),
                ("ParentPosition", integer(2)),
                (
                    "Groups",
                    repeated(vec![
                        prefix_bucket("B2-a1", 1, "B2", 2, &["B2-a1"]),
                        prefix_bucket("B2-b1", 2, "B2", 2, &["B2-b1"]),
                    ]),
                ),
            ]),
        ]),
    )]);
    cases.push(success(
        "G07-parent-Batch-bounds",
        "batch-per-parent",
        batch_source(),
        per_batch,
    ));
    cases.push(success(
        "G07-flat-first-prefix-not-member-original-parent",
        "batch-flat",
        batch_source(),
        output(vec![
            prefix_bucket("B1-a1", 1, "B1", 1, &["B1-a1", "B2-a1"]),
            prefix_bucket("B1-b1", 2, "B1", 1, &["B1-b1", "B2-b1"]),
            prefix_bucket("B2-c1", 3, "B2", 2, &["B2-c1"]),
        ]),
    ));
    // D1: only the approved Group root singleton, not an unapproved scalar singleton.
    let singleton = group(vec![
        ("Key", text("S")),
        ("Label", text("singleton")),
        ("Marker", text("pinned.root")),
    ]);
    cases.push(root_case(
        "G08-nonrepeating-root-singleton",
        "root-singleton",
        singleton,
        repeated(vec![group(vec![
            ("First", text("singleton")),
            ("GroupPosition", integer(1)),
            ("RootMarker", text("pinned.root")),
            (
                "Members",
                repeated(vec![group(vec![
                    ("Label", text("singleton")),
                    ("MemberPosition", integer(1)),
                ])]),
            ),
        ])]),
    ));
    let root_input = repeated(vec![
        group(vec![("Key", text("A")), ("Label", text("r1"))]),
        group(vec![("Key", text("B")), ("Label", text("r2"))]),
        group(vec![("Key", text("A")), ("Label", text("r3"))]),
    ]);
    let root_group = |first: &str, position: i64, labels: &[&str]| {
        group(vec![
            ("First", text(first)),
            ("GroupPosition", integer(position)),
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
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
    };
    cases.push(root_case(
        "G08-repeated-root-groups",
        "root-repeated-group",
        root_input,
        repeated(vec![
            root_group("r1", 1, &["r1", "r3"]),
            root_group("r2", 2, &["r2"]),
        ]),
    ));
    let nan = Instance::Scalar(Value::Float(f64::from_bits(0x7ff8_0000_0000_0041)));
    let float = |value: f64| Instance::Scalar(Value::Float(value));
    let scalar_input = repeated(vec![
        integer(1),
        text("1"),
        integer(1),
        float(1.0),
        float(0.0),
        float(-0.0),
        nan.clone(),
        nan.clone(),
    ]);
    cases.push(root_case(
        "G08-repeated-scalar-root-tags-zeros-NaNs",
        "root-repeated-scalar",
        scalar_input,
        repeated(vec![
            integer(1),
            text("1"),
            float(1.0),
            float(0.0),
            nan.clone(),
            nan.clone(),
        ]),
    ));
    let scalar_members = |position: i64, members: Vec<Instance>| {
        group(vec![
            ("GroupPosition", integer(position)),
            ("Members", repeated(members)),
        ])
    };
    cases.push(root_case(
        "G08-scalar-field-members-and-construction",
        "scalar-values",
        group(vec![(
            "Values",
            repeated(vec![
                integer(1),
                text("1"),
                integer(1),
                float(1.0),
                float(0.0),
                float(-0.0),
            ]),
        )]),
        group(vec![
            (
                "Representatives",
                repeated(vec![integer(1), text("1"), float(1.0), float(0.0)]),
            ),
            (
                "Groups",
                repeated(vec![
                    scalar_members(1, vec![integer(1), integer(1)]),
                    scalar_members(2, vec![text("1")]),
                    scalar_members(3, vec![float(1.0)]),
                    scalar_members(4, vec![float(0.0), float(-0.0)]),
                ]),
            ),
        ]),
    ));
    let typed_keys = [
        (Value::Int(1), "i1"),
        (Value::String("1".into()), "s1"),
        (Value::Int(1), "i2"),
        (Value::Float(1.0), "f1"),
        (Value::Null, "null"),
        (Value::json_null(), "json-null"),
        (Value::Float(0.0), "zero+"),
        (Value::Float(-0.0), "zero-"),
        (Value::Float(f64::from_bits(0x7ff8_0000_0000_0041)), "nan1"),
        (Value::Float(f64::from_bits(0x7ff8_0000_0000_0041)), "nan2"),
        (Value::Bool(true), "bool"),
        (Value::String("true".into()), "string"),
    ];
    let tagged_source = replace(
        source(),
        "Rows",
        repeated(
            typed_keys
                .into_iter()
                .map(|(key, label)| row(key, label, true, 1))
                .collect(),
        ),
    );
    cases.push(success(
        "G08-tagged-key-equality",
        "tagged-keys",
        tagged_source,
        output(vec![
            bucket("i1", 1, "P.root", &["i1", "i2"]),
            bucket("s1", 2, "P.root", &["s1"]),
            bucket("f1", 3, "P.root", &["f1"]),
            bucket("null", 4, "P.root", &["null"]),
            bucket("json-null", 5, "P.root", &["json-null"]),
            bucket("zero+", 6, "P.root", &["zero+", "zero-"]),
            bucket("nan1", 7, "P.root", &["nan1"]),
            bucket("nan2", 8, "P.root", &["nan2"]),
            bucket("bool", 9, "P.root", &["bool"]),
            bucket("string", 10, "P.root", &["string"]),
        ]),
    ));
    let gaps = replace(
        source(),
        "Rows",
        repeated(vec![
            Instance::Scalar(Value::Null),
            row(Value::String("A".into()), "gap-A1", false, 1),
            Instance::Scalar(Value::json_null()),
            row(Value::String("B".into()), "gap-B", true, 2),
            row(Value::String("A".into()), "gap-A2", true, 3),
        ]),
    );
    cases.push(success(
        "G09-owned-null-pruning-raw-gaps",
        "raw-gap-filter",
        gaps.clone(),
        output(vec![
            bucket("gap-B", 1, "P.root", &["gap-B"]),
            bucket("gap-A2", 2, "P.root", &["gap-A2"]),
        ]),
    ));
    let null_bucket = group(vec![
        ("First", Instance::Scalar(Value::Null)),
        ("GroupPosition", integer(1)),
        ("RootMarker", text("P.root")),
        (
            "Members",
            repeated(vec![
                group(vec![
                    ("Label", Instance::Scalar(Value::Null)),
                    ("MemberPosition", integer(1)),
                    ("RootMarker", text("P.root")),
                ]),
                group(vec![
                    ("Label", Instance::Scalar(Value::Null)),
                    ("MemberPosition", integer(2)),
                    ("RootMarker", text("P.root")),
                ]),
            ]),
        ),
    ]);
    cases.push(success(
        "G09-legacy-source-keeps-null-row-domain",
        "legacy-key-only",
        gaps,
        output(vec![
            null_bucket,
            bucket("gap-A1", 2, "P.root", &["gap-A1", "gap-A2"]),
            bucket("gap-B", 3, "P.root", &["gap-B"]),
        ]),
    ));
    cases.push(success(
        "G09-owned-key-only-demand",
        "legacy-key-only",
        source(),
        standard("P"),
    ));
    cases.push(success(
        "G09-owned-post-only-demand",
        "legacy-post-only",
        source(),
        output(buckets("P")[..2].to_vec()),
    ));
    cases.push(success(
        "G09-unrelated-ordinary-By-island",
        "legacy-island",
        source(),
        group(vec![
            ("Groups", repeated(buckets("P"))),
            ("LegacyGroups", repeated(buckets("P"))),
        ]),
    ));
    let poison = |key: &str, first: bool, later: bool| {
        change_row(
            change_row(source(), 0, key, boolean(first)),
            1,
            "FilterFail",
            boolean(later),
        )
    };
    let key_post = change_row(poison("KeyFail", true, false), 0, "PostFail", boolean(true));
    cases.push(ordinary_case(
        "E01-key-before-post",
        "phase",
        key_post,
        raise(501, "key.poison"),
    ));
    cases.push(ordinary_case(
        "E02-post-before-later-filter",
        "phase",
        poison("PostFail", true, true),
        raise(502, "post.poison"),
    ));
    cases.push(ordinary_case(
        "E03-prefilter-before-any-key",
        "phase-prefilter",
        poison("KeyFail", true, true),
        raise(503, "filter.poison"),
    ));
    cases.push(ordinary_case(
        "E04-sort-before-bound",
        "phase-sorted",
        change_row(source(), 0, "SortFail", boolean(true)),
        raise(504, "sort.poison"),
    ));
    cases.push(ordinary_case(
        "E05-bound-before-key",
        "phase-bound-raise",
        change_row(source(), 0, "KeyFail", boolean(true)),
        raise(505, "bound.poison"),
    ));
    cases.push(ordinary_case(
        "E06-empty-still-validates-bound",
        "phase-bad-bound",
        replace(source(), "Rows", repeated(vec![])),
        json!({"Err":{"kind":"NotAnItemCount","node":907,"found":"bool"}}),
    ));
    cases.push(ordinary_case(
        "E07-first-zero-still-evaluates-key",
        "phase-first-zero",
        change_row(source(), 0, "KeyFail", boolean(true)),
        raise(501, "key.poison"),
    ));
    cases.push(ordinary_case(
        "E08-post-must-be-bool",
        "phase-post-string",
        source(),
        json!({"Err":{"kind":"NotABool","node":906,"found":"string"}}),
    ));
    let later_post = change_row(
        change_row(source(), 0, "Keep", boolean(true)),
        2,
        "PostFail",
        boolean(true),
    );
    cases.push(ordinary_case(
        "E09-earlier-true-does-not-skip-later-post",
        "phase",
        later_post,
        raise(502, "post.poison"),
    ));
    cases.push(success(
        "E10-false-filter-skips-key-and-post",
        "filter-false-skips-key-post",
        source(),
        output(vec![]),
    ));
    let mut dropped = source();
    for i in 0..5 {
        dropped = change_row(
            change_row(dropped, i, "Keep", boolean(false)),
            i,
            "TargetFail",
            boolean(true),
        );
    }
    cases.push(success(
        "E11-dropped-target-remains-lazy",
        "phase-target",
        dropped,
        output(vec![]),
    ));
    let mut excluded = source();
    for i in 0..5 {
        excluded = change_row(excluded, i, "TargetFail", boolean(true));
    }
    cases.push(success(
        "E12-window-excluded-target-remains-lazy",
        "phase-first-zero",
        excluded,
        output(vec![]),
    ));
    let mut position = owned(
        Some(205),
        "InactiveSourceFrame",
        "Rows",
        &[],
        &[],
        None,
        None,
    );
    position["Err"]["collection"] = json!(["Rows"]);
    cases.push(ordinary_case(
        "E13-other-owner-position-is-inactive",
        "inactive-other-position",
        source(),
        position,
    ));
    let mut pin = owned(
        Some(202),
        "InactiveSourceFrame",
        "Rows",
        &[],
        &[],
        None,
        None,
    );
    pin["Err"]["collection"] = json!(["Rows"]);
    cases.push(ordinary_case(
        "E14-other-owner-pin-is-inactive",
        "inactive-other-pin",
        source(),
        pin,
    ));
    cases.push(ordinary_case(
        "E15-only-trusted-group-container-unwraps",
        "desc-repetition",
        source(),
        owned(
            Some(180),
            "UniteratedRepetition",
            "Primary",
            &["Rows", "Details", "Code"],
            &["Details", "Code"],
            Some(2),
            None,
        ),
    ));
    cases.push(ordinary_case(
        "E16-malformed-descendant-keeps-cause",
        "desc-traversal",
        change_row(source(), 0, "Details", text("bad")),
        owned(
            Some(180),
            "CannotTraverse",
            "Primary",
            &["Rows", "Details", "Code"],
            &["Details", "Code"],
            Some(2),
            Some("Scalar"),
        ),
    ));
    cases.push(ordinary_case(
        "E17-root-pin-does-not-unwrap-repeated-root-field",
        "primary-root-pin",
        source(),
        owned(
            Some(181),
            "UniteratedRepetition",
            "Primary",
            &["Rows", "Label"],
            &["Rows", "Label"],
            Some(1),
            None,
        ),
    ));
    let global_error = json!({"Err":{"kind":"MappingFailure","rule":1,"message":"global.reject"}});
    let mut global = ordinary_case(
        "F01-global-before-row-and-groups",
        "failures",
        replace(source(), "GlobalReject", boolean(true)),
        global_error.clone(),
    );
    for call in &mut global.calls {
        call["expected"] = global_error.clone();
    }
    cases.push(global);
    let row_error = json!({"Err":{"kind":"MappingFailure","rule":2,"message":"P-b1"}});
    let mut failed = ordinary_case(
        "F02-row-before-group-key",
        "failures",
        change_row(
            change_row(source(), 0, "KeyFail", boolean(true)),
            1,
            "Reject",
            boolean(true),
        ),
        row_error.clone(),
    );
    for call in &mut failed.calls {
        call["expected"] = row_error.clone();
    }
    cases.push(failed);
    cases.push(success(
        "F03-global-row-and-grouping-success",
        "failures",
        source(),
        standard("P"),
    ));
    let mut optional = success(
        "S01-unused-owned-input-stays-optional",
        "basic-primary",
        source(),
        standard("P"),
    );
    optional.inputs.clear();
    optional.calls.retain(|call| call["selection"] == "Primary");
    cases.push(optional);
    let mut unknown = success(
        "S02-exact-selected-name-resolution-first",
        "basic-primary",
        source(),
        standard("P"),
    );
    unknown.inputs.clear();
    unknown.calls = vec![
        json!({"selection":"audit💠","expected":{"Err":{"kind":"UnknownTarget","name":"audit💠"}}}),
    ];
    cases.push(unknown);
    cases.push(success(
        "W01-implicit-first-group",
        "implicit-first",
        source(),
        group(vec![("Groups", buckets("P")[0].clone())]),
    ));
    cases.push(success(
        "W02-implicit-first-empty-group",
        "implicit-first",
        replace(source(), "Rows", repeated(vec![])),
        group(vec![("Groups", group(vec![]))]),
    ));
    cases.push(success(
        "W03-windows-apply-to-retained-groups-in-order",
        "ordered-windows",
        source(),
        output(vec![bucket("P-c1", 1, "P.root", &["P-c1"])]),
    ));
    let mut unavailable = ordinary_case(
        "S03-missing-exact-owned-root-is-lazy-unavailable",
        "basic-named",
        source(),
        owned(None, "SourceUnavailable", "Rows", &[], &[], None, None),
    );
    unavailable.inputs.retain(|(name, _)| name != "Rows");
    for call in &mut unavailable.calls {
        let target = if call["selection"] == projects::AUDIT {
            json!({"named":projects::AUDIT,"index":0})
        } else {
            json!("Primary")
        };
        call["expected"] = owned(None, "SourceUnavailable", "Rows", &[], &[], None, None);
        call["expected"]["Err"]["native_scope"] = json!({"target":target,"children":[0]});
    }
    cases.push(unavailable);
    cases.push(ordinary_case(
        "E18-representative-leaf-must-still-be-scalar",
        "basic-primary",
        change_row(source(), 0, "Label", group(vec![])),
        owned(
            Some(102),
            "ExpectedScalar",
            "Primary",
            &["Rows", "Label"],
            &["Label"],
            None,
            Some("Group"),
        ),
    ));
    let alias_input = repeated(vec![
        group(vec![("Key", text("A")), ("Label", text("a1"))]),
        group(vec![("Key", text("B")), ("Label", text("b1"))]),
        group(vec![("Key", text("A")), ("Label", text("a2"))]),
    ]);
    let alias_output = output(vec![
        root_group("a1", 1, &["a1", "a2"]),
        root_group("b1", 2, &["b1"]),
    ]);
    let alias_expected = ok(vec![
        (None, alias_output.clone()),
        (Some(projects::AUDIT.into()), alias_output.clone()),
    ]);
    cases.push(Case {
        id: "G11-legacy-Alias-canonical-empty-member-redirect",
        profile: "legacy-named-repeated-root",
        source: group(vec![("Marker", text("unused.primary"))]),
        inputs: vec![("Alias".into(), alias_input.clone())],
        native: true,
        calls: vec![
            json!({"selection":"All","expected":alias_expected}),
            json!({"selection":"Primary","expected":primary(alias_output.clone())}),
            json!({"selection":projects::AUDIT,"expected":audit(alias_output.clone())}),
        ],
    });
    let pinned_error = owned(
        Some(4),
        "UniteratedRepetition",
        "Alias",
        &["Key"],
        &["Key"],
        Some(0),
        None,
    );
    cases.push(Case {
        id: "G11-Alias-root-pin-is-not-representative",
        profile: "legacy-named-repeated-root-pin",
        source: group(vec![("Marker", text("unused.primary"))]),
        inputs: vec![("Alias".into(), alias_input)],
        native: true,
        calls: vec![
            json!({"selection":"All","expected":pinned_error.clone()}),
            json!({"selection":"Primary","expected":pinned_error}),
            json!({"selection":projects::AUDIT,"expected":audit(alias_output)}),
        ],
    });
    cases
}
