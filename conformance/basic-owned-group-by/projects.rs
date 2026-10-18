//! Public Project constructors; no Program is assembled in this corpus.
use super::*;
use mapping::{
    NamedSource, NamedTarget, OwnedPosition, OwnedSourceField, OwnedSourceRef, SequenceWindow,
    SortFilterOrder, SourceOwner,
};

pub const AUDIT: &str = " audit💠 ";
pub const EXACT: &str = " Røws💠 ";
pub fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).into()).collect()
}
pub fn owner(base: u32) -> SourceOwner {
    match base {
        100 => SourceOwner::Primary,
        200 => SourceOwner::Named {
            name: "Rows".into(),
        },
        300 => SourceOwner::Named { name: EXACT.into() },
        _ => panic!("authored owner"),
    }
}
fn field(owner: SourceOwner, parts: &[&str], pin: Option<&[&str]>) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame: pin.map(path),
    })
}
fn position(owner: SourceOwner, parts: &[&str]) -> Node {
    Node::OwnedPosition(OwnedPosition {
        owner,
        collection: Some(path(parts)),
    })
}
pub fn iteration(base: u32, parts: &[&str]) -> ScopeIteration {
    ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner: owner(base),
            path: path(parts),
        },
        output_path: None,
    }
}
fn bindings(fields: &[(&str, u32)]) -> Vec<Binding> {
    fields
        .iter()
        .map(|(name, node)| Binding {
            target_field: (*name).into(),
            node: *node,
        })
        .collect()
}
fn mixed(name: &str) -> SchemaNode {
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
fn rows() -> SchemaNode {
    SchemaNode::group(
        "Rows",
        vec![
            mixed("Key"),
            string("Label"),
            bool_("Keep"),
            int("Rank"),
            int("Limit"),
            bool_("KeyFail"),
            bool_("PostFail"),
            bool_("FilterFail"),
            bool_("SortFail"),
            bool_("TargetFail"),
            bool_("Reject"),
            SchemaNode::group("Details", vec![string("Code")]).repeating(),
        ],
    )
    .repeating()
}
fn endpoint() -> SchemaNode {
    SchemaNode::group(
        "Endpoint",
        vec![
            string("Marker"),
            int("Limit"),
            bool_("GlobalReject"),
            rows(),
            SchemaNode::group("Batch", vec![string("Id"), int("Limit"), rows()]).repeating(),
        ],
    )
}
fn member_schema() -> SchemaNode {
    SchemaNode::group(
        "Members",
        vec![string("Label"), int("MemberPosition"), string("RootMarker")],
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
fn output() -> SchemaNode {
    SchemaNode::group("Output", vec![groups_schema()])
}
fn member(base: u32, legacy: bool) -> Scope {
    Scope {
        target_field: "Members".into(),
        iteration: if legacy {
            ScopeIteration::Source(vec![])
        } else {
            iteration(base, &["Rows"])
        },
        bindings: bindings(&[
            ("Label", base + 2),
            ("MemberPosition", base + 5),
            ("RootMarker", base + 6),
        ]),
        ..Default::default()
    }
}
pub fn grouped(base: u32) -> Scope {
    Scope {
        target_field: "Groups".into(),
        iteration: iteration(base, &["Rows"]),
        group_by: Some(base + 1),
        bindings: bindings(&[
            ("First", base + 2),
            ("GroupPosition", base + 5),
            ("RootMarker", base + 6),
        ]),
        children: vec![member(base, false)],
        ..Default::default()
    }
}
fn graph() -> Graph {
    let mut nodes = BTreeMap::new();
    for base in [100, 200, 300] {
        for (offset, name) in [
            (1, "Key"),
            (2, "Label"),
            (3, "Keep"),
            (4, "Rank"),
            (8, "Limit"),
            (9, "KeyFail"),
            (10, "PostFail"),
            (11, "FilterFail"),
            (12, "SortFail"),
            (13, "TargetFail"),
        ] {
            nodes.insert(
                base + offset,
                field(owner(base), &["Rows", name], Some(&["Rows"])),
            );
        }
        nodes.insert(base + 5, position(owner(base), &["Rows"]));
        nodes.insert(base + 6, field(owner(base), &["Marker"], Some(&[])));
        nodes.insert(base + 7, field(owner(base), &["Limit"], Some(&[])));
        for (id, condition, then, else_) in [
            (20, base + 9, 501, base + 1),
            (21, base + 10, 502, base + 3),
            (22, base + 11, 503, 900),
            (23, base + 12, 504, base + 4),
            (24, base + 13, 506, base + 2),
        ] {
            nodes.insert(
                base + id,
                Node::If {
                    condition,
                    then,
                    else_,
                },
            );
        }
        for (offset, bound) in [(30, 903), (31, 905), (33, 904)] {
            nodes.insert(
                base + offset,
                Node::Call {
                    function: "greater_than".into(),
                    args: vec![base + 5, bound],
                },
            );
        }
    }
    for (id, name) in [(400, "Label"), (401, "Key"), (402, "Keep"), (409, "Limit")] {
        nodes.insert(
            id,
            field(
                SourceOwner::Primary,
                &["Batch", "Rows", name],
                Some(&["Batch", "Rows"]),
            ),
        );
    }
    nodes.insert(403, position(SourceOwner::Primary, &["Batch", "Rows"]));
    nodes.insert(
        404,
        field(SourceOwner::Primary, &["Batch", "Id"], Some(&["Batch"])),
    );
    nodes.insert(405, position(SourceOwner::Primary, &["Batch"]));
    nodes.insert(
        406,
        field(SourceOwner::Primary, &["Batch", "Limit"], Some(&["Batch"])),
    );
    for (id, message) in [
        (501, "key.poison"),
        (502, "post.poison"),
        (503, "filter.poison"),
        (504, "sort.poison"),
        (505, "bound.poison"),
        (506, "target.poison"),
    ] {
        nodes.insert(
            id,
            Node::Raise {
                message: Some(id + 50),
            },
        );
        nodes.insert(
            id + 50,
            Node::Const {
                value: Value::String(message.into()),
            },
        );
    }
    for (id, value) in [
        (900, Value::Bool(true)),
        (901, Value::Bool(false)),
        (902, Value::Int(0)),
        (903, Value::Int(1)),
        (904, Value::Int(2)),
        (905, Value::Int(3)),
        (906, Value::String("not-bool".into())),
        (907, Value::Bool(true)),
    ] {
        nodes.insert(id, Node::Const { value });
    }
    for (id, name) in [(601, "Label"), (604, "Key"), (605, "Keep")] {
        nodes.insert(
            id,
            Node::SourceField {
                path: path(&[name]),
                frame: Some(path(&["Rows"])),
            },
        );
    }
    nodes.insert(
        602,
        Node::Position {
            collection: path(&["Rows"]),
        },
    );
    nodes.insert(
        603,
        Node::SourceField {
            path: path(&["Marker"]),
            frame: None,
        },
    );
    nodes.insert(
        606,
        Node::SourceField {
            path: path(&["RequiredLegacy", "Marker"]),
            frame: None,
        },
    );
    nodes.insert(
        701,
        field(SourceOwner::Primary, &["GlobalReject"], Some(&[])),
    );
    nodes.insert(
        702,
        Node::Const {
            value: Value::String("global.reject".into()),
        },
    );
    nodes.insert(
        703,
        field(SourceOwner::Primary, &["Rows", "Reject"], Some(&["Rows"])),
    );
    nodes.insert(
        704,
        Node::SourceField {
            path: path(&["GlobalReject"]),
            frame: None,
        },
    );
    nodes.insert(
        180,
        field(
            SourceOwner::Primary,
            &["Rows", "Details", "Code"],
            Some(&["Rows"]),
        ),
    );
    nodes.insert(
        181,
        field(SourceOwner::Primary, &["Rows", "Label"], Some(&[])),
    );
    Graph { nodes }
}
pub fn base() -> Project {
    Project {
        source: endpoint(),
        target: output(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: ["Rows", EXACT, "RequiredLegacy", "Unused"]
            .into_iter()
            .map(|name| NamedSource {
                name: name.into(),
                path: format!("unused/{name}.json"),
                schema: endpoint(),
                options: Default::default(),
                dynamic_path: None,
            })
            .collect(),
        extra_targets: vec![
            NamedTarget {
                name: AUDIT.into(),
                path: None,
                schema: output(),
                options: Default::default(),
                root: Scope {
                    children: vec![grouped(200)],
                    ..Default::default()
                },
            },
            NamedTarget {
                name: "legacy".into(),
                path: None,
                schema: SchemaNode::group("Legacy", vec![string("Marker")]),
                options: Default::default(),
                root: Scope {
                    bindings: bindings(&[("Marker", 606)]),
                    ..Default::default()
                },
            },
        ],
        graph: graph(),
        root: Scope {
            children: vec![grouped(100)],
            ..Default::default()
        },
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}
fn legacy_group(key: u32, post: Option<u32>) -> Scope {
    Scope {
        target_field: "Groups".into(),
        iteration: ScopeIteration::Source(path(&["Rows"])),
        group_by: Some(key),
        post_group_filter: post,
        bindings: bindings(&[("First", 601), ("GroupPosition", 602), ("RootMarker", 603)]),
        children: vec![Scope {
            target_field: "Members".into(),
            iteration: ScopeIteration::Source(vec![]),
            bindings: bindings(&[("Label", 601), ("MemberPosition", 602), ("RootMarker", 603)]),
            ..Default::default()
        }],
        ..Default::default()
    }
}
fn nested(project: &mut Project, outer: u32, inner: u32, regroup: bool) {
    let mut outside = grouped(outer);
    let mut inside = grouped(inner);
    inside.target_field = "Nested".into();
    if !regroup {
        inside.bindings.extend(bindings(&[
            ("OuterFirst", outer + 2),
            ("OuterPosition", outer + 5),
        ]));
        inside.children[0].bindings.extend(bindings(&[
            ("OuterFirst", outer + 2),
            ("OuterPosition", outer + 5),
        ]));
    }
    outside.children.push(inside);
    project.root.children = vec![outside];
    let mut inner_schema = groups_schema();
    inner_schema.name = "Nested".into();
    if !regroup {
        let ir::SchemaKind::Group { children, .. } = &mut inner_schema.kind else {
            unreachable!()
        };
        children.splice(3..3, [string("OuterFirst"), int("OuterPosition")]);
        let members = children
            .iter_mut()
            .find(|field| field.name == "Members")
            .unwrap();
        let ir::SchemaKind::Group { children, .. } = &mut members.kind else {
            unreachable!()
        };
        children.extend([string("OuterFirst"), int("OuterPosition")]);
    }
    let mut outer_schema = groups_schema();
    let ir::SchemaKind::Group { children, .. } = &mut outer_schema.kind else {
        unreachable!()
    };
    children.push(inner_schema);
    project.target = SchemaNode::group("Output", vec![outer_schema]);
}
fn batches(project: &mut Project, per_parent: bool) {
    let mut groups = Scope {
        target_field: "Groups".into(),
        iteration: iteration(100, &["Batch", "Rows"]),
        group_by: Some(401),
        bindings: bindings(&[
            ("First", 400),
            ("GroupPosition", 403),
            ("RootMarker", 106),
            ("Prefix", 404),
            ("PrefixPosition", 405),
        ]),
        children: vec![Scope {
            target_field: "Members".into(),
            iteration: iteration(100, &["Batch", "Rows"]),
            bindings: bindings(&[
                ("Label", 400),
                ("MemberPosition", 403),
                ("RootMarker", 106),
                ("Prefix", 404),
                ("PrefixPosition", 405),
            ]),
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut group_schema = groups_schema();
    let ir::SchemaKind::Group { children, .. } = &mut group_schema.kind else {
        unreachable!()
    };
    // Binding fields precede child fields in both the schema and typed oracle.
    children.splice(3..3, [string("Prefix"), int("PrefixPosition")]);
    let members = children
        .iter_mut()
        .find(|field| field.name == "Members")
        .unwrap();
    let ir::SchemaKind::Group { children, .. } = &mut members.kind else {
        unreachable!()
    };
    children.extend([string("Prefix"), int("PrefixPosition")]);
    if per_parent {
        groups.windows = vec![SequenceWindow::First { count: 406 }];
        project.root.children = vec![Scope {
            target_field: "Batches".into(),
            iteration: iteration(100, &["Batch"]),
            bindings: bindings(&[("Parent", 404), ("ParentPosition", 405)]),
            children: vec![groups],
            ..Default::default()
        }];
        project.target = SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Batches",
                    vec![string("Parent"), int("ParentPosition"), group_schema],
                )
                .repeating(),
            ],
        );
    } else {
        project.root.children = vec![groups];
        project.target = SchemaNode::group("Output", vec![group_schema]);
    }
}
fn roots(profile: &str) -> Project {
    let scalar = profile == "root-repeated-scalar";
    let repeated = profile != "root-singleton";
    let mut project = base();
    project.extra_sources.clear();
    project.extra_targets.clear();
    project.source = if scalar {
        mixed("Input").repeating()
    } else {
        let schema = SchemaNode::group(
            "Input",
            vec![mixed("Key"), string("Label"), string("Marker")],
        );
        if repeated { schema.repeating() } else { schema }
    };
    project.graph = Graph {
        nodes: BTreeMap::from([
            (
                1,
                field(
                    SourceOwner::Primary,
                    if scalar { &[] } else { &["Key"] },
                    None,
                ),
            ),
            (
                2,
                field(
                    SourceOwner::Primary,
                    if scalar { &[] } else { &["Label"] },
                    None,
                ),
            ),
            (3, position(SourceOwner::Primary, &[])),
        ]),
    };
    if scalar {
        project.target = mixed("Output").repeating();
        project.root = Scope {
            iteration: iteration(100, &[]),
            group_by: Some(1),
            construction: mapping::ScopeConstruction::Scalar { value: 2 },
            ..Default::default()
        };
    } else {
        let mut fields = vec![string("First"), int("GroupPosition")];
        let mut outer = bindings(&[("First", 2), ("GroupPosition", 3)]);
        if !repeated {
            project
                .graph
                .nodes
                .insert(4, field(SourceOwner::Primary, &["Marker"], Some(&[])));
            fields.push(string("RootMarker"));
            outer.extend(bindings(&[("RootMarker", 4)]));
        }
        fields.push(
            SchemaNode::group("Members", vec![string("Label"), int("MemberPosition")]).repeating(),
        );
        project.target = SchemaNode::group("Output", fields).repeating();
        project.root = Scope {
            iteration: iteration(100, &[]),
            group_by: Some(1),
            bindings: outer,
            children: vec![Scope {
                target_field: "Members".into(),
                iteration: ScopeIteration::Source(vec![]),
                bindings: bindings(&[("Label", 2), ("MemberPosition", 3)]),
                ..Default::default()
            }],
            ..Default::default()
        };
    }
    project
}
fn scalar_values() -> Project {
    let mut project = base();
    project.extra_sources.clear();
    project.extra_targets.clear();
    project.source = SchemaNode::group("Input", vec![mixed("Values").repeating()]);
    project.graph = Graph {
        nodes: BTreeMap::from([
            (1, field(SourceOwner::Primary, &["Values"], None)),
            (2, position(SourceOwner::Primary, &["Values"])),
        ]),
    };
    project.target = SchemaNode::group(
        "Output",
        vec![
            mixed("Representatives").repeating(),
            SchemaNode::group(
                "Groups",
                vec![int("GroupPosition"), mixed("Members").repeating()],
            )
            .repeating(),
        ],
    );
    project.root = Scope {
        children: vec![
            Scope {
                target_field: "Representatives".into(),
                iteration: iteration(100, &["Values"]),
                group_by: Some(1),
                construction: mapping::ScopeConstruction::Scalar { value: 1 },
                ..Default::default()
            },
            Scope {
                target_field: "Groups".into(),
                iteration: iteration(100, &["Values"]),
                group_by: Some(1),
                bindings: bindings(&[("GroupPosition", 2)]),
                children: vec![Scope {
                    target_field: "Members".into(),
                    iteration: iteration(100, &["Values"]),
                    construction: mapping::ScopeConstruction::Scalar { value: 1 },
                    ..Default::default()
                }],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    project
}
fn alias_root(pinned: bool) -> Project {
    let alias = SourceOwner::Named {
        name: "Alias".into(),
    };
    let mut project = base();
    project.source = SchemaNode::group("Input", vec![string("Marker")]);
    project.graph = Graph {
        nodes: BTreeMap::from([
            (1, field(alias.clone(), &["Key"], None)),
            (2, field(alias.clone(), &["Label"], None)),
            (3, position(alias.clone(), &[])),
            (4, field(alias.clone(), &["Key"], Some(&[]))),
        ]),
    };
    project.extra_sources = vec![NamedSource {
        name: "Alias".into(),
        path: "unused/Alias.json".into(),
        schema: SchemaNode::group("AliasRows", vec![string("Key"), string("Label")]).repeating(),
        options: Default::default(),
        dynamic_path: None,
    }];
    let schema = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Groups",
                vec![
                    string("First"),
                    int("GroupPosition"),
                    SchemaNode::group("Members", vec![string("Label"), int("MemberPosition")])
                        .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    let members = Scope {
        target_field: "Members".into(),
        iteration: ScopeIteration::Source(path(&["Alias"])),
        bindings: bindings(&[("Label", 2), ("MemberPosition", 3)]),
        ..Default::default()
    };
    let grouped = Scope {
        target_field: "Groups".into(),
        iteration: ScopeIteration::Source(path(&["Alias"])),
        group_by: Some(1),
        bindings: bindings(&[("First", if pinned { 4 } else { 2 }), ("GroupPosition", 3)]),
        children: vec![members],
        ..Default::default()
    };
    project.root = Scope {
        children: vec![grouped.clone()],
        ..Default::default()
    };
    project.target = schema.clone();
    let mut owned_members = grouped;
    owned_members.bindings[0].node = 2;
    owned_members.children[0].iteration = ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner: alias,
            path: vec![],
        },
        output_path: None,
    };
    project.extra_targets = vec![NamedTarget {
        name: AUDIT.into(),
        path: None,
        schema,
        options: Default::default(),
        root: Scope {
            children: vec![owned_members],
            ..Default::default()
        },
    }];
    project
}
pub fn project(profile: &str) -> Project {
    if profile.starts_with("root-") {
        return roots(profile);
    }
    if profile == "scalar-values" {
        return scalar_values();
    }
    if profile == "legacy-named-repeated-root" || profile == "legacy-named-repeated-root-pin" {
        return alias_root(profile.ends_with("-pin"));
    }
    let mut project = base();
    let group = &mut project.root.children[0];
    match profile {
        "basic-primary" | "tagged-keys" => {}
        "basic-named" => *group = grouped(200),
        "basic-unicode" => *group = grouped(300),
        "legacy-members" => group.children[0] = member(100, true),
        "post-primary" => group.post_group_filter = Some(103),
        "post-named" => {
            *group = grouped(200);
            group.post_group_filter = Some(203);
        }
        "sorted-post" | "sorted-post-last" => {
            group.sort_by = Some(104);
            group.post_group_filter = Some(131);
            if profile.ends_with("last") {
                group.windows = vec![SequenceWindow::Last { count: 107 }];
            }
        }
        "filter-before-sort" | "sort-before-filter" => {
            group.filter = Some(130);
            group.sort_by = Some(104);
            group.post_group_filter = Some(103);
            group.sort_filter_order = if profile == "filter-before-sort" {
                SortFilterOrder::FilterThenSort
            } else {
                SortFilterOrder::SortThenFilter
            };
        }
        "sorted-position-key" => {
            group.sort_by = Some(104);
            group.group_by = Some(133);
        }
        "raw-gap-filter" => group.filter = Some(131),
        "legacy-key-only" => *group = legacy_group(101, None),
        "legacy-post-only" => *group = legacy_group(604, Some(103)),
        "legacy-island" => {
            let mut legacy = legacy_group(604, None);
            legacy.target_field = "LegacyGroups".into();
            project.root.children.push(legacy);
            let mut schema = groups_schema();
            schema.name = "LegacyGroups".into();
            project.target = SchemaNode::group("Output", vec![groups_schema(), schema]);
        }
        "nested-P-N" => nested(&mut project, 100, 200, false),
        "nested-N-P" => nested(&mut project, 200, 100, false),
        "regroup-P" => nested(&mut project, 100, 100, true),
        "parent-row-N" => {
            nested(&mut project, 100, 200, false);
            project.root.children[0].group_by = None;
        }
        "batch-per-parent" => batches(&mut project, true),
        "batch-flat" => batches(&mut project, false),
        "phase" | "phase-prefilter" | "phase-sorted" | "phase-first-zero" | "phase-bad-bound"
        | "phase-bound-raise" | "phase-target" | "phase-post-string" => {
            group.filter = Some(122);
            group.group_by = Some(120);
            group.post_group_filter = Some(121);
            group.bindings[0].node = 124;
            if profile == "phase-prefilter" {
                group.sort_by = Some(123);
                group.sort_filter_order = SortFilterOrder::FilterThenSort;
            }
            if profile == "phase-sorted" {
                group.sort_by = Some(123);
                group.windows = vec![SequenceWindow::First { count: 505 }];
            }
            if profile == "phase-first-zero" {
                group.windows = vec![SequenceWindow::First { count: 902 }];
            }
            if profile == "phase-bad-bound" {
                group.windows = vec![SequenceWindow::First { count: 907 }];
            }
            if profile == "phase-bound-raise" {
                group.windows = vec![SequenceWindow::First { count: 505 }];
            }
            if profile == "phase-post-string" {
                group.post_group_filter = Some(906);
            }
        }
        "filter-false-skips-key-post" => {
            group.filter = Some(901);
            group.group_by = Some(501);
            group.post_group_filter = Some(502);
        }
        "desc-repetition" | "desc-traversal" => group.bindings[0].node = 180,
        "inactive-other-position" => group.bindings[1].node = 205,
        "inactive-other-pin" => group.bindings[0].node = 202,
        "primary-root-pin" => group.bindings[0].node = 181,
        "failures" => {
            group.group_by = Some(120);
            project.failure_rules = vec![
                mapping::FailureRule {
                    iteration: mapping::FailureIteration::Source { collection: vec![] },
                    selection: mapping::FailureSelection::WhenTrue { predicate: 704 },
                    message: Some(702),
                },
                mapping::FailureRule {
                    iteration: mapping::FailureIteration::Source {
                        collection: path(&["Rows"]),
                    },
                    selection: mapping::FailureSelection::WhenTrue { predicate: 703 },
                    message: Some(102),
                },
            ];
        }
        "implicit-first" => {
            group.iteration_output = mapping::IterationOutput::First;
            let mut schema = groups_schema();
            schema.repeating = false;
            project.target = SchemaNode::group("Output", vec![schema]);
        }
        "ordered-windows" => {
            group.windows = vec![
                SequenceWindow::SkipFirst { count: 903 },
                SequenceWindow::Last { count: 903 },
                SequenceWindow::First { count: 904 },
            ]
        }
        _ => panic!("unknown authored profile {profile}"),
    }
    project
}
