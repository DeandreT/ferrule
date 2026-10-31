//! Self-authored designs shared by interpreter and generated-host regressions.
use ir::{ScalarType, SchemaNode};
use mapping::{Binding, Graph, Node, Project, Scope, ScopeIteration};
use serde_json::{Value, json};

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}

fn scalar(name: &str, ty: ScalarType) -> SchemaNode {
    SchemaNode::scalar(name, ty)
}

fn bind(target: &str, node: u32) -> Binding {
    Binding {
        target_field: target.into(),
        node,
    }
}

pub fn project(separate: bool, hops: usize) -> Project {
    let text = |name| scalar(name, ScalarType::String);
    let flag = scalar("Flag", ScalarType::Bool);
    let object = SchemaNode::group("Object", vec![text("Value")]);
    let detail = SchemaNode::group(
        "Detail",
        vec![
            text("Flag"),
            text("Label"),
            object.clone(),
            SchemaNode::group("Inner", vec![text("Flag"), text("Label")]),
            SchemaNode::group(
                "Children",
                vec![text("Flag"), SchemaNode::group("Deep", vec![text("Flag")])],
            )
            .repeating(),
        ],
    )
    .nullable_container()
    .unwrap();
    let output_fields = vec![
        scalar("Pinned", ScalarType::Bool),
        scalar("Absolute", ScalarType::Bool),
        text("Fallback"),
        SchemaNode::group(
            "Item",
            vec![text("Name"), scalar("Position", ScalarType::Int)],
        )
        .repeating(),
        SchemaNode::group(
            "Child",
            vec![scalar("Ancestor", ScalarType::Bool), text("ChildFlag")],
        )
        .repeating(),
    ];
    let leaf = Scope {
        target_field: "Rows".into(),
        iteration: ScopeIteration::Source(if separate {
            if hops == 1 {
                path(&["Detail"])
            } else {
                path(&["Detail", "Inner"])
            }
        } else if hops == 1 {
            path(&["Rows", "Detail"])
        } else {
            path(&["Rows", "Detail", "Inner"])
        }),
        bindings: vec![bind("Pinned", 0), bind("Absolute", 1), bind("Fallback", 2)],
        children: vec![
            Scope {
                target_field: "Item".into(),
                iteration: ScopeIteration::OptionalPresence { condition: 0 },
                bindings: vec![bind("Name", 3), bind("Position", 4)],
                ..Default::default()
            },
            Scope {
                target_field: "Child".into(),
                iteration: ScopeIteration::Source(path(&["Children", "Deep"])),
                bindings: vec![bind("Ancestor", 0), bind("ChildFlag", 5)],
                ..Default::default()
            },
        ]
        .into_iter()
        .take(if hops == 1 { 2 } else { 1 })
        .collect(),
        ..Default::default()
    };
    let (target, root) = if separate {
        (
            SchemaNode::group(
                "Output",
                vec![
                    SchemaNode::group(
                        "Outer",
                        vec![SchemaNode::group("Rows", output_fields).repeating()],
                    )
                    .repeating(),
                ],
            ),
            Scope {
                children: vec![Scope {
                    target_field: "Outer".into(),
                    iteration: ScopeIteration::Source(path(&["Rows"])),
                    children: vec![leaf],
                    ..Default::default()
                }],
                ..Default::default()
            },
        )
    } else {
        (
            SchemaNode::group(
                "Output",
                vec![SchemaNode::group("Rows", output_fields).repeating()],
            ),
            Scope {
                children: vec![leaf],
                ..Default::default()
            },
        )
    };
    Project {
        source: SchemaNode::group(
            "Input",
            vec![SchemaNode::group("Rows", vec![flag, text("Label"), object, detail]).repeating()],
        ),
        target,
        graph: Graph {
            nodes: [
                (
                    0,
                    Node::SourceField {
                        path: path(&["Flag"]),
                        frame: Some(path(&["Rows"])),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: path(&["Rows", "Flag"]),
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: path(&["Label"]),
                        frame: None,
                    },
                ),
                (
                    3,
                    Node::SourceField {
                        path: path(&["Label"]),
                        frame: Some(path(&["Rows"])),
                    },
                ),
                (
                    4,
                    Node::Position {
                        collection: path(&["Rows"]),
                    },
                ),
                (
                    5,
                    Node::SourceField {
                        path: path(&["Flag"]),
                        frame: Some(path(&["Children"])),
                    },
                ),
            ]
            .into(),
        },
        root,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}

pub fn cases(separate: bool, hops: usize) -> Vec<Value> {
    let descendant = |flag: &str, label: &str| {
        json!({
            "Flag":flag,"Label":label,"Object":{"Value":"descendant"},
            "Inner":{"Flag":"inner","Label":"inner-label"},
            "Children":[{"Flag":"child-a","Deep":{"Flag":"deep-a"}},{"Flag":"child-b","Deep":{"Flag":"deep-b"}}]
        })
    };
    let input = json!({"Rows":[
        {"Flag":true,"Label":"row-a","Object":{"Value":"ancestor-a"},"Detail":descendant("not-a-bool", "detail-a")},
        {"Flag":false,"Label":"row-b","Object":{"Value":"ancestor-b"},"Detail":descendant("also-not-a-bool", "detail-b")},
        {"Flag":true,"Label":"absent"},
        {"Flag":true,"Label":"empty","Detail":{}},
        {"Flag":false,"Label":"null","Detail":null}
    ]});
    let expected_row = |flag: bool,
                        label: &str,
                        position: usize,
                        fallback: &str,
                        children: bool| {
        let mut row = json!({"Pinned":flag,"Absolute":flag,"Fallback":fallback,"Item":[]});
        if hops == 1 {
            row["Child"] = json!([]);
        }
        if flag {
            row["Item"] = json!([{"Name":label,"Position":position}]);
        }
        if children {
            row["Child"] = json!([{"Ancestor":flag,"ChildFlag":"child-a"},{"Ancestor":flag,"ChildFlag":"child-b"}]);
        }
        row
    };
    let mut rows = vec![
        expected_row(
            true,
            "row-a",
            1,
            if hops == 1 { "detail-a" } else { "inner-label" },
            hops == 1,
        ),
        expected_row(
            false,
            "row-b",
            2,
            if hops == 1 { "detail-b" } else { "inner-label" },
            hops == 1,
        ),
    ];
    let expected = if separate {
        let mut outer = rows
            .drain(..)
            .map(|row| json!({"Rows":[row]}))
            .collect::<Vec<_>>();
        if hops == 1 {
            outer.push(json!({"Rows":[expected_row(true,"absent",3,"absent",false)]}));
            let mut empty = expected_row(true, "empty", 4, "empty", false);
            empty.as_object_mut().unwrap().remove("Fallback");
            outer.push(json!({"Rows":[empty]}));
            outer.push(json!({"Rows":[expected_row(false,"null",5,"null",false)]}));
        } else {
            outer.extend([
                json!({"Rows":[]}),
                json!({"Rows":[expected_row(true,"empty",4,"empty",false)]}),
                json!({"Rows":[]}),
            ]);
        }
        json!({"Outer":outer})
    } else {
        if hops == 1 {
            let mut empty = expected_row(true, "empty", 4, "empty", false);
            empty.as_object_mut().unwrap().remove("Fallback");
            rows.extend([
                expected_row(true, "absent", 3, "absent", false),
                empty,
                expected_row(false, "null", 5, "null", false),
            ]);
        } else {
            rows.push(expected_row(true, "empty", 4, "empty", false));
        }
        json!({"Rows":rows})
    };
    vec![json!({"input":input.to_string(),"expected":expected})]
}

/// Reducers add their own repeating/private frames after the singleton Detail.
/// Its colliding fields must never become the owner of a pinned Rows read.
pub fn reducer_project() -> Project {
    use mapping::{AggregateOp, SequenceExpr};
    let mut project = project(false, 1);
    project.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    scalar("RowValue", ScalarType::String),
                    scalar("RowPosition", ScalarType::Int),
                    scalar("AggregateValue", ScalarType::String),
                    scalar("ChildPositionTotal", ScalarType::Int),
                    scalar("AggregateRowPositionTotal", ScalarType::Int),
                    scalar("GeneratedExists", ScalarType::Bool),
                    scalar("GeneratedValue", ScalarType::String),
                    scalar("GeneratedPositionTotal", ScalarType::Int),
                    scalar("GeneratedRowPositionTotal", ScalarType::Int),
                ],
            )
            .repeating(),
        ],
    );
    let call = |function: &str, args: &[u32]| Node::Call {
        function: function.into(),
        args: args.to_vec(),
    };
    let private_item = || Node::SourceField {
        path: vec![],
        frame: None,
    };
    let tokens = |item| SequenceExpr::Tokenize {
        input: 27,
        delimiter: 12,
        item,
    };
    let range = |item| SequenceExpr::Generate {
        from: None,
        to: 22,
        item,
    };
    let aggregate = |function, expression, arg| Node::Aggregate {
        function,
        collection: path(&["Children"]),
        value: vec![],
        expression: Some(expression),
        arg,
    };
    let generated = |function, sequence, expression, arg| Node::SequenceAggregate {
        function,
        sequence,
        predicate: None,
        expression: Some(expression),
        arg,
    };
    project.graph.nodes.extend([
        (
            6,
            Node::Position {
                collection: path(&["Children"]),
            },
        ),
        (10, aggregate(AggregateOp::Join, 11, Some(12))),
        (11, call("concat", &[3, 13, 5])),
        (
            12,
            Node::Const {
                value: ir::Value::String(",".into()),
            },
        ),
        (
            13,
            Node::Const {
                value: ir::Value::String(":".into()),
            },
        ),
        (14, aggregate(AggregateOp::Sum, 6, None)),
        (15, aggregate(AggregateOp::Sum, 4, None)),
        (
            20,
            Node::SequenceExists {
                sequence: tokens(21),
                predicate: 25,
            },
        ),
        (21, private_item()),
        (
            22,
            Node::Const {
                value: ir::Value::Int(2),
            },
        ),
        (23, call("equal", &[21, 29])),
        (24, call("equal", &[26, 22])),
        (25, call("and", &[0, 23, 24])),
        (26, Node::Position { collection: vec![] }),
        (
            27,
            Node::Const {
                value: ir::Value::String("x,y".into()),
            },
        ),
        (
            29,
            Node::Const {
                value: ir::Value::String("y".into()),
            },
        ),
        (30, generated(AggregateOp::Join, tokens(31), 32, Some(12))),
        (31, private_item()),
        (32, call("concat", &[3, 13, 31])),
        (40, generated(AggregateOp::Sum, range(41), 42, None)),
        (41, private_item()),
        (42, Node::Position { collection: vec![] }),
        (45, generated(AggregateOp::Sum, range(46), 4, None)),
        (46, private_item()),
    ]);
    let scope = &mut project.root.children[0];
    scope.children.clear();
    scope.bindings = vec![
        bind("RowValue", 3),
        bind("RowPosition", 4),
        bind("AggregateValue", 10),
        bind("ChildPositionTotal", 14),
        bind("AggregateRowPositionTotal", 15),
        bind("GeneratedExists", 20),
        bind("GeneratedValue", 30),
        bind("GeneratedPositionTotal", 40),
        bind("GeneratedRowPositionTotal", 45),
    ];
    project
}

pub fn reducer_cases() -> Vec<Value> {
    vec![json!({
        "input": json!({"Rows":[
            {"Flag":true,"Label":"row-a","Detail":{"Flag":"shadow-flag-a","Label":"shadow-a","Children":[{"Flag":"one"},{"Flag":"two"}]}},
            {"Flag":false,"Label":"row-b","Detail":{"Flag":"shadow-flag-b","Label":"shadow-b","Children":[{"Flag":"three"}]}}
        ]}).to_string(),
        "expected":{"Rows":[
            {"RowValue":"row-a","RowPosition":1,"AggregateValue":"row-a:one,row-a:two","ChildPositionTotal":3,"AggregateRowPositionTotal":2,"GeneratedExists":true,"GeneratedValue":"row-a:x,row-a:y","GeneratedPositionTotal":3,"GeneratedRowPositionTotal":2},
            {"RowValue":"row-b","RowPosition":2,"AggregateValue":"row-b:three","ChildPositionTotal":1,"AggregateRowPositionTotal":2,"GeneratedExists":false,"GeneratedValue":"row-b:x,row-b:y","GeneratedPositionTotal":3,"GeneratedRowPositionTotal":4}
        ]}
    })]
}
