//! Public Project definitions for owned `group_ending_with` parity. Each profile
//! is one small, named change to the shared base.
use super::*;
use mapping::{
    NamedSource, NamedTarget, OwnedPosition, OwnedSourceField, OwnedSourceRef, SequenceWindow as W,
    SourceOwner,
};

pub const EXACT: &str = " Røws💠 ";
pub const AUDIT: &str = "audit";
pub const UNICODE_TARGET: &str = "unicode";

pub const PROFILES: &[&str] = &[
    "E01-base",
    "E02-all-false",
    "E03-all-true",
    "E04-post",
    "E05-filter",
    "E06-sort",
    "E07-parent-window",
    "E08-last",
    "E09-from-to",
    "E10-position-boundary",
    "E11-first",
    "E12-nested-owners",
    "E13-nested-members",
    "E14-boundary-raise",
    "E15-boundary-not-bool",
    "E16-post-raise",
    "E17-last-boundary",
    "E18-bound-before-boundary",
    "E19-unselected-targets",
];

pub fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).into()).collect()
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
        .map(|(name, id)| Binding {
            target_field: (*name).into(),
            node: *id,
        })
        .collect()
}
fn string(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}
fn int(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::Int)
}
fn bool_(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::Bool)
}
pub fn row_schema() -> SchemaNode {
    SchemaNode::group(
        "Rows",
        vec![
            bool_("End"),
            string("Label"),
            bool_("Keep"),
            int("Rank"),
            int("Limit"),
            string("Key"),
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
/// One Ending scope over `Rows` of `name`. Node ids are `offset` + 1 End,
/// 2 Label, 3 Keep, 4 Rank, 5 position, 6 root Marker, 7 root Limit.
pub fn grouped(name: &str, offset: u32) -> Scope {
    Scope {
        target_field: "Groups".into(),
        iteration: iteration(name, &["Rows"]),
        group_ending_with: Some(offset + 1),
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
fn raise(p: &mut Project, id: u32, message: &str) {
    constant(p, id + 1000, Value::String(message.into()));
    p.graph.nodes.insert(
        id,
        Node::Raise {
            message: Some(id + 1000),
        },
    );
}

/// Primary groups Primary rows; the named targets group the named endpoints
/// `Rows` and the exact Unicode owner with the same collection and boundary.
pub fn base() -> Project {
    let mut nodes = BTreeMap::new();
    for (name, offset) in [("Primary", 0), ("Rows", 200), (EXACT, 300)] {
        for (id, leaf) in [(1, "End"), (2, "Label"), (3, "Keep"), (4, "Rank")] {
            nodes.insert(offset + id, field(name, &["Rows", leaf], Some(&["Rows"])));
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
        extra_targets: [(AUDIT, "Rows", 200), (UNICODE_TARGET, EXACT, 300)]
            .into_iter()
            .map(|(name, own, offset)| NamedTarget {
                name: name.into(),
                path: None,
                schema: SchemaNode::group("Output", vec![groups_schema()]),
                options: Default::default(),
                root: Scope {
                    children: vec![grouped(own, offset)],
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
        (912, Value::Int(2)),
        (913, Value::Int(3)),
        (917, Value::Int(7)),
        (920, Value::String("A".into())),
    ] {
        constant(&mut p, id, value);
    }
    p
}

/// Outer Ending groups of `outer` containing an inner Ending scope.
fn nested(p: &mut Project, outer_name: &str, outer: u32, inner: Scope, window: Option<W>) {
    let mut scope = grouped(outer_name, outer);
    scope.target_field = "Outer".into();
    scope.windows = window.into_iter().collect();
    scope.bindings = bindings(&[("OuterFirst", outer + 2), ("OuterPosition", outer + 5)]);
    scope.children = vec![Scope {
        target_field: "Inner".into(),
        children: vec![inner],
        ..Default::default()
    }];
    p.root.children = vec![scope];
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Outer",
                vec![
                    string("OuterFirst"),
                    int("OuterPosition"),
                    SchemaNode::group("Inner", vec![groups_schema()]),
                ],
            )
            .repeating(),
        ],
    );
}

pub fn project(profile: &str) -> Project {
    assert!(
        PROFILES.contains(&profile),
        "unknown authored profile {profile}"
    );
    let mut p = base();
    let g = &mut p.root.children[0];
    match profile {
        "E01-base" => {}
        "E02-all-false" => g.group_ending_with = Some(902),
        "E03-all-true" => g.group_ending_with = Some(901),
        "E04-post" => g.post_group_filter = Some(3),
        "E05-filter" => {
            // Select only rows whose own boundary is false.
            g.filter = Some(30);
            p.graph.nodes.insert(
                30,
                Node::If {
                    condition: 1,
                    then: 902,
                    else_: 901,
                },
            );
        }
        "E06-sort" => g.sort_by = Some(4),
        // Bound read once from the receiving parent: P.root Limit is 2.
        "E07-parent-window" => g.windows = vec![W::First { count: 7 }],
        "E08-last" => g.windows = vec![W::Last { count: 912 }],
        "E09-from-to" => {
            g.windows = vec![W::FromTo {
                first: 912,
                last: 913,
            }]
        }
        "E10-position-boundary" => {
            g.group_ending_with = Some(31);
            call(&mut p, 31, "equal", &[5, 912]);
        }
        "E11-first" => {
            g.iteration_output = mapping::IterationOutput::First;
            let ir::SchemaKind::Group { children, .. } = &mut p.target.kind else {
                unreachable!("authored group")
            };
            children[0].repeating = false;
        }
        "E12-nested-owners" => nested(
            &mut p,
            "Primary",
            0,
            grouped("Rows", 200),
            Some(W::First { count: 912 }),
        ),
        "E13-nested-members" => {
            let mut inner = grouped("Primary", 0);
            inner.group_ending_with = Some(3);
            nested(&mut p, "Primary", 0, inner, None);
        }
        "E14-boundary-raise" => {
            g.group_ending_with = Some(101);
            raise(&mut p, 101, "boundary");
        }
        "E15-boundary-not-bool" => g.group_ending_with = Some(920),
        "E16-post-raise" => {
            g.group_ending_with = Some(902);
            g.post_group_filter = Some(105);
            raise(&mut p, 105, "post");
        }
        "E17-last-boundary" => {
            // Only the seventh candidate raises; its boundary cannot change
            // the partition but is still evaluated.
            g.group_ending_with = Some(33);
            call(&mut p, 32, "equal", &[5, 917]);
            p.graph.nodes.insert(
                33,
                Node::If {
                    condition: 32,
                    then: 101,
                    else_: 1,
                },
            );
            raise(&mut p, 101, "boundary");
        }
        "E18-bound-before-boundary" => {
            g.group_ending_with = Some(101);
            g.windows = vec![W::First { count: 104 }];
            raise(&mut p, 101, "boundary");
            raise(&mut p, 104, "bound");
        }
        "E19-unselected-targets" => {
            g.windows = vec![W::First { count: 910 }];
            g.bindings[0].node = 106;
            raise(&mut p, 106, "target");
        }
        _ => unreachable!("closed profile list"),
    }
    p
}
