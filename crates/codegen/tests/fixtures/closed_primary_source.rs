#![allow(dead_code)]
use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding, ExecutionContract, FunctionId, FunctionParameter, FunctionParameterId, Graph,
    NamedTarget, Node, Project, Scope, ScopeIteration, UserFunction,
};
use std::collections::BTreeMap;

pub const BMP: &str = "X\u{ff21}";
pub const SUPPLEMENTARY: &str = "X\u{10000}";
pub const TRANSPORT_FIRST: &str = "AsciiSource";
pub const TRANSPORT_SECOND: &str = "UnicodeSource";

fn bind(name: &str, node: u32) -> Binding {
    Binding {
        target_field: name.into(),
        node,
    }
}
fn text(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}

pub fn project() -> Project {
    let result = || vec![text("Root"), text("Result")];
    let mut row_fields = result();
    row_fields.push(SchemaNode::group("Child", vec![text("Result")]).repeating());
    Project {
        execution_contract: Some(ExecutionContract::ClosedPrimarySourceV1),
        source: SchemaNode::group(
            "Input",
            vec![
                text("Text").nillable(),
                text(TRANSPORT_FIRST),
                text(TRANSPORT_SECOND),
                SchemaNode::group(
                    "Row",
                    vec![
                        text("Value").nillable(),
                        SchemaNode::group("Cell", vec![text("Value")]).repeating(),
                    ],
                )
                .repeating(),
                SchemaNode::group("Cell", vec![text("Value")]).repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Output",
            vec![
                text("Root"),
                text("UnicodeA"),
                text("UnicodeB"),
                SchemaNode::group("Rows", row_fields).repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: mapping::FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        target_options: mapping::FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        extra_sources: Vec::new(),
        extra_targets: vec![NamedTarget {
            name: "audit".into(),
            path: None,
            options: mapping::FormatOptions {
                xml_document: true,
                ..Default::default()
            },
            schema: SchemaNode::group("Audit", vec![text("Root")]),
            root: Scope {
                bindings: vec![bind("Root", 4)],
                ..Default::default()
            },
        }],
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    Node::SourceField {
                        path: vec!["Text".into()],
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: Some(vec!["Row".into()]),
                    },
                ),
                (
                    3,
                    Node::UserFunctionCall {
                        function: FunctionId::new(1),
                        args: vec![2],
                    },
                ),
                (
                    4,
                    Node::UserFunctionCall {
                        function: FunctionId::new(1),
                        args: vec![1],
                    },
                ),
                (
                    5,
                    Node::SourceField {
                        path: vec![TRANSPORT_FIRST.into()],
                        frame: None,
                    },
                ),
                (
                    6,
                    Node::SourceField {
                        path: vec![TRANSPORT_SECOND.into()],
                        frame: None,
                    },
                ),
            ]),
        },
        user_functions: BTreeMap::from([(
            FunctionId::new(1),
            UserFunction {
                library: "synthetic".into(),
                name: "identity".into(),
                description: None,
                parameters: vec![FunctionParameter {
                    id: FunctionParameterId::new(1),
                    name: "value".into(),
                    ty: ScalarType::String,
                }],
                output_name: "value".into(),
                output_type: ScalarType::String,
                body: Graph {
                    nodes: BTreeMap::from([
                        (
                            1,
                            Node::FunctionParameter {
                                parameter: FunctionParameterId::new(1),
                            },
                        ),
                        (
                            2,
                            Node::Const {
                                value: Value::Bool(true),
                            },
                        ),
                        // Admission must not execute an unselected body branch.
                        (3, Node::Raise { message: None }),
                        (
                            4,
                            Node::If {
                                condition: 2,
                                then: 1,
                                else_: 3,
                            },
                        ),
                    ]),
                },
                output: 4,
            },
        )]),
        root: Scope {
            bindings: vec![bind("Root", 4), bind("UnicodeA", 5), bind("UnicodeB", 6)],
            children: vec![Scope {
                target_field: "Rows".into(),
                iteration: ScopeIteration::Source(vec!["Row".into()]),
                bindings: vec![bind("Root", 4), bind("Result", 3)],
                children: vec![Scope {
                    target_field: "Child".into(),
                    iteration: ScopeIteration::Source(vec!["Cell".into()]),
                    bindings: vec![bind("Result", 3)],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
    }
}

pub fn program() -> codegen::Program {
    codegen::lower(&project()).unwrap()
}

/// Supplementary NCNames are valid for the typed guard, but intentionally
/// outside the existing Structured XML transport profile. Do not widen it.
pub fn unicode_program() -> codegen::Program {
    let mut project = project();
    project.source = SchemaNode::group("Input", vec![text(BMP), text(SUPPLEMENTARY)]);
    project.target = SchemaNode::group("Output", vec![text("A"), text("B")]);
    project.source_options = Default::default();
    project.target_options = Default::default();
    project.extra_targets.clear();
    project.graph.nodes = BTreeMap::from([
        (
            1,
            Node::SourceField {
                path: vec![BMP.into()],
                frame: None,
            },
        ),
        (
            2,
            Node::SourceField {
                path: vec![SUPPLEMENTARY.into()],
                frame: None,
            },
        ),
        (
            3,
            Node::UserFunctionCall {
                function: FunctionId::new(1),
                args: vec![1],
            },
        ),
        (
            4,
            Node::UserFunctionCall {
                function: FunctionId::new(1),
                args: vec![2],
            },
        ),
    ]);
    project.root = Scope {
        bindings: vec![bind("A", 3), bind("B", 4)],
        ..Default::default()
    };
    codegen::lower(&project).unwrap()
}

pub fn document(json: bool) -> codegen::DocumentProgram {
    build_document(json, false)
}

pub fn filename_document() -> codegen::DocumentProgram {
    build_document(false, true)
}

fn build_document(json: bool, output: bool) -> codegen::DocumentProgram {
    let mut project = project();
    if !output {
        project.extra_targets.clear();
    }
    let extension = if json { "json" } else { "xml" };
    project.source_path = Some(format!("input.{extension}"));
    project.target_path = Some(format!("output.{extension}"));
    if json {
        project.source_options = Default::default();
        project.target_options = Default::default();
    }
    project.graph.nodes.insert(
        10,
        Node::Const {
            value: Value::String(format!("input.{extension}")),
        },
    );
    project.graph.nodes.insert(
        11,
        Node::Const {
            value: Value::String(format!("output.{extension}")),
        },
    );
    let document = mapping::DocumentProject::new(project, mapping::PrimarySourcePlan::new(10));
    let document = if output {
        document.with_output_filename(11)
    } else {
        document
    };
    codegen::lower_document(&document).unwrap()
}
