//! Genuine public Project definitions for owned SequenceExists. Authored
//! source only: no producer, predicate or consumer is executed here.
use super::*;
use mapping::{
    NamedSource, NamedTarget, OwnedPosition, OwnedSourceField, OwnedSourceRef, SequenceExpr,
    SourceOwner,
};

pub const NAMED: &str = "Cfg";
pub const EXACT: &str = " Cfg💠 ";
pub const UNUSED: &str = "unusedStatic";
pub const AUDIT: &str = "audit";
pub const UNICODE_TARGET: &str = "unicode";

pub const PROFILES: &[&str] = &[
    "S01-colliding",
    "S02-producer-owners",
    "S03-generate",
    "S04-by-length",
    "S05-regex",
    "S06-first-true",
    "S07-empty",
    "N01-rows",
    "N02-nested-owners",
    "G01-group-by",
    "G02-starting",
    "G03-ending",
    "F01-filter",
    "I01-item-at",
    "I02-nested-exists",
    "L01-legacy-island",
    "E01-later-error",
    "E02-producer-error",
    "E03-producer-type",
    "E04-not-bool",
    "E05-missing-owner",
    "E06-item-predicate",
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
fn position(name: &str) -> Node {
    Node::OwnedPosition(OwnedPosition {
        owner: owner(name),
        collection: Some(path(&["Rows"])),
    })
}
pub fn iteration(name: &str) -> ScopeIteration {
    ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner: owner(name),
            path: path(&["Rows"]),
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
fn boolean(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::Bool)
}
fn string(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}
fn int(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::Int)
}
fn output(fields: Vec<SchemaNode>) -> SchemaNode {
    SchemaNode::group("Output", fields)
}
pub fn endpoint() -> SchemaNode {
    SchemaNode::group(
        "Endpoint",
        vec![
            string("Marker"),
            string("Code"),
            string("List"),
            int("Count"),
            SchemaNode::group(
                "Rows",
                vec![string("Code"), string("Label"), boolean("Keep")],
            )
            .repeating(),
        ],
    )
}
pub fn constant(p: &mut Project, id: u32, value: Value) {
    p.graph.nodes.insert(id, Node::Const { value });
}
pub fn call(p: &mut Project, id: u32, function: &str, args: &[u32]) {
    p.graph.nodes.insert(
        id,
        Node::Call {
            function: function.into(),
            args: args.to_vec(),
        },
    );
}
pub fn raise(p: &mut Project, id: u32, message: &str) {
    constant(p, id + 1000, Value::String(message.into()));
    p.graph.nodes.insert(
        id,
        Node::Raise {
            message: Some(id + 1000),
        },
    );
}
pub fn iff(p: &mut Project, id: u32, condition: u32, then: u32, else_: u32) {
    p.graph.nodes.insert(
        id,
        Node::If {
            condition,
            then,
            else_,
        },
    );
}
pub fn tokenize(input: u32, item: u32) -> SequenceExpr {
    SequenceExpr::Tokenize {
        input,
        delimiter: 900,
        item,
    }
}
/// Inserts the consumer and its private item declaration.
pub fn exists(p: &mut Project, id: u32, sequence: SequenceExpr, predicate: u32) {
    p.graph.nodes.insert(
        sequence.item(),
        Node::SourceField {
            path: vec![],
            frame: None,
        },
    );
    p.graph.nodes.insert(
        id,
        Node::SequenceExists {
            sequence,
            predicate,
        },
    );
}

/// Owned reads for each endpoint use one ID block: Primary 0, Cfg 100,
/// exact Unicode 200 and the never-supplied static declaration 300.
pub fn base() -> Project {
    let mut p = Project {
        source: endpoint(),
        target: output(vec![boolean("Found")]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: [NAMED, EXACT, UNUSED]
            .into_iter()
            .map(|name| NamedSource {
                name: name.into(),
                path: format!("unused/{}.json", name.trim()),
                schema: endpoint(),
                options: Default::default(),
                dynamic_path: None,
            })
            .collect(),
        extra_targets: vec![
            NamedTarget {
                name: AUDIT.into(),
                path: None,
                schema: output(vec![boolean("Found"), boolean("Primary")]),
                options: Default::default(),
                root: Scope {
                    bindings: bindings(&[("Found", 600), ("Primary", 601)]),
                    ..Default::default()
                },
            },
            NamedTarget {
                name: UNICODE_TARGET.into(),
                path: None,
                schema: output(vec![boolean("Found"), boolean("Cross")]),
                options: Default::default(),
                root: Scope {
                    bindings: bindings(&[("Found", 602), ("Cross", 603)]),
                    ..Default::default()
                },
            },
        ],
        graph: Graph::default(),
        root: Scope::default(),
        failure_rules: vec![],
        user_functions: Default::default(),
    };
    for (name, offset) in [("Primary", 0), (NAMED, 100), (EXACT, 200), (UNUSED, 300)] {
        for (id, leaf) in [(1, "Code"), (2, "List"), (3, "Count"), (8, "Marker")] {
            p.graph
                .nodes
                .insert(offset + id, field(name, &[leaf], Some(&[])));
        }
        for (id, leaf) in [(4, "Code"), (5, "Label"), (6, "Keep")] {
            p.graph
                .nodes
                .insert(offset + id, field(name, &["Rows", leaf], Some(&["Rows"])));
        }
        p.graph.nodes.insert(offset + 7, position(name));
    }
    for (id, value) in [
        (900, Value::String("|".into())),
        (901, Value::Bool(true)),
        (902, Value::Bool(false)),
        (903, Value::Null),
        (904, Value::String("x|M|U".into())),
        (905, Value::String("boom".into())),
        (906, Value::String("M|Z".into())),
        (907, Value::xml_nil()),
        (908, Value::String("Q|M|M".into())),
        (909, Value::String("Z".into())),
        (910, Value::Int(2)),
        (913, Value::String("\\|".into())),
        (914, Value::String("[|]".into())),
        (915, Value::String("i".into())),
        (916, Value::String("Q|M".into())),
        (917, Value::String("stop|go".into())),
        (918, Value::String("go".into())),
        (919, Value::String("".into())),
        (920, Value::String("x|boom|M".into())),
        (921, Value::String("x|M|boom".into())),
    ] {
        constant(&mut p, id, value);
    }
    // Fixed named targets: exact named owners beside a colliding Primary code.
    call(&mut p, 610, "equal", &[96, 101]);
    exists(&mut p, 600, tokenize(102, 96), 610);
    call(&mut p, 611, "equal", &[97, 1]);
    exists(&mut p, 601, tokenize(102, 97), 611);
    call(&mut p, 612, "equal", &[98, 201]);
    exists(&mut p, 602, tokenize(202, 98), 612);
    call(&mut p, 613, "equal", &[99, 101]);
    exists(&mut p, 603, tokenize(202, 99), 613);
    p
}

fn rows_scope(field: &str, name: &str) -> Scope {
    Scope {
        target_field: field.into(),
        iteration: iteration(name),
        ..Default::default()
    }
}

pub fn project(profile: &str) -> Project {
    assert!(PROFILES.contains(&profile), "unknown profile {profile}");
    let mut p = base();
    let root = |fields: &[(&str, u32)]| Scope {
        bindings: bindings(fields),
        ..Default::default()
    };
    match profile {
        "S01-colliding" => {
            call(&mut p, 20, "equal", &[90, 1]);
            exists(&mut p, 10, tokenize(904, 90), 20);
            call(&mut p, 21, "equal", &[91, 101]);
            exists(&mut p, 11, tokenize(904, 91), 21);
            call(&mut p, 22, "equal", &[92, 201]);
            exists(&mut p, 12, tokenize(904, 92), 22);
            p.graph.nodes.insert(
                24,
                Node::SourceField {
                    path: path(&["Code"]),
                    frame: None,
                },
            );
            call(&mut p, 23, "equal", &[93, 24]);
            exists(&mut p, 13, tokenize(904, 93), 23);
            p.target = output(vec![
                boolean("Primary"),
                boolean("Named"),
                boolean("Unicode"),
                boolean("Legacy"),
            ]);
            p.root = root(&[
                ("Primary", 10),
                ("Named", 11),
                ("Unicode", 12),
                ("Legacy", 13),
            ]);
        }
        "S02-producer-owners" => {
            call(&mut p, 20, "equal", &[90, 101]);
            exists(&mut p, 10, tokenize(102, 90), 20);
            call(&mut p, 21, "equal", &[91, 101]);
            exists(&mut p, 11, tokenize(202, 91), 21);
            p.target = output(vec![boolean("Named"), boolean("Unicode")]);
            p.root = root(&[("Named", 10), ("Unicode", 11)]);
        }
        "S03-generate" => {
            call(&mut p, 20, "equal", &[90, 3]);
            exists(
                &mut p,
                10,
                SequenceExpr::Generate {
                    from: None,
                    to: 103,
                    item: 90,
                },
                20,
            );
            raise(&mut p, 21, "predicate");
            exists(
                &mut p,
                11,
                SequenceExpr::Generate {
                    from: Some(103),
                    to: 203,
                    item: 91,
                },
                21,
            );
            p.target = output(vec![boolean("Omitted"), boolean("Empty")]);
            p.root = root(&[("Omitted", 10), ("Empty", 11)]);
        }
        "S04-by-length" => {
            call(&mut p, 20, "equal", &[90, 101]);
            exists(
                &mut p,
                10,
                SequenceExpr::TokenizeByLength {
                    input: 2,
                    length: 910,
                    item: 90,
                },
                20,
            );
            p.root = root(&[("Found", 10)]);
        }
        "S05-regex" => {
            call(&mut p, 20, "equal", &[90, 1]);
            exists(
                &mut p,
                10,
                SequenceExpr::TokenizeRegex {
                    input: 102,
                    pattern: 913,
                    flags: None,
                    item: 90,
                },
                20,
            );
            call(&mut p, 21, "equal", &[91, 101]);
            exists(
                &mut p,
                11,
                SequenceExpr::TokenizeRegex {
                    input: 102,
                    pattern: 914,
                    flags: Some(915),
                    item: 91,
                },
                21,
            );
            p.target = output(vec![boolean("NoFlags"), boolean("Flags")]);
            p.root = root(&[("NoFlags", 10), ("Flags", 11)]);
        }
        "S06-first-true" | "E01-later-error" => {
            // equal(item, Cfg.Code) ? true : (item == "boom" ? raise : false)
            call(&mut p, 21, "equal", &[90, 101]);
            call(&mut p, 27, "equal", &[90, 905]);
            raise(&mut p, 50, "late");
            iff(&mut p, 26, 27, 50, 902);
            iff(&mut p, 25, 21, 901, 26);
            let items = if profile == "S06-first-true" {
                921
            } else {
                920
            };
            exists(&mut p, 10, tokenize(items, 90), 25);
            p.root = root(&[("Found", 10)]);
        }
        "S07-empty" => {
            raise(&mut p, 20, "predicate");
            exists(&mut p, 10, tokenize(903, 90), 20);
            call(&mut p, 21, "equal", &[91, 919]);
            exists(&mut p, 11, tokenize(919, 91), 21);
            raise(&mut p, 22, "delimiter");
            raise(&mut p, 23, "suppressed predicate");
            exists(
                &mut p,
                12,
                SequenceExpr::Tokenize {
                    input: 903,
                    delimiter: 22,
                    item: 92,
                },
                23,
            );
            call(&mut p, 24, "equal", &[93, 301]);
            exists(&mut p, 13, tokenize(903, 93), 24);
            p.target = output(vec![
                boolean("NullInput"),
                boolean("EmptyText"),
                boolean("Suppressed"),
                boolean("Unreached"),
            ]);
            p.root = root(&[
                ("NullInput", 10),
                ("EmptyText", 11),
                ("Suppressed", 12),
                ("Unreached", 13),
            ]);
        }
        "N01-rows" => {
            p.graph
                .nodes
                .insert(23, Node::Position { collection: vec![] });
            call(&mut p, 21, "equal", &[90, 4]);
            call(&mut p, 22, "equal", &[23, 7]);
            call(&mut p, 20, "and", &[21, 22]);
            exists(&mut p, 10, tokenize(908, 90), 20);
            call(&mut p, 24, "equal", &[91, 101]);
            exists(&mut p, 11, tokenize(4, 91), 24);
            p.target = output(vec![
                SchemaNode::group(
                    "Rows",
                    vec![
                        string("Label"),
                        int("Position"),
                        boolean("Match"),
                        boolean("NamedHit"),
                    ],
                )
                .repeating(),
            ]);
            let mut rows = rows_scope("Rows", "Primary");
            rows.bindings = bindings(&[
                ("Label", 5),
                ("Position", 7),
                ("Match", 10),
                ("NamedHit", 11),
            ]);
            p.root = Scope {
                children: vec![rows],
                ..Default::default()
            };
        }
        "N02-nested-owners" => {
            call(&mut p, 21, "equal", &[90, 4]);
            call(&mut p, 22, "equal", &[90, 104]);
            call(&mut p, 20, "and", &[21, 22]);
            exists(&mut p, 10, tokenize(916, 90), 20);
            p.target = output(vec![
                SchemaNode::group(
                    "Outer",
                    vec![
                        string("Label"),
                        SchemaNode::group("Inner", vec![string("Label"), boolean("Found")])
                            .repeating(),
                    ],
                )
                .repeating(),
            ]);
            let mut inner = rows_scope("Inner", NAMED);
            inner.bindings = bindings(&[("Label", 105), ("Found", 10)]);
            let mut outer = rows_scope("Outer", "Primary");
            outer.bindings = bindings(&[("Label", 5)]);
            outer.children = vec![inner];
            p.root = Scope {
                children: vec![outer],
                ..Default::default()
            };
        }
        "G01-group-by" => {
            call(&mut p, 20, "equal", &[90, 4]);
            exists(&mut p, 10, tokenize(906, 90), 20);
            call(&mut p, 21, "equal", &[91, 7]);
            exists(
                &mut p,
                11,
                SequenceExpr::Generate {
                    from: Some(910),
                    to: 910,
                    item: 91,
                },
                21,
            );
            p.target = output(vec![
                SchemaNode::group(
                    "Groups",
                    vec![
                        string("First"),
                        boolean("Found"),
                        boolean("Ordinal"),
                        SchemaNode::group(
                            "Members",
                            vec![string("Label"), boolean("Found"), boolean("Ordinal")],
                        )
                        .repeating(),
                    ],
                )
                .repeating(),
            ]);
            let mut members = rows_scope("Members", "Primary");
            members.bindings = bindings(&[("Label", 5), ("Found", 10), ("Ordinal", 11)]);
            let mut groups = rows_scope("Groups", "Primary");
            groups.group_by = Some(6);
            groups.bindings = bindings(&[("First", 5), ("Found", 10), ("Ordinal", 11)]);
            groups.children = vec![members];
            p.root = Scope {
                children: vec![groups],
                ..Default::default()
            };
        }
        "G02-starting" | "G03-ending" => {
            call(&mut p, 20, "equal", &[90, 4]);
            exists(&mut p, 10, tokenize(909, 90), 20);
            p.target = output(vec![
                SchemaNode::group("Groups", vec![string("First")]).repeating(),
            ]);
            let mut groups = rows_scope("Groups", "Primary");
            if profile == "G02-starting" {
                groups.group_starting_with = Some(10);
            } else {
                groups.group_ending_with = Some(10);
            }
            groups.bindings = bindings(&[("First", 5)]);
            p.root = Scope {
                children: vec![groups],
                ..Default::default()
            };
        }
        "F01-filter" => {
            call(&mut p, 20, "equal", &[90, 4]);
            exists(&mut p, 10, tokenize(102, 90), 20);
            p.target = output(vec![
                SchemaNode::group("Rows", vec![string("Label")]).repeating(),
            ]);
            let mut rows = rows_scope("Rows", "Primary");
            rows.filter = Some(10);
            rows.bindings = bindings(&[("Label", 5)]);
            p.root = Scope {
                children: vec![rows],
                ..Default::default()
            };
        }
        "I01-item-at" => {
            p.graph.nodes.insert(
                91,
                Node::SourceField {
                    path: vec![],
                    frame: None,
                },
            );
            p.graph.nodes.insert(
                30,
                Node::SequenceItemAt {
                    sequence: tokenize(102, 91),
                    index: 910,
                },
            );
            call(&mut p, 20, "equal", &[90, 30]);
            exists(&mut p, 10, tokenize(904, 90), 20);
            p.root = root(&[("Found", 10)]);
        }
        "I02-nested-exists" => {
            call(&mut p, 23, "equal", &[91, 101]);
            exists(&mut p, 22, tokenize(102, 91), 23);
            call(&mut p, 21, "equal", &[90, 918]);
            call(&mut p, 20, "and", &[21, 22]);
            exists(&mut p, 10, tokenize(917, 90), 20);
            p.root = root(&[("Found", 10)]);
        }
        "L01-legacy-island" => {
            p.graph.nodes.insert(
                24,
                Node::SourceField {
                    path: path(&["Code"]),
                    frame: Some(path(&["Rows"])),
                },
            );
            call(&mut p, 20, "equal", &[90, 24]);
            exists(&mut p, 10, tokenize(909, 90), 20);
            p.target = output(vec![
                SchemaNode::group("Legacy", vec![boolean("Found")]).repeating(),
            ]);
            p.root = Scope {
                children: vec![Scope {
                    target_field: "Legacy".into(),
                    iteration: ScopeIteration::Source(path(&["Rows"])),
                    bindings: bindings(&[("Found", 10)]),
                    ..Default::default()
                }],
                ..Default::default()
            };
        }
        "E02-producer-error" => {
            raise(&mut p, 30, "producer");
            raise(&mut p, 20, "predicate");
            exists(&mut p, 10, tokenize(30, 90), 20);
            p.root = root(&[("Found", 10)]);
        }
        "E03-producer-type" => {
            raise(&mut p, 20, "predicate");
            exists(&mut p, 10, tokenize(907, 90), 20);
            p.root = root(&[("Found", 10)]);
        }
        "E04-not-bool" => {
            exists(&mut p, 10, tokenize(904, 90), 101);
            p.root = root(&[("Found", 10)]);
        }
        "E06-item-predicate" => {
            exists(&mut p, 10, tokenize(904, 90), 90);
            p.root = root(&[("Found", 10)]);
        }
        "E05-missing-owner" => {
            call(&mut p, 20, "equal", &[90, 301]);
            exists(&mut p, 10, tokenize(904, 90), 20);
            p.root = root(&[("Found", 10)]);
        }
        _ => unreachable!("authored profile"),
    }
    p
}
