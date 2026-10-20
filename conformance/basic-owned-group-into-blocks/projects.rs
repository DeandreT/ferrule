//! Public Project definitions for owned `group_into_blocks` parity. Each profile
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
    "B01-base",
    "B02-size-three",
    "B03-size-one",
    "B04-size-whole",
    "B05-size-float",
    "B06-post",
    "B07-filter",
    "B08-sort",
    "B09-parent-window",
    "B10-last",
    "B11-from-to",
    "B12-position-filter",
    "B13-first",
    "B14-nested-owners",
    "B15-nested-members",
    "B16-size-zero",
    "B17-size-negative",
    "B18-size-null",
    "B19-size-string",
    "B20-size-raise",
    "B21-bound-before-size",
    "B22-size-before-filter",
    "B23-post-raise",
    "B24-item-size",
    "B25-unselected-targets",
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
/// One IntoBlocks scope over `Rows` of `name`, sized by the owner's root
/// `Limit` in the receiving parent. Node ids are `offset` + 2 Label, 3 Keep,
/// 4 Rank, 5 position, 6 root Marker, 7 root Limit.
pub fn grouped(name: &str, offset: u32) -> Scope {
    Scope {
        target_field: "Groups".into(),
        iteration: iteration(name, &["Rows"]),
        group_into_blocks: Some(offset + 7),
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
/// `Rows` and the exact Unicode owner with the same collection and size path.
pub fn base() -> Project {
    let mut nodes = BTreeMap::new();
    for (name, offset) in [("Primary", 0), ("Rows", 200), (EXACT, 300)] {
        for (id, leaf) in [(2, "Label"), (3, "Keep"), (4, "Rank")] {
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
        (911, Value::Int(1)),
        (912, Value::Int(2)),
        (913, Value::Int(3)),
        (914, Value::Int(-1)),
        (915, Value::Null),
        (916, Value::String("2.5".into())),
        (917, Value::Int(7)),
        (918, Value::Float(2.9)),
    ] {
        constant(&mut p, id, value);
    }
    p
}

/// Outer blocks of `outer` containing an inner blocks scope.
fn nested(p: &mut Project, outer_name: &str, outer: u32, inner: Scope, window: Option<W>) {
    nested_with(p, grouped(outer_name, outer), inner, window);
}

/// `outer` is a Primary (offset 0) blocks scope wrapping `inner`.
fn nested_with(p: &mut Project, mut scope: Scope, inner: Scope, window: Option<W>) {
    scope.target_field = "Outer".into();
    scope.windows = window.into_iter().collect();
    scope.bindings = bindings(&[("OuterFirst", 2), ("OuterPosition", 5)]);
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
        "B01-base" => {}
        "B02-size-three" => g.group_into_blocks = Some(913),
        "B03-size-one" => g.group_into_blocks = Some(911),
        "B04-size-whole" => g.group_into_blocks = Some(917),
        "B05-size-float" => g.group_into_blocks = Some(918),
        "B06-post" => g.post_group_filter = Some(3),
        "B07-filter" => g.filter = Some(3),
        "B08-sort" => g.sort_by = Some(4),
        // Bound read once from the receiving parent: P.root Limit is 2.
        "B09-parent-window" => g.windows = vec![W::First { count: 7 }],
        "B10-last" => g.windows = vec![W::Last { count: 912 }],
        "B11-from-to" => {
            g.windows = vec![W::FromTo {
                first: 912,
                last: 913,
            }]
        }
        // A per-candidate filter sees candidate positions.
        "B12-position-filter" => {
            g.filter = Some(31);
            call(&mut p, 31, "greater_than", &[5, 911]);
        }
        "B13-first" => {
            g.iteration_output = mapping::IterationOutput::First;
            let ir::SchemaKind::Group { children, .. } = &mut p.target.kind else {
                unreachable!("authored group")
            };
            children[0].repeating = false;
        }
        "B14-nested-owners" => nested(
            &mut p,
            "Primary",
            0,
            grouped("Rows", 200),
            Some(W::First { count: 912 }),
        ),
        "B15-nested-members" => {
            let mut inner = grouped("Primary", 0);
            inner.group_into_blocks = Some(912);
            let mut outer = grouped("Primary", 0);
            outer.group_into_blocks = Some(913);
            nested_with(&mut p, outer, inner, None);
        }
        "B16-size-zero" => g.group_into_blocks = Some(910),
        "B17-size-negative" => g.group_into_blocks = Some(914),
        "B18-size-null" => g.group_into_blocks = Some(915),
        "B19-size-string" => g.group_into_blocks = Some(916),
        "B20-size-raise" => {
            g.group_into_blocks = Some(103);
            raise(&mut p, 103, "size");
        }
        "B21-bound-before-size" => {
            g.group_into_blocks = Some(103);
            g.windows = vec![W::First { count: 104 }];
            raise(&mut p, 103, "size");
            raise(&mut p, 104, "bound");
        }
        "B22-size-before-filter" => {
            g.group_into_blocks = Some(910);
            g.filter = Some(102);
            raise(&mut p, 102, "filter");
        }
        "B23-post-raise" => {
            g.post_group_filter = Some(105);
            raise(&mut p, 105, "post");
        }
        // The size is a parent expression: a row-pinned field has no active row.
        "B24-item-size" => g.group_into_blocks = Some(4),
        "B25-unselected-targets" => {
            g.windows = vec![W::First { count: 910 }];
            g.bindings[0].node = 106;
            raise(&mut p, 106, "target");
        }
        _ => unreachable!("closed profile list"),
    }
    p
}
