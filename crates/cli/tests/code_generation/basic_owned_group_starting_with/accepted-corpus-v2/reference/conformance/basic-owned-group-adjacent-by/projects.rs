//! Public Projects only. The independently literal outcomes live in cases.rs.
use super::*;
use mapping::{
    NamedSource, NamedTarget, OwnedPosition, OwnedSourceField, OwnedSourceRef, SequenceExpr,
    SequenceWindow as W, SortFilterOrder, SourceOwner,
};
pub const EXACT: &str = " Røws💠 ";
pub const AUDIT: &str = " audit💠 ";
pub const UNICODE_TARGET: &str = " U💠 ";
pub const PROFILES: &[&str] = &[
    "A01",
    "A02",
    "A03",
    "A04",
    "A05",
    "A06",
    "A07",
    "A08",
    "A09",
    "A10",
    "A11",
    "A12",
    "A13",
    "A14",
    "A15",
    "A16",
    "A17",
    "A18",
    "A19",
    "A20",
    "A21",
    "A22-key",
    "A22-post",
    "A23",
    "N01",
    "N02",
    "N03",
    "N04",
    "N05",
    "N06",
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
    "N12-inactive",
    "N12-bypass",
    "N12-strict",
    "N13",
    "N14",
    "T01",
    "E01",
    "E02",
    "E03",
    "E04",
    "E05",
    "E06",
    "E06-first",
    "E07",
    "E08",
    "E09",
    "E10",
    "E11",
    "E12",
    "E13",
    "E14",
    "E15",
    "E16-window",
    "E16-post",
    "E16-reached",
    "E17-position",
    "E17-pin",
    "E17-unpinned",
    "E18-row",
    "E18-global",
    "I01",
    "I02",
    "I03",
    "I04-empty",
    "I04-null",
    "I04-producer",
    "I05-generate",
    "I05-tokenize",
    "I05-length",
    "I05-regex",
    "I05-nested",
    "C02",
    "C03",
];
pub fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
pub fn owner(name: &str) -> SourceOwner {
    if name == "Primary" {
        SourceOwner::Primary
    } else {
        SourceOwner::Named { name: name.into() }
    }
}
pub fn field(name: &str, parts: &[&str], pin: Option<&[&str]>) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner: owner(name),
            path: path(parts),
        },
        frame: pin.map(path),
    })
}
pub fn position(name: &str, parts: &[&str]) -> Node {
    Node::OwnedPosition(OwnedPosition {
        owner: owner(name),
        collection: Some(path(parts)),
    })
}
pub fn iteration(name: &str, parts: &[&str]) -> ScopeIteration {
    ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner: owner(name),
            path: path(parts),
        },
        output_path: None,
    }
}
pub fn bindings(fields: &[(&str, u32)]) -> Vec<Binding> {
    fields
        .iter()
        .map(|(name, node)| Binding {
            target_field: (*name).into(),
            node: *node,
        })
        .collect()
}
pub fn mixed(name: &str) -> SchemaNode {
    SchemaNode::scalar_union(
        name,
        ir::ScalarTypeSet::new([
            ScalarType::String,
            ScalarType::Int,
            ScalarType::Float,
            ScalarType::Bool,
        ])
        .unwrap(),
    )
}
pub fn row_schema() -> SchemaNode {
    SchemaNode::group(
        "Rows",
        vec![
            mixed("Key"),
            string("Label"),
            bool_("Keep"),
            int("Rank"),
            int("Limit"),
        ],
    )
    .repeating()
}
pub fn endpoint() -> SchemaNode {
    SchemaNode::group(
        "Endpoint",
        vec![string("Marker"), int("Limit"), row_schema()],
    )
}
fn member_schema() -> SchemaNode {
    SchemaNode::group(
        "Members",
        vec![
            string("Label"),
            int("MemberPosition"),
            int("LegacyPosition"),
        ],
    )
    .repeating()
}
pub fn groups_schema() -> SchemaNode {
    SchemaNode::group(
        "Groups",
        vec![
            string("First"),
            int("GroupPosition"),
            string("RootMarker"),
            member_schema(),
        ],
    )
    .repeating()
}
fn grouped(name: &str, offset: u32) -> Scope {
    Scope {
        target_field: "Groups".into(),
        iteration: iteration(name, &["Rows"]),
        group_adjacent_by: Some(offset + 1),
        bindings: bindings(&[
            ("First", offset + 2),
            ("GroupPosition", offset + 5),
            ("RootMarker", offset + 6),
        ]),
        children: vec![Scope {
            target_field: "Members".into(),
            iteration: iteration(name, &["Rows"]),
            bindings: bindings(&[
                ("Label", offset + 2),
                ("MemberPosition", offset + 5),
                ("LegacyPosition", 600),
            ]),
            ..Default::default()
        }],
        ..Default::default()
    }
}
fn constant(p: &mut Project, id: u32, value: Value) {
    p.graph.nodes.insert(id, Node::Const { value });
}
fn call(p: &mut Project, id: u32, function: &str, args: &[u32]) {
    p.graph.nodes.insert(
        id,
        Node::Call {
            function: function.into(),
            args: args.to_vec(),
        },
    );
}
pub fn base() -> Project {
    let mut nodes = BTreeMap::new();
    for (name, offset) in [("Primary", 0), ("Rows", 200), (EXACT, 300)] {
        for (id, field_name) in [
            (1, "Key"),
            (2, "Label"),
            (3, "Keep"),
            (4, "Rank"),
            (8, "Limit"),
        ] {
            nodes.insert(
                offset + id,
                field(name, &["Rows", field_name], Some(&["Rows"])),
            );
        }
        nodes.insert(offset + 5, position(name, &["Rows"]));
        nodes.insert(offset + 6, field(name, &["Marker"], Some(&[])));
        nodes.insert(offset + 7, field(name, &["Limit"], Some(&[])));
    }
    nodes.insert(600, Node::Position { collection: vec![] });
    let mut p = Project {
        source: endpoint(),
        target: SchemaNode::group("Output", vec![groups_schema()]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: ["unusedStatic", "Rows", EXACT, "unavailableDynamic"]
            .into_iter()
            .map(|name| NamedSource {
                name: name.into(),
                path: format!("unused/{name}.json"),
                schema: endpoint(),
                options: Default::default(),
                dynamic_path: (name == "unavailableDynamic").then_some(
                    mapping::DynamicSourcePath {
                        node: 900,
                        iteration: vec![],
                    },
                ),
            })
            .collect(),
        extra_targets: [(AUDIT, 206), (UNICODE_TARGET, 306)]
            .into_iter()
            .map(|(name, node)| NamedTarget {
                name: name.into(),
                path: None,
                schema: SchemaNode::group("Audit", vec![string("Marker")]),
                options: Default::default(),
                root: Scope {
                    bindings: bindings(&[("Marker", node)]),
                    ..Default::default()
                },
            })
            .collect(),
        graph: Graph { nodes },
        root: Scope {
            children: vec![grouped("Primary", 0)],
            ..Default::default()
        },
        failure_rules: vec![],
        user_functions: Default::default(),
    };
    for (id, value) in [
        (900, Value::String("unused.json".into())),
        (901, Value::Bool(true)),
        (902, Value::Bool(false)),
        (910, Value::Int(0)),
        (911, Value::Int(1)),
        (912, Value::Int(2)),
        (913, Value::Int(3)),
        (914, Value::Int(4)),
        (920, Value::String("A".into())),
        (921, Value::String("C".into())),
        (922, Value::String("not bool".into())),
        (923, Value::String("audit".into())),
    ] {
        constant(&mut p, id, value);
    }
    call(&mut p, 30, "greater_than", &[5, 912]);
    call(&mut p, 31, "less_or_equal", &[5, 913]);
    call(&mut p, 32, "equal", &[5, 914]);
    call(&mut p, 33, "equal", &[1, 920]);
    call(&mut p, 34, "equal", &[5, 912]);
    call(&mut p, 35, "equal", &[1, 921]);
    for (id, message) in [
        (101, "key"),
        (102, "filter"),
        (103, "sort"),
        (104, "bound"),
        (105, "post"),
        (106, "target"),
    ] {
        constant(&mut p, id + 1000, Value::String(message.into()));
        p.graph.nodes.insert(
            id,
            Node::Raise {
                message: Some(id + 1000),
            },
        );
    }
    p
}
fn first(p: &mut Project) {
    p.root.children[0].iteration_output = mapping::IterationOutput::First;
    let ir::SchemaKind::Group { children, .. } = &mut p.target.kind else {
        unreachable!()
    };
    children[0].repeating = false;
}
fn nested(
    p: &mut Project,
    outer_name: &str,
    outer: u32,
    inner_name: &str,
    inner: u32,
    same: bool,
    ordinary: bool,
) {
    let mut outside = grouped(outer_name, outer);
    outside.target_field.clear();
    outside.bindings = bindings(&[("OuterFirst", outer + 2), ("OuterPosition", outer + 5)]);
    outside.children.clear();
    outside.windows = if same {
        vec![W::FromTo {
            first: 913,
            last: 913,
        }]
    } else {
        vec![W::First { count: 912 }]
    };
    let mut inside = grouped(inner_name, inner);
    if same {
        inside.group_adjacent_by = Some(920);
    }
    if ordinary {
        inside.group_adjacent_by = None;
        inside.group_by = Some(inner + 1);
    }
    outside.children.push(Scope {
        target_field: "Inner".into(),
        children: vec![inside],
        ..Default::default()
    });
    let mut fields = vec![
        string("OuterFirst"),
        int("OuterPosition"),
        SchemaNode::group("Inner", vec![groups_schema()]),
    ];
    if ordinary {
        let mut members = grouped(outer_name, outer).children.remove(0);
        members.target_field = "Ordinary".into();
        outside.children.insert(0, members);
        let mut schema = member_schema();
        schema.name = "Ordinary".into();
        fields.insert(2, schema);
    }
    p.root = outside;
    p.target = SchemaNode::group("Output", fields).repeating();
}
fn batches(p: &mut Project, per_parent: bool) {
    p.source = SchemaNode::group(
        "Endpoint",
        vec![
            string("Marker"),
            int("Limit"),
            row_schema(),
            SchemaNode::group(
                "Batches",
                vec![string("BatchName"), int("Limit"), row_schema()],
            )
            .repeating(),
        ],
    );
    for (id, name) in [(401, "Key"), (402, "Label")] {
        p.graph.nodes.insert(
            id,
            field(
                "Primary",
                &["Batches", "Rows", name],
                Some(&["Batches", "Rows"]),
            ),
        );
    }
    p.graph
        .nodes
        .insert(405, position("Primary", &["Batches", "Rows"]));
    p.graph.nodes.insert(
        406,
        field("Primary", &["Batches", "BatchName"], Some(&["Batches"])),
    );
    p.graph.nodes.insert(
        407,
        field("Primary", &["Batches", "Limit"], Some(&["Batches"])),
    );
    p.graph.nodes.insert(
        408,
        Node::Position {
            collection: path(&["Batches", "Rows"]),
        },
    );
    let mut scope = Scope {
        target_field: "Groups".into(),
        iteration: iteration("Primary", &["Batches", "Rows"]),
        group_adjacent_by: Some(401),
        bindings: bindings(&[("First", 402), ("GroupPosition", 405), ("BatchName", 406)]),
        children: vec![Scope {
            target_field: "Members".into(),
            iteration: iteration("Primary", &["Batches", "Rows"]),
            bindings: bindings(&[
                ("Label", 402),
                ("MemberPosition", 405),
                ("LegacyPosition", 408),
                ("BatchName", 406),
            ]),
            ..Default::default()
        }],
        ..Default::default()
    };
    let schema = SchemaNode::group(
        "Groups",
        vec![
            string("First"),
            int("GroupPosition"),
            string("BatchName"),
            SchemaNode::group(
                "Members",
                vec![
                    string("Label"),
                    int("MemberPosition"),
                    int("LegacyPosition"),
                    string("BatchName"),
                ],
            )
            .repeating(),
        ],
    )
    .repeating();
    if per_parent {
        scope.windows = vec![W::First { count: 407 }];
        p.root = Scope {
            children: vec![Scope {
                target_field: "Batches".into(),
                iteration: iteration("Primary", &["Batches"]),
                bindings: bindings(&[("BatchName", 406)]),
                children: vec![scope],
                ..Default::default()
            }],
            ..Default::default()
        };
        p.target = SchemaNode::group(
            "Output",
            vec![SchemaNode::group("Batches", vec![string("BatchName"), schema]).repeating()],
        );
    } else {
        p.root.children = vec![scope];
        p.target = SchemaNode::group("Output", vec![schema]);
    }
}
fn roots(profile: &str) -> Project {
    let mut p = base();
    let named = profile.contains("-N") || profile.contains("Alias");
    let alias = profile.contains("Alias");
    let name = if alias {
        "Alias"
    } else if named {
        "Rows"
    } else {
        "Primary"
    };
    let declared_repeated = !profile.starts_with("N07") && !profile.ends_with("false");
    let mut schema = SchemaNode::group("Root", vec![mixed("Key"), string("Label")]);
    schema.repeating = declared_repeated;
    if named {
        p.source = SchemaNode::group("Primary", vec![]);
        p.extra_sources[1].name = name.into();
        p.extra_sources[1].schema = schema;
    } else {
        p.source = schema;
    }
    // Drop row-only stored helpers: the root fixture has a distinct schema.
    p.graph.nodes.retain(|id, _| {
        [
            206, 306, 900, 901, 902, 910, 911, 912, 913, 914, 920, 921, 922, 923,
        ]
        .contains(id)
    });
    if named {
        p.graph.nodes.insert(
            206,
            Node::Const {
                value: Value::String("audit".into()),
            },
        );
    }
    for (id, field_name, pin) in [
        (1, "Key", None),
        (2, "Label", None),
        (10, "Key", Some(&[][..])),
        (11, "Label", Some(&[][..])),
    ] {
        p.graph.nodes.insert(id, field(name, &[field_name], pin));
    }
    p.graph.nodes.insert(5, position(name, &[]));
    p.graph.nodes.insert(
        600,
        Node::Position {
            collection: if alias { path(&["Alias"]) } else { vec![] },
        },
    );
    let route = if alias {
        ScopeIteration::Source(path(&["Alias"]))
    } else {
        iteration(name, &[])
    };
    p.root = Scope {
        iteration: route.clone(),
        group_adjacent_by: Some(1),
        bindings: bindings(&[
            (
                "First",
                if profile.starts_with("N09") {
                    10
                } else if profile.starts_with("N07") {
                    11
                } else {
                    2
                },
            ),
            ("GroupPosition", 5),
        ]),
        children: vec![Scope {
            target_field: "Members".into(),
            iteration: route,
            bindings: bindings(&[
                ("Label", 11),
                ("MemberPosition", 5),
                ("LegacyPosition", 600),
            ]),
            ..Default::default()
        }],
        ..Default::default()
    };
    p.target = SchemaNode::group(
        "Output",
        vec![string("First"), int("GroupPosition"), member_schema()],
    )
    .repeating();
    if profile == "N10" {
        p.root.group_adjacent_by = None;
    }
    if profile == "N11" {
        // A newer Item belonging to Rows must not shadow Primary's root group.
        p.extra_sources[1].schema =
            SchemaNode::group("Root", vec![mixed("Key"), string("Label")]).repeating();
        p.graph
            .nodes
            .insert(12, field("Primary", &["Key"], Some(&[])));
        p.root.children[0].iteration = iteration("Rows", &[]);
        p.root.children[0].bindings = bindings(&[("Label", 12)]);
        p.graph.nodes.insert(
            206,
            Node::Const {
                value: Value::String("audit".into()),
            },
        );
    }
    p
}
fn item_at(p: &mut Project, producer: u8, index: u32, id: u32, item: u32) {
    for (node, value) in [
        (950, Value::String("A|B|A".into())),
        (951, Value::String("|".into())),
        (952, Value::String("[|]".into())),
        (953, Value::Int(10)),
        (954, Value::Int(12)),
        (955, Value::String("ABA".into())),
        (956, Value::String("0|2".into())),
    ] {
        constant(p, node, value);
    }
    p.graph.nodes.insert(
        item,
        Node::SourceField {
            path: vec![],
            frame: None,
        },
    );
    let sequence = match producer {
        0 => SequenceExpr::Generate {
            from: Some(953),
            to: 954,
            item,
        },
        1 => SequenceExpr::Tokenize {
            input: 950,
            delimiter: 951,
            item,
        },
        2 => SequenceExpr::TokenizeByLength {
            input: 955,
            length: 911,
            item,
        },
        _ => SequenceExpr::TokenizeRegex {
            input: 950,
            pattern: 952,
            flags: None,
            item,
        },
    };
    p.graph
        .nodes
        .insert(id, Node::SequenceItemAt { sequence, index });
}
pub fn project(profile: &str) -> Project {
    assert!(
        PROFILES.contains(&profile),
        "unknown authored profile {profile}"
    );
    if profile.starts_with("N07")
        || profile.starts_with("N08")
        || profile.starts_with("N09")
        || profile == "N10"
        || profile == "N11"
    {
        return roots(profile);
    }
    let mut p = base();
    match profile {
        "A01" | "E15" => {}
        "A02" => p.root.children[0] = grouped("Rows", 200),
        "A03" => p.root.children[0] = grouped(EXACT, 300),
        "A04" => p.root.children[0].group_adjacent_by = Some(920),
        "A05" => p.root.children[0].post_group_filter = Some(3),
        "A06" => p.root.children[0].post_group_filter = Some(902),
        "A07" => p.root.children[0].filter = Some(33),
        "A08" | "A09" | "A10" | "A11" | "A12" | "A14" => {
            let g = &mut p.root.children[0];
            g.sort_by = Some(4);
            if ["A09", "A10", "A11"].contains(&profile) {
                g.filter = Some(30);
            }
            if profile == "A09" {
                g.sort_filter_order = SortFilterOrder::FilterThenSort;
            }
            if profile == "A11" {
                g.post_group_filter = Some(32);
            }
            if profile == "A12" {
                g.group_adjacent_by = Some(31);
            }
            if profile == "A14" {
                g.windows = vec![W::First { count: 7 }];
            }
        }
        "A13" => {
            p.root.children[0].sort_by = Some(5);
            p.root.children[0].sort_descending = true;
        }
        "A15" => {
            p.root.children[0].windows = vec![W::SkipFirst { count: 911 }, W::Last { count: 912 }]
        }
        "A16" => {
            p.root.children[0].windows = vec![W::Last { count: 912 }, W::SkipFirst { count: 911 }]
        }
        "A17" => {
            p.root.children[0].windows = vec![W::FromTo {
                first: 913,
                last: 914,
            }]
        }
        "A18" | "A19" => first(&mut p),
        "A20" => {
            p.root.children[0].windows = vec![W::First { count: 910 }];
            p.root.children[0].bindings[0].node = 106;
        }
        "A21" => {
            p.graph.nodes.insert(
                75,
                Node::ValueMap {
                    input: 5,
                    input_type: None,
                    table: vec![
                        (Value::Int(1), Value::Bool(true)),
                        (Value::Int(3), Value::Bool(true)),
                        (Value::Int(5), Value::Bool(true)),
                    ],
                    default: Some(Value::Bool(false)),
                },
            );
            p.graph.nodes.insert(
                76,
                Node::If {
                    condition: 75,
                    then: 1,
                    else_: 101,
                },
            );
            p.graph.nodes.insert(
                77,
                Node::If {
                    condition: 75,
                    then: 901,
                    else_: 105,
                },
            );
            p.root.children[0].group_adjacent_by = Some(76);
            p.root.children[0].post_group_filter = Some(77);
        }
        "A22-key" | "A22-post" => {
            let g = &mut p.root.children[0];
            g.iteration = ScopeIteration::Source(path(&["Rows"]));
            if profile == "A22-post" {
                g.group_adjacent_by = Some(601);
                g.post_group_filter = Some(605);
            }
            g.bindings = bindings(&[("First", 602), ("GroupPosition", 600), ("RootMarker", 603)]);
            g.children[0].iteration = ScopeIteration::Source(vec![]);
            g.children[0].bindings = bindings(&[
                ("Label", 602),
                ("MemberPosition", 600),
                ("LegacyPosition", 600),
            ]);
        }
        "A23" => {
            let ir::SchemaKind::Group { children, .. } = &mut p.target.kind else {
                unreachable!()
            };
            for (mode, name) in ["By", "Adjacent", "Starting", "Ending", "Blocks"]
                .into_iter()
                .enumerate()
            {
                let mut g = grouped("Primary", 0);
                g.target_field = name.into();
                g.iteration = ScopeIteration::Source(path(&["Rows"]));
                g.group_adjacent_by = None;
                g.bindings =
                    bindings(&[("First", 602), ("GroupPosition", 600), ("RootMarker", 603)]);
                g.children[0].iteration = ScopeIteration::Source(vec![]);
                g.children[0].bindings = bindings(&[
                    ("Label", 602),
                    ("MemberPosition", 600),
                    ("LegacyPosition", 600),
                ]);
                match mode {
                    0 => g.group_by = Some(601),
                    1 => g.group_adjacent_by = Some(601),
                    2 => g.group_starting_with = Some(604),
                    3 => g.group_ending_with = Some(604),
                    _ => g.group_into_blocks = Some(912),
                }
                p.root.children.push(g);
                let mut schema = groups_schema();
                schema.name = name.into();
                children.push(schema);
            }
        }
        "N01" => nested(&mut p, "Primary", 0, "Rows", 200, false, false),
        "N02" => nested(&mut p, "Rows", 200, "Primary", 0, false, false),
        "N03" | "N04" => nested(&mut p, "Primary", 0, "Primary", 0, true, profile == "N04"),
        "N05" | "N06" => batches(&mut p, profile == "N06"),
        "N12-inactive" | "N12-bypass" | "N12-strict" => {
            p.source.repeating = profile == "N12-inactive";
            p.graph.nodes.insert(
                20,
                field(
                    "Primary",
                    if profile == "N12-strict" {
                        &["Rows", "Label"]
                    } else {
                        &["Marker"]
                    },
                    Some(&[]),
                ),
            );
            p.root.children[0].bindings[0].node = 20;
        }
        "N13" => {
            p.source = mixed("Input").repeating();
            p.target = mixed("Output").repeating();
            p.graph.nodes.retain(|id, _| [206, 306, 900].contains(id));
            p.graph.nodes.insert(1, field("Primary", &[], None));
            p.root = Scope {
                iteration: iteration("Primary", &[]),
                group_adjacent_by: Some(1),
                construction: mapping::ScopeConstruction::Scalar { value: 1 },
                ..Default::default()
            };
        }
        "N14" => {
            let ir::SchemaKind::Group { children, .. } = &mut p.source.kind else {
                unreachable!()
            };
            let ir::SchemaKind::Group { children: rows, .. } = &mut children[2].kind else {
                unreachable!()
            };
            rows.push(SchemaNode::group("Details", vec![string("Value")]).repeating());
            p.graph.nodes.insert(
                20,
                field("Primary", &["Rows", "Details", "Value"], Some(&["Rows"])),
            );
            p.root.children[0].bindings[0].node = 20;
        }
        "T01" => {
            let ir::SchemaKind::Group { children, .. } = &mut p.target.kind else {
                unreachable!()
            };
            let ir::SchemaKind::Group {
                children: fields, ..
            } = &mut children[0].kind
            else {
                unreachable!()
            };
            fields.insert(0, mixed("Key"));
            let ir::SchemaKind::Group {
                children: members, ..
            } = &mut fields[4].kind
            else {
                unreachable!()
            };
            members.insert(0, mixed("Key"));
            p.root.children[0].bindings.insert(
                0,
                Binding {
                    target_field: "Key".into(),
                    node: 1,
                },
            );
            p.root.children[0].children[0].bindings.insert(
                0,
                Binding {
                    target_field: "Key".into(),
                    node: 1,
                },
            );
        }
        "E01" | "E02" | "E03" => {
            p.graph.nodes.insert(
                40,
                Node::If {
                    condition: 34,
                    then: 102,
                    else_: 901,
                },
            );
            let g = &mut p.root.children[0];
            g.filter = Some(40);
            g.group_adjacent_by = Some(if profile == "E02" { 1 } else { 101 });
            g.post_group_filter = Some(105);
            if profile == "E03" {
                g.sort_by = Some(103);
                g.sort_filter_order = SortFilterOrder::FilterThenSort;
                g.windows = vec![W::First { count: 104 }];
            }
        }
        "E04" | "E05" => {
            let g = &mut p.root.children[0];
            g.sort_by = Some(if profile == "E04" { 103 } else { 4 });
            g.windows = vec![W::First { count: 104 }];
            g.filter = Some(102);
            g.group_adjacent_by = Some(101);
            g.post_group_filter = Some(105);
        }
        "E06" | "E06-first" | "E07" => {
            let g = &mut p.root.children[0];
            g.windows = vec![W::First { count: 910 }];
            if profile == "E07" {
                g.post_group_filter = Some(105);
            } else {
                g.group_adjacent_by = Some(101);
            }
            if profile == "E06-first" {
                first(&mut p);
            }
        }
        "E08" => {
            p.graph.nodes.insert(
                40,
                Node::If {
                    condition: 34,
                    then: 105,
                    else_: 901,
                },
            );
            p.root.children[0].group_adjacent_by = Some(920);
            p.root.children[0].post_group_filter = Some(40);
        }
        "E09" => {
            p.root.children[0].post_group_filter = Some(105);
            constant(&mut p, 105, Value::String("not bool".into()));
        }
        "E10" => {
            p.root.children[0].windows = vec![W::First { count: 104 }];
            constant(&mut p, 104, Value::Bool(true));
        }
        "E11" => {
            p.root.children[0].windows = vec![W::First { count: 910 }, W::First { count: 104 }]
        }
        "E12" => {
            let g = &mut p.root.children[0];
            g.filter = Some(902);
            g.group_adjacent_by = Some(101);
            g.post_group_filter = Some(105);
            g.windows = vec![W::First { count: 7 }];
            g.bindings[0].node = 106;
        }
        "E13" => {
            p.root.children[0].post_group_filter = Some(902);
            p.root.children[0].bindings[0].node = 106;
        }
        "E14" => {
            p.root.children[0].post_group_filter = Some(3);
            p.graph.nodes.insert(
                40,
                Node::If {
                    condition: 35,
                    then: 106,
                    else_: 2,
                },
            );
            p.root.children[0].bindings[0].node = 40;
        }
        "E16-window" | "E16-post" | "E16-reached" => {
            p.root.children[0].bindings[0].node = 202;
            if profile == "E16-window" {
                p.root.children[0].windows = vec![W::First { count: 910 }];
            }
            if profile == "E16-post" {
                p.root.children[0].post_group_filter = Some(902);
            }
        }
        "E17-position" => p.root.children[0].bindings[1].node = 205,
        "E17-pin" => p.root.children[0].bindings[0].node = 202,
        "E17-unpinned" => {
            p.graph
                .nodes
                .insert(20, field("Rows", &["Rows", "Label"], None));
            p.root.children[0].bindings[0].node = 20;
        }
        "E18-row" | "E18-global" | "C03" => {
            p.graph.nodes.insert(
                50,
                Node::SourceField {
                    path: path(&["Rows", "Marker"]),
                    frame: None,
                },
            );
            constant(&mut p, 51, Value::String("row.reject".into()));
            p.failure_rules = vec![mapping::FailureRule {
                iteration: mapping::FailureIteration::Source {
                    collection: if profile == "E18-row" {
                        path(&["Rows"])
                    } else {
                        vec![]
                    },
                },
                selection: if profile == "E18-row" {
                    mapping::FailureSelection::WhenTrue { predicate: 3 }
                } else if profile == "C03" {
                    mapping::FailureSelection::WhenTrue { predicate: 902 }
                } else {
                    mapping::FailureSelection::All
                },
                message: Some(if profile == "C03" { 50 } else { 51 }),
            }];
            if profile == "E18-global" {
                constant(&mut p, 51, Value::String("global.reject".into()));
            }
            if profile != "C03" {
                p.root.children[0].group_adjacent_by = Some(101);
            }
        }
        "I01" | "I02" | "I03" | "I04-empty" | "I04-null" | "I04-producer" | "I05-generate"
        | "I05-tokenize" | "I05-length" | "I05-regex" | "I05-nested" => {
            let producer = if profile == "I02" || profile == "I05-generate" {
                0
            } else if profile == "I05-length" {
                2
            } else if profile == "I05-regex" {
                3
            } else {
                1
            };
            item_at(&mut p, producer, 5, 70, 970);
            match profile {
                "I01" => {
                    let ir::SchemaKind::Group { children, .. } = &mut p.source.kind else {
                        unreachable!()
                    };
                    let ir::SchemaKind::Group { children: rows, .. } = &mut children[2].kind else {
                        unreachable!()
                    };
                    rows.push(int("Index"));
                    p.graph
                        .nodes
                        .insert(9, field("Primary", &["Rows", "Index"], Some(&["Rows"])));
                    let Node::SequenceItemAt { index, .. } = p.graph.nodes.get_mut(&70).unwrap()
                    else {
                        unreachable!()
                    };
                    *index = 9;
                    p.root.children[0].group_adjacent_by = Some(70);
                }
                "I02" => {
                    let ir::SchemaKind::Group { children, .. } = &mut p.target.kind else {
                        unreachable!()
                    };
                    let ir::SchemaKind::Group {
                        children: fields, ..
                    } = &mut children[0].kind
                    else {
                        unreachable!()
                    };
                    fields.insert(3, mixed("Selected"));
                    let ir::SchemaKind::Group {
                        children: members, ..
                    } = &mut fields[4].kind
                    else {
                        unreachable!()
                    };
                    members.push(mixed("Selected"));
                    p.root.children[0].bindings.push(Binding {
                        target_field: "Selected".into(),
                        node: 70,
                    });
                    p.root.children[0].children[0].bindings.push(Binding {
                        target_field: "Selected".into(),
                        node: 70,
                    });
                }
                "I03" => {
                    let ir::SchemaKind::Group { children, .. } = &mut p.source.kind else {
                        unreachable!()
                    };
                    children.push(int("Index"));
                    let ir::SchemaKind::Group { children, .. } =
                        &mut p.extra_sources[1].schema.kind
                    else {
                        unreachable!()
                    };
                    children.push(int("Index"));
                    p.graph
                        .nodes
                        .insert(71, field("Rows", &["Index"], Some(&[])));
                    p.graph.nodes.insert(
                        70,
                        Node::SequenceItemAt {
                            sequence: SequenceExpr::Tokenize {
                                input: 956,
                                delimiter: 951,
                                item: 970,
                            },
                            index: 71,
                        },
                    );
                    p.root.children[0].sort_by = Some(4);
                    p.root.children[0].windows = vec![W::First { count: 70 }];
                }
                "I04-empty" | "I04-null" | "I04-producer" => {
                    p.graph.nodes.insert(
                        70,
                        Node::SequenceItemAt {
                            sequence: SequenceExpr::Generate {
                                from: Some(953),
                                to: 954,
                                item: 970,
                            },
                            index: 105,
                        },
                    );
                    if profile == "I04-producer" {
                        p.graph.nodes.insert(
                            953,
                            Node::Raise {
                                message: Some(1101),
                            },
                        );
                    } else {
                        constant(
                            &mut p,
                            953,
                            if profile == "I04-null" {
                                Value::Null
                            } else {
                                Value::Int(13)
                            },
                        );
                    }
                    p.root.children[0].group_adjacent_by = Some(70);
                }
                _ => {
                    let selected = if profile == "I05-nested" {
                        item_at(&mut p, 0, 5, 72, 972);
                        constant(&mut p, 953, Value::Int(1));
                        constant(&mut p, 954, Value::Int(3));
                        let Node::SequenceItemAt { index, .. } =
                            p.graph.nodes.get_mut(&70).unwrap()
                        else {
                            unreachable!()
                        };
                        *index = 72;
                        70
                    } else {
                        70
                    };
                    call(
                        &mut p,
                        73,
                        "equal",
                        &[selected, if producer == 0 { 954 } else { 920 }],
                    );
                    p.root.children[0].post_group_filter = Some(73);
                }
            }
        }
        "C02" => {
            p.graph
                .nodes
                .insert(51, field("Primary", &["Marker"], Some(&[])));
            p.graph.nodes.insert(
                50,
                Node::SourceField {
                    path: path(&["Rows", "Marker"]),
                    frame: None,
                },
            );
            p.graph.nodes.insert(
                6,
                Node::If {
                    condition: 901,
                    then: 51,
                    else_: 50,
                },
            );
        }
        _ => panic!("unimplemented authored profile {profile}"),
    }
    if ["A22-key", "A22-post", "A23"].contains(&profile) {
        for (id, name) in [(601, "Key"), (602, "Label")] {
            p.graph.nodes.insert(
                id,
                Node::SourceField {
                    path: path(&[name]),
                    frame: Some(path(&["Rows"])),
                },
            );
        }
        p.graph.nodes.insert(
            603,
            Node::SourceField {
                path: path(&["Marker"]),
                frame: None,
            },
        );
        call(&mut p, 604, "equal", &[601, 920]);
        p.graph.nodes.insert(
            605,
            Node::If {
                condition: 3,
                then: 901,
                else_: 901,
            },
        );
    }
    if profile == "E15" {
        p.root.children[0] = grouped("Rows", 200);
    }
    if profile == "E15" || profile.starts_with("E16") {
        constant(&mut p, 206, Value::String("audit".into()));
    }
    p
}
