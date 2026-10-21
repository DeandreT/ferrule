//! Genuine public Project definitions for owned SequenceAggregate. Authored
//! source only: no producer, predicate, expression or reducer is executed here.
use super::*;
use mapping::{
    AggregateOp, NamedSource, NamedTarget, OwnedPosition, OwnedSourceField, OwnedSourceRef,
    SequenceExpr, SourceOwner,
};

pub const NAMED: &str = "Cfg";
pub const EXACT: &str = " Cfg💠 ";
pub const UNUSED: &str = "unusedStatic";
pub const AUDIT: &str = "audit";
pub const UNICODE_TARGET: &str = "unicode";

pub const PROFILES: &[&str] = &[
    "S01-colliding",
    "S02-phases",
    "S03-reducers",
    "S04-producers",
    "S05-empty",
    "S06-numeric",
    "S07-lazy",
    "N01-rows",
    "N02-nested-owners",
    "G01-group-by",
    "G02-starting",
    "G03-ending",
    "F01-filter",
    "I01-item-at",
    "I02-nested-reducers",
    "L01-legacy-island",
    "E01-expression-first",
    "E02-predicate-first",
    "E03-argument-last",
    "E04-producer-error",
    "E05-empty-argument",
    "E06-not-bool",
    "E07-overflow",
    "E08-non-finite",
    "E09-missing-owner",
];

/// Every reducer, in the order S03 and S05 bind them.
pub const REDUCERS: [(AggregateOp, &str); 7] = [
    (AggregateOp::Count, "Count"),
    (AggregateOp::Sum, "Sum"),
    (AggregateOp::Avg, "Avg"),
    (AggregateOp::Min, "Min"),
    (AggregateOp::Max, "Max"),
    (AggregateOp::Join, "Join"),
    (AggregateOp::ItemAt, "ItemAt"),
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
fn float(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::Float)
}
fn output(fields: Vec<SchemaNode>) -> SchemaNode {
    SchemaNode::group("Output", fields)
}
/// The target type of each reducer's result in S03 and S05.
fn reducer_field(function: AggregateOp, name: &str) -> SchemaNode {
    match function {
        AggregateOp::Avg => float(name),
        AggregateOp::Join => string(name),
        _ => int(name),
    }
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
fn text(p: &mut Project, id: u32, value: &str) {
    constant(p, id, Value::String(value.into()));
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
fn item_position(p: &mut Project, id: u32) {
    p.graph
        .nodes
        .insert(id, Node::Position { collection: vec![] });
}
pub fn tokenize(input: u32, item: u32) -> SequenceExpr {
    SequenceExpr::Tokenize {
        input,
        delimiter: 900,
        item,
    }
}
fn generate(from: Option<u32>, to: u32, item: u32) -> SequenceExpr {
    SequenceExpr::Generate { from, to, item }
}
/// One reducer's optional phases: item predicate, item value expression and
/// receiving-context argument.
#[derive(Clone, Copy, Default)]
pub struct Phases {
    pub predicate: Option<u32>,
    pub expression: Option<u32>,
    pub arg: Option<u32>,
}
fn when(predicate: u32) -> Phases {
    Phases {
        predicate: Some(predicate),
        ..Phases::default()
    }
}
/// Inserts the consumer and its private item declaration.
pub fn aggregate(
    p: &mut Project,
    id: u32,
    function: AggregateOp,
    sequence: SequenceExpr,
    phases: Phases,
) {
    p.graph.nodes.insert(
        sequence.item(),
        Node::SourceField {
            path: vec![],
            frame: None,
        },
    );
    p.graph.nodes.insert(
        id,
        Node::SequenceAggregate {
            function,
            sequence,
            predicate: phases.predicate,
            expression: phases.expression,
            arg: phases.arg,
        },
    );
}

/// Owned reads for each endpoint use one ID block: Primary 0, Cfg 100,
/// exact Unicode 200 and the never-supplied static declaration 300.
pub fn base() -> Project {
    let mut p = Project {
        source: endpoint(),
        target: output(vec![int("Value")]),
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
                schema: output(vec![int("Count"), int("Primary")]),
                options: Default::default(),
                root: Scope {
                    bindings: bindings(&[("Count", 600), ("Primary", 601)]),
                    ..Default::default()
                },
            },
            NamedTarget {
                name: UNICODE_TARGET.into(),
                path: None,
                schema: output(vec![string("Joined"), int("Cross")]),
                options: Default::default(),
                root: Scope {
                    bindings: bindings(&[("Joined", 602), ("Cross", 603)]),
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
        (904, Value::String("x|M|U|U|M|Q|M".into())),
        (905, Value::String("a".into())),
        (906, Value::String("M|Z|Z".into())),
        (908, Value::String("Q|M|M".into())),
        (909, Value::String("Z".into())),
        (910, Value::Int(2)),
        (911, Value::Int(1)),
        (912, Value::String("b".into())),
        (913, Value::String("\\|".into())),
        (914, Value::String("[|]".into())),
        (915, Value::String("i".into())),
        (916, Value::String("Q|M".into())),
        (917, Value::String("Z|Z".into())),
        (919, Value::String("".into())),
    ] {
        constant(&mut p, id, value);
    }
    // Fixed named targets: exact named owners beside a colliding Primary code.
    call(&mut p, 610, "equal", &[96, 101]);
    aggregate(
        &mut p,
        600,
        AggregateOp::Count,
        tokenize(102, 96),
        when(610),
    );
    call(&mut p, 611, "equal", &[97, 1]);
    aggregate(
        &mut p,
        601,
        AggregateOp::Count,
        tokenize(102, 97),
        when(611),
    );
    call(&mut p, 612, "concat", &[98, 201]);
    aggregate(
        &mut p,
        602,
        AggregateOp::Join,
        tokenize(202, 98),
        Phases {
            expression: Some(612),
            arg: Some(101),
            ..Phases::default()
        },
    );
    call(&mut p, 613, "equal", &[99, 101]);
    aggregate(
        &mut p,
        603,
        AggregateOp::Count,
        tokenize(202, 99),
        when(613),
    );
    p
}

fn rows_scope(field: &str, name: &str) -> Scope {
    Scope {
        target_field: field.into(),
        iteration: iteration(name),
        ..Default::default()
    }
}
fn root(fields: &[(&str, u32)]) -> Scope {
    Scope {
        bindings: bindings(fields),
        ..Default::default()
    }
}
fn children(scope: Scope) -> Scope {
    Scope {
        children: vec![scope],
        ..Default::default()
    }
}
/// `join(tokenize(list), item == "b" ? raise : true, item == "a" ? raise : item, raise)`.
fn ordered_failures(p: &mut Project, list: &str) {
    text(p, 930, list);
    call(p, 20, "equal", &[90, 905]);
    call(p, 21, "equal", &[90, 912]);
    raise(p, 50, "predicate");
    raise(p, 51, "expression");
    raise(p, 52, "argument");
    iff(p, 23, 21, 50, 901);
    iff(p, 24, 20, 51, 90);
    aggregate(
        p,
        10,
        AggregateOp::Join,
        tokenize(930, 90),
        Phases {
            predicate: Some(23),
            expression: Some(24),
            arg: Some(52),
        },
    );
    p.target = output(vec![string("Value")]);
    p.root = root(&[("Value", 10)]);
}

pub fn project(profile: &str) -> Project {
    assert!(PROFILES.contains(&profile), "unknown profile {profile}");
    let mut p = base();
    match profile {
        "S01-colliding" => {
            for (index, owned) in [1, 101, 201, 24].into_iter().enumerate() {
                let index = index as u32;
                call(&mut p, 20 + index, "equal", &[90 + index, owned]);
                aggregate(
                    &mut p,
                    10 + index,
                    AggregateOp::Count,
                    tokenize(904, 90 + index),
                    when(20 + index),
                );
            }
            p.graph.nodes.insert(
                24,
                Node::SourceField {
                    path: path(&["Code"]),
                    frame: None,
                },
            );
            p.target = output(vec![
                int("Primary"),
                int("Named"),
                int("Unicode"),
                int("Legacy"),
            ]);
            p.root = root(&[
                ("Primary", 10),
                ("Named", 11),
                ("Unicode", 12),
                ("Legacy", 13),
            ]);
        }
        "S02-phases" => {
            call(&mut p, 20, "concat", &[90, 201]);
            aggregate(
                &mut p,
                10,
                AggregateOp::Join,
                tokenize(102, 90),
                Phases {
                    expression: Some(20),
                    arg: Some(1),
                    ..Phases::default()
                },
            );
            call(&mut p, 21, "multiply", &[91, 3]);
            aggregate(
                &mut p,
                11,
                AggregateOp::Sum,
                generate(None, 103, 91),
                Phases {
                    expression: Some(21),
                    ..Phases::default()
                },
            );
            call(&mut p, 22, "equal", &[92, 1]);
            call(&mut p, 23, "not", &[22]);
            aggregate(
                &mut p,
                12,
                AggregateOp::ItemAt,
                tokenize(2, 92),
                Phases {
                    predicate: Some(23),
                    arg: Some(203),
                    ..Phases::default()
                },
            );
            p.target = output(vec![string("Joined"), int("Summed"), string("Picked")]);
            p.root = root(&[("Joined", 10), ("Summed", 11), ("Picked", 12)]);
        }
        "S03-reducers" => {
            let mut fields = Vec::new();
            let mut targets = Vec::new();
            for (index, (function, name)) in REDUCERS.into_iter().enumerate() {
                let index = index as u32;
                call(&mut p, 20 + index, "equal", &[80 + index, 3]);
                call(&mut p, 30 + index, "not", &[20 + index]);
                let arg = match function {
                    AggregateOp::Join => Some(101),
                    AggregateOp::ItemAt => Some(3),
                    _ => None,
                };
                aggregate(
                    &mut p,
                    10 + index,
                    function,
                    generate(None, 103, 80 + index),
                    Phases {
                        predicate: Some(30 + index),
                        arg,
                        ..Phases::default()
                    },
                );
                fields.push(reducer_field(function, name));
                targets.push((name, 10 + index));
            }
            p.target = output(fields);
            p.root = root(&targets);
        }
        "S04-producers" => {
            aggregate(
                &mut p,
                10,
                AggregateOp::Join,
                SequenceExpr::TokenizeByLength {
                    input: 102,
                    length: 910,
                    item: 90,
                },
                Phases {
                    arg: Some(201),
                    ..Phases::default()
                },
            );
            aggregate(
                &mut p,
                11,
                AggregateOp::Count,
                SequenceExpr::TokenizeRegex {
                    input: 102,
                    pattern: 913,
                    flags: None,
                    item: 91,
                },
                Phases::default(),
            );
            aggregate(
                &mut p,
                12,
                AggregateOp::Join,
                SequenceExpr::TokenizeRegex {
                    input: 102,
                    pattern: 914,
                    flags: Some(915),
                    item: 92,
                },
                Phases {
                    arg: Some(1),
                    ..Phases::default()
                },
            );
            aggregate(
                &mut p,
                13,
                AggregateOp::Sum,
                generate(Some(3), 103, 93),
                Phases::default(),
            );
            p.target = output(vec![
                string("ByLength"),
                int("RegexNoFlags"),
                string("RegexFlags"),
                int("Range"),
            ]);
            p.root = root(&[
                ("ByLength", 10),
                ("RegexNoFlags", 11),
                ("RegexFlags", 12),
                ("Range", 13),
            ]);
        }
        "S05-empty" => {
            raise(&mut p, 20, "predicate");
            raise(&mut p, 21, "expression");
            let mut fields = Vec::new();
            let mut targets = Vec::new();
            for (index, (function, name)) in REDUCERS.into_iter().enumerate() {
                let index = index as u32;
                let arg = match function {
                    AggregateOp::Join => Some(101),
                    AggregateOp::ItemAt => Some(3),
                    _ => None,
                };
                aggregate(
                    &mut p,
                    10 + index,
                    function,
                    tokenize(903, 80 + index),
                    Phases {
                        predicate: Some(20),
                        expression: Some(21),
                        arg,
                    },
                );
                fields.push(reducer_field(function, name));
                targets.push((name, 10 + index));
            }
            aggregate(
                &mut p,
                17,
                AggregateOp::Count,
                tokenize(919, 87),
                Phases::default(),
            );
            aggregate(&mut p, 18, AggregateOp::Sum, tokenize(904, 88), when(902));
            raise(&mut p, 22, "delimiter");
            call(&mut p, 23, "equal", &[89, 301]);
            aggregate(
                &mut p,
                19,
                AggregateOp::Count,
                SequenceExpr::Tokenize {
                    input: 903,
                    delimiter: 22,
                    item: 89,
                },
                when(23),
            );
            fields.extend([int("EmptyText"), int("Rejected"), int("Unreached")]);
            targets.extend([("EmptyText", 17), ("Rejected", 18), ("Unreached", 19)]);
            p.target = output(fields);
            p.root = root(&targets);
        }
        "S06-numeric" => {
            text(&mut p, 930, "1|a|2");
            text(&mut p, 931, "a|b");
            text(&mut p, 932, "2|1.5");
            let none = Phases::default();
            aggregate(&mut p, 10, AggregateOp::Sum, tokenize(930, 90), none);
            aggregate(&mut p, 11, AggregateOp::Avg, tokenize(930, 91), none);
            aggregate(&mut p, 12, AggregateOp::Min, tokenize(931, 92), none);
            aggregate(&mut p, 13, AggregateOp::Min, tokenize(932, 93), none);
            aggregate(&mut p, 14, AggregateOp::Max, tokenize(932, 94), none);
            p.target = output(vec![
                int("Sum"),
                float("Avg"),
                int("NoNumbers"),
                float("MinMixed"),
                int("MaxMixed"),
            ]);
            p.root = root(&[
                ("Sum", 10),
                ("Avg", 11),
                ("NoNumbers", 12),
                ("MinMixed", 13),
                ("MaxMixed", 14),
            ]);
        }
        "S07-lazy" => {
            // Rejected items never evaluate their failing value expression.
            text(&mut p, 930, "c|a|d");
            call(&mut p, 20, "equal", &[90, 905]);
            call(&mut p, 21, "not", &[20]);
            call(&mut p, 22, "equal", &[90, 905]);
            raise(&mut p, 23, "expression");
            iff(&mut p, 24, 22, 23, 90);
            aggregate(
                &mut p,
                10,
                AggregateOp::Join,
                tokenize(930, 90),
                Phases {
                    predicate: Some(21),
                    expression: Some(24),
                    arg: Some(101),
                },
            );
            p.target = output(vec![string("Value")]);
            p.root = root(&[("Value", 10)]);
        }
        "N01-rows" => {
            item_position(&mut p, 23);
            call(&mut p, 20, "equal", &[90, 4]);
            call(&mut p, 21, "equal", &[23, 7]);
            call(&mut p, 22, "and", &[20, 21]);
            aggregate(&mut p, 10, AggregateOp::Count, tokenize(908, 90), when(22));
            item_position(&mut p, 26);
            call(&mut p, 25, "concat", &[91, 5, 26, 7]);
            aggregate(
                &mut p,
                11,
                AggregateOp::Join,
                tokenize(916, 91),
                Phases {
                    expression: Some(25),
                    arg: Some(101),
                    ..Phases::default()
                },
            );
            p.target = output(vec![
                SchemaNode::group(
                    "Rows",
                    vec![
                        string("Label"),
                        int("Position"),
                        int("Count"),
                        string("Labels"),
                    ],
                )
                .repeating(),
            ]);
            let mut rows = rows_scope("Rows", "Primary");
            rows.bindings =
                bindings(&[("Label", 5), ("Position", 7), ("Count", 10), ("Labels", 11)]);
            p.root = children(rows);
        }
        "N02-nested-owners" => {
            call(&mut p, 20, "equal", &[90, 4]);
            call(&mut p, 21, "equal", &[90, 104]);
            call(&mut p, 22, "and", &[20, 21]);
            aggregate(&mut p, 10, AggregateOp::Count, tokenize(916, 90), when(22));
            p.target = output(vec![
                SchemaNode::group(
                    "Outer",
                    vec![
                        string("Label"),
                        SchemaNode::group("Inner", vec![string("Label"), int("Count")]).repeating(),
                    ],
                )
                .repeating(),
            ]);
            let mut inner = rows_scope("Inner", NAMED);
            inner.bindings = bindings(&[("Label", 105), ("Count", 10)]);
            let mut outer = rows_scope("Outer", "Primary");
            outer.bindings = bindings(&[("Label", 5)]);
            outer.children = vec![inner];
            p.root = children(outer);
        }
        "G01-group-by" => {
            call(&mut p, 20, "equal", &[90, 4]);
            aggregate(&mut p, 10, AggregateOp::Count, tokenize(906, 90), when(20));
            call(&mut p, 21, "equal", &[91, 7]);
            aggregate(
                &mut p,
                11,
                AggregateOp::Count,
                generate(Some(910), 910, 91),
                when(21),
            );
            p.target = output(vec![
                SchemaNode::group(
                    "Groups",
                    vec![
                        string("First"),
                        int("Count"),
                        int("Ordinal"),
                        SchemaNode::group(
                            "Members",
                            vec![string("Label"), int("Count"), int("Ordinal")],
                        )
                        .repeating(),
                    ],
                )
                .repeating(),
            ]);
            let mut members = rows_scope("Members", "Primary");
            members.bindings = bindings(&[("Label", 5), ("Count", 10), ("Ordinal", 11)]);
            let mut groups = rows_scope("Groups", "Primary");
            groups.group_by = Some(6);
            groups.bindings = bindings(&[("First", 5), ("Count", 10), ("Ordinal", 11)]);
            groups.children = vec![members];
            p.root = children(groups);
        }
        "G02-starting" | "G03-ending" | "F01-filter" => {
            // A row is selected when exactly one generated item equals its code.
            let list = if profile == "F01-filter" { 102 } else { 909 };
            call(&mut p, 20, "equal", &[90, 4]);
            aggregate(&mut p, 10, AggregateOp::Count, tokenize(list, 90), when(20));
            call(&mut p, 12, "equal", &[10, 911]);
            let field = if profile == "F01-filter" {
                "Rows"
            } else {
                "Groups"
            };
            p.target = output(vec![
                SchemaNode::group(field, vec![string("First")]).repeating(),
            ]);
            let mut scope = rows_scope(field, "Primary");
            match profile {
                "G02-starting" => scope.group_starting_with = Some(12),
                "G03-ending" => scope.group_ending_with = Some(12),
                _ => scope.filter = Some(12),
            }
            scope.bindings = bindings(&[("First", 5)]);
            p.root = children(scope);
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
            aggregate(&mut p, 10, AggregateOp::Count, tokenize(904, 90), when(20));
            p.root = root(&[("Value", 10)]);
        }
        "I02-nested-reducers" => {
            call(&mut p, 23, "equal", &[91, 101]);
            p.graph.nodes.insert(
                22,
                Node::SequenceExists {
                    sequence: tokenize(102, 91),
                    predicate: 23,
                },
            );
            p.graph.nodes.insert(
                91,
                Node::SourceField {
                    path: vec![],
                    frame: None,
                },
            );
            call(&mut p, 25, "equal", &[92, 101]);
            aggregate(&mut p, 24, AggregateOp::Count, tokenize(102, 92), when(25));
            call(&mut p, 26, "multiply", &[90, 24]);
            aggregate(
                &mut p,
                10,
                AggregateOp::Sum,
                generate(None, 103, 90),
                Phases {
                    predicate: Some(22),
                    expression: Some(26),
                    ..Phases::default()
                },
            );
            p.root = root(&[("Value", 10)]);
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
            aggregate(&mut p, 10, AggregateOp::Count, tokenize(917, 90), when(20));
            p.target = output(vec![
                SchemaNode::group("Legacy", vec![int("Count")]).repeating(),
            ]);
            p.root = children(Scope {
                target_field: "Legacy".into(),
                iteration: ScopeIteration::Source(path(&["Rows"])),
                bindings: bindings(&[("Count", 10)]),
                ..Default::default()
            });
        }
        "E01-expression-first" => ordered_failures(&mut p, "a|b"),
        "E02-predicate-first" => ordered_failures(&mut p, "b|a"),
        "E03-argument-last" => ordered_failures(&mut p, "c|d"),
        "E04-producer-error" => {
            raise(&mut p, 30, "producer");
            raise(&mut p, 20, "predicate");
            raise(&mut p, 21, "argument");
            aggregate(
                &mut p,
                10,
                AggregateOp::Count,
                tokenize(30, 90),
                Phases {
                    predicate: Some(20),
                    arg: Some(21),
                    ..Phases::default()
                },
            );
            p.root = root(&[("Value", 10)]);
        }
        "E05-empty-argument" => {
            raise(&mut p, 20, "predicate");
            aggregate(
                &mut p,
                10,
                AggregateOp::Join,
                tokenize(903, 90),
                Phases {
                    predicate: Some(20),
                    arg: Some(301),
                    ..Phases::default()
                },
            );
            p.target = output(vec![string("Value")]);
            p.root = root(&[("Value", 10)]);
        }
        "E06-not-bool" => {
            aggregate(&mut p, 10, AggregateOp::Count, tokenize(904, 90), when(101));
            p.root = root(&[("Value", 10)]);
        }
        "E07-overflow" => {
            text(&mut p, 930, "9223372036854775807|1");
            aggregate(
                &mut p,
                10,
                AggregateOp::Sum,
                tokenize(930, 90),
                Phases::default(),
            );
            p.root = root(&[("Value", 10)]);
        }
        "E08-non-finite" => {
            text(&mut p, 930, "1|1e999");
            aggregate(
                &mut p,
                10,
                AggregateOp::Avg,
                tokenize(930, 90),
                Phases::default(),
            );
            p.target = output(vec![float("Value")]);
            p.root = root(&[("Value", 10)]);
        }
        "E09-missing-owner" => {
            call(&mut p, 20, "equal", &[90, 301]);
            aggregate(&mut p, 10, AggregateOp::Count, tokenize(904, 90), when(20));
            p.root = root(&[("Value", 10)]);
        }
        _ => unreachable!("authored profile"),
    }
    p
}
