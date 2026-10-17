use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, Value};
use mapping::{FunctionParameter, Graph, Node, NodeId, Project};

#[path = "udf_multi_case_if_else/backend.rs"]
mod backend;
#[path = "udf_multi_case_if_else/fixtures.rs"]
mod fixtures;

use fixtures::NestedFailure;

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_udf_multi_case_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// The feed list describes physical positions, including declared keyless pins.
// First/Second are boolean inputs; A/B/C/D are independently authored literals.
fn fixture(dir: &Directory, multi: bool, feeds: &[Option<u32>], bad: Option<u32>) -> PathBuf {
    fixtures::fixture(&dir.0, multi, feeds, bad)
}

fn strict(path: &Path) -> Project {
    let report = mfd::import_with_profile(
        path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap();
    assert!(
        report.imported.warnings.is_empty(),
        "{:?}",
        report.imported.warnings
    );
    assert!(engine::validate(&report.imported.project).is_empty());
    report.imported.project
}

fn twice(dir: &Directory, multi: bool, feeds: &[Option<u32>], bad: Option<u32>) -> [Project; 2] {
    let first = strict(&fixture(dir, multi, feeds, bad));
    let path = dir.0.join("second.mfd");
    let report = mfd::export_with_profile(&first, &path, mfd::ExportProfile::NativeMfd).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let second = strict(&path);
    [first, second]
}

#[derive(Clone, Debug, PartialEq)]
enum Tree {
    Variable(String),
    Literal(Value),
    Call(String, Vec<Tree>),
    If(Box<Tree>, Box<Tree>, Box<Tree>),
}

fn text(value: &str) -> Tree {
    Tree::Literal(Value::String(value.into()))
}

fn variable(name: &str) -> Tree {
    Tree::Variable(name.into())
}

fn branch(condition: Tree, then: Tree, otherwise: Tree) -> Tree {
    Tree::If(Box::new(condition), Box::new(then), Box::new(otherwise))
}

fn tree(graph: &Graph, id: NodeId, parameters: &[FunctionParameter]) -> Tree {
    match &graph.nodes[&id] {
        Node::Const { value } => Tree::Literal(value.clone()),
        Node::SourceField { path, frame: None } if path.len() == 1 => variable(&path[0]),
        Node::FunctionParameter { parameter } => {
            variable(&parameters.iter().find(|p| p.id == *parameter).unwrap().name)
        }
        Node::Call { function, args } => Tree::Call(
            function.clone(),
            args.iter().map(|id| tree(graph, *id, parameters)).collect(),
        ),
        Node::If {
            condition,
            then,
            else_,
        } => branch(
            tree(graph, *condition, parameters),
            tree(graph, *then, parameters),
            tree(graph, *else_, parameters),
        ),
        other => panic!("unexpected expression node: {other:?}"),
    }
}

fn assert_structure(project: &Project, multi: bool, expected: &Tree) {
    let bindings = &project.root.bindings;
    assert_eq!(
        bindings
            .iter()
            .map(|b| b.target_field.as_str())
            .collect::<Vec<_>>(),
        if multi {
            vec!["Choice", "Marker"]
        } else {
            vec!["Choice"]
        }
    );
    let choice = bindings
        .iter()
        .find(|b| b.target_field == "Choice")
        .unwrap()
        .node;
    if multi {
        assert!(project.user_functions.is_empty());
        assert_eq!(tree(&project.graph, choice, &[]), *expected);
        let marker = bindings
            .iter()
            .find(|b| b.target_field == "Marker")
            .unwrap()
            .node;
        assert_eq!(tree(&project.graph, marker, &[]), text("D"));
    } else {
        let Node::UserFunctionCall { function, args } = &project.graph.nodes[&choice] else {
            panic!("single-output definition must remain callable");
        };
        assert_eq!(project.user_functions.len(), 1);
        let function = &project.user_functions[function];
        assert_eq!(
            (function.library.as_str(), function.name.as_str()),
            ("example", "Choose")
        );
        assert_eq!(function.output_name, "Choice");
        assert_eq!(function.output_type, ScalarType::String);
        assert_eq!(
            function
                .parameters
                .iter()
                .map(|p| (p.name.as_str(), p.ty))
                .collect::<BTreeMap<_, _>>(),
            BTreeMap::from([("First", ScalarType::Bool), ("Second", ScalarType::Bool)])
        );
        assert_eq!(function.parameters.len(), args.len());
        // Saved call-port order deliberately differs from declaration order.
        assert_eq!(
            function
                .parameters
                .iter()
                .zip(args)
                .map(|(p, id)| (p.name.as_str(), tree(&project.graph, *id, &[])))
                .collect::<BTreeMap<_, _>>(),
            BTreeMap::from([("First", variable("First")), ("Second", variable("Second"))])
        );
        assert_eq!(
            tree(&function.body, function.output, &function.parameters),
            *expected
        );
    }
    backend::assert_structure(project, expected);
}

fn input(first: bool, second: bool) -> Instance {
    Instance::Group(
        vec![
            ("First".into(), Instance::Scalar(Value::Bool(first))),
            ("Second".into(), Instance::Scalar(Value::Bool(second))),
        ]
        .into(),
    )
}

fn assert_values(project: &Project, multi: bool, cases: &[(bool, bool, Option<&str>)]) {
    for &(first, second, choice) in cases {
        let mut fields = vec![(
            "Choice".into(),
            Instance::Scalar(choice.map_or(Value::Null, |value| Value::String(value.into()))),
        )];
        if multi {
            fields.push(("Marker".into(), Instance::Scalar(Value::String("D".into()))));
        }
        let expected = Instance::Group(fields.into());
        assert_eq!(
            engine::run(project, &input(first, second)).unwrap(),
            expected,
            "First={first}, Second={second}"
        );
    }
}

const FULL: &[Option<u32>] = &[Some(1000), Some(1002), Some(1001), Some(1003), Some(1004)];

#[test]
fn scalar_udf_preserves_ordered_cases_default_and_both_output_routes_twice() {
    let expected = branch(
        variable("First"),
        text("A"),
        branch(variable("Second"), text("B"), text("C")),
    );
    for multi in [false, true] {
        let dir = Directory::new();
        for project in twice(&dir, multi, FULL, None) {
            assert_structure(&project, multi, &expected);
            assert_values(
                &project,
                multi,
                &[
                    (true, true, Some("A")),
                    (true, false, Some("A")),
                    (false, true, Some("B")),
                    (false, false, Some("C")),
                ],
            );
        }
    }
}

#[test]
fn scalar_udf_keeps_binary_even_keyless_and_incomplete_pin_semantics_twice() {
    type PinVariant<'a> = (&'a [Option<u32>], Tree, [Option<&'a str>; 4]);
    let null = Tree::Literal(Value::Null);
    let variants: &[PinVariant<'_>] = &[
        (
            &[Some(1000), Some(1002), Some(1004)],
            branch(variable("First"), text("A"), text("C")),
            [Some("A"), Some("A"), Some("C"), Some("C")],
        ),
        (
            &[Some(1000), Some(1002), Some(1001), Some(1003)],
            branch(
                variable("First"),
                text("A"),
                branch(variable("Second"), text("B"), null.clone()),
            ),
            [Some("A"), Some("A"), Some("B"), None],
        ),
        (
            &[Some(1000), Some(1002), Some(1001), None, Some(1004)],
            branch(
                variable("First"),
                text("A"),
                branch(variable("Second"), null.clone(), text("C")),
            ),
            [Some("A"), Some("A"), None, Some("C")],
        ),
        (&[], null.clone(), [None; 4]),
        (&[Some(1000)], null, [None; 4]),
    ];
    for multi in [false, true] {
        for (feeds, expected, values) in variants {
            let dir = Directory::new();
            for project in twice(&dir, multi, feeds, None) {
                assert_structure(&project, multi, expected);
                assert_values(
                    &project,
                    multi,
                    &[
                        (true, true, values[0]),
                        (true, false, values[1]),
                        (false, true, values[2]),
                        (false, false, values[3]),
                    ],
                );
            }
        }
    }
}

#[test]
fn selected_late_value_or_default_retains_typed_failure_and_other_branches_are_lazy_twice() {
    for multi in [false, true] {
        for (bad, literal, second) in [(1003, "B", true), (1004, "C", false)] {
            let failure = Tree::Call("format_guid_string".into(), vec![text(literal)]);
            let expected = branch(
                variable("First"),
                text("A"),
                branch(
                    variable("Second"),
                    if second { failure.clone() } else { text("B") },
                    if second { text("C") } else { failure.clone() },
                ),
            );
            let dir = Directory::new();
            for project in twice(&dir, multi, FULL, Some(bad)) {
                assert_structure(&project, multi, &expected);
                assert_values(
                    &project,
                    multi,
                    &[
                        (true, true, Some("A")),
                        (false, !second, Some(if second { "C" } else { "B" })),
                    ],
                );
                let error = engine::run(&project, &input(false, second)).unwrap_err();
                let source = match (multi, error) {
                    (true, engine::EngineError::Function(source)) => source,
                    (
                        false,
                        engine::EngineError::UserFunctionBuiltin {
                            function,
                            node,
                            source,
                        },
                    ) => {
                        let function = &project.user_functions[&function];
                        assert_eq!(function.name, "Choose");
                        assert_eq!(tree(&function.body, node, &function.parameters), failure);
                        source
                    }
                    (_, other) => panic!("unexpected selected-branch failure: {other:?}"),
                };
                assert_eq!(
                    source,
                    functions::FunctionError::InvalidArgument {
                        function: "format_guid_string",
                        message: "requires exactly 32 ASCII hexadecimal characters",
                    }
                );
            }
        }
    }
}

// Independently authored definitions are saved caller-first: ChooseNested ->
// Pick -> Join. Every call binds by componentid, not by declaration/port order.
fn nested_fixture(dir: &Directory, multi: bool, bad: Option<NestedFailure>) -> PathBuf {
    fixtures::nested_fixture(&dir.0, multi, bad)
}

fn nested_twice(dir: &Directory, multi: bool, bad: Option<NestedFailure>) -> [Project; 2] {
    let first = strict(&nested_fixture(dir, multi, bad));
    let path = dir.0.join("nested-second.mfd");
    let report = mfd::export_with_profile(&first, &path, mfd::ExportProfile::NativeMfd).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    [first, strict(&path)]
}

// Separate oracle for resolving first-class calls at any expression position.
// This fixture uses only booleans and strings. Check their types when binding
// parameter IDs; leave the older oracle and route/identity assertions unchanged.
fn resolved_tree(
    project: &Project,
    graph: &Graph,
    id: NodeId,
    parameters: &BTreeMap<mapping::FunctionParameterId, (ScalarType, Tree)>,
) -> (ScalarType, Tree) {
    match &graph.nodes[&id] {
        Node::Const { value } => (
            match value {
                Value::Bool(_) => ScalarType::Bool,
                Value::String(_) => ScalarType::String,
                other => panic!("unexpected nested fixture literal: {other:?}"),
            },
            Tree::Literal(value.clone()),
        ),
        Node::SourceField { path, frame: None }
            if path.len() == 1 && matches!(path[0].as_str(), "First" | "Second") =>
        {
            (ScalarType::Bool, variable(&path[0]))
        }
        Node::FunctionParameter { parameter } => parameters[parameter].clone(),
        Node::Call { function, args } => {
            assert_eq!(
                args.len(),
                match function.as_str() {
                    "concat" => 2,
                    "format_guid_string" => 1,
                    other => panic!("unexpected nested fixture builtin: {other}"),
                }
            );
            let args = args
                .iter()
                .map(|id| resolved_tree(project, graph, *id, parameters))
                .map(|(ty, tree)| {
                    assert_eq!(ty, ScalarType::String);
                    tree
                })
                .collect();
            (ScalarType::String, Tree::Call(function.clone(), args))
        }
        Node::If {
            condition,
            then,
            else_,
        } => {
            let (condition_type, condition) = resolved_tree(project, graph, *condition, parameters);
            let (then_type, then) = resolved_tree(project, graph, *then, parameters);
            let (else_type, else_) = resolved_tree(project, graph, *else_, parameters);
            assert_eq!(condition_type, ScalarType::Bool);
            assert_eq!(then_type, else_type);
            (then_type, branch(condition, then, else_))
        }
        Node::UserFunctionCall { function, args } => {
            let function = &project.user_functions[function];
            assert_eq!(function.parameters.len(), args.len());
            let arguments = function
                .parameters
                .iter()
                .zip(args)
                .map(|(parameter, id)| {
                    let argument = resolved_tree(project, graph, *id, parameters);
                    assert_eq!(parameter.ty, argument.0);
                    (parameter.id, argument)
                })
                .collect::<BTreeMap<_, _>>();
            assert_eq!(arguments.len(), function.parameters.len());
            let result = resolved_tree(project, &function.body, function.output, &arguments);
            assert_eq!(function.output_type, result.0);
            result
        }
        other => panic!("unexpected resolved expression node: {other:?}"),
    }
}

fn concat(head: Tree, tail: Tree) -> Tree {
    Tree::Call("concat".into(), vec![head, tail])
}

fn nested_choice(bad: Option<NestedFailure>) -> (Tree, Tree, Tree) {
    let late = branch(
        variable("Second"),
        concat(text("B"), text("!")),
        concat(text("LATE-UNUSED"), text("!")),
    );
    let default = branch(
        variable("First"),
        concat(text("DEFAULT-UNUSED"), text("?")),
        concat(text("C"), text("?")),
    );
    let late = if matches!(bad, Some(NestedFailure::Late)) {
        Tree::Call("format_guid_string".into(), vec![late])
    } else {
        late
    };
    let default = if matches!(bad, Some(NestedFailure::Default)) {
        Tree::Call("format_guid_string".into(), vec![default])
    } else {
        default
    };
    (
        branch(
            variable("First"),
            text("A"),
            branch(variable("Second"), late.clone(), default.clone()),
        ),
        late,
        default,
    )
}

fn assert_nested_function(
    function: &mapping::UserFunction,
    name: &str,
    parameters: &[(&str, ScalarType)],
    output: &str,
    expected: &Tree,
) {
    assert_eq!(function.library, "example");
    assert_eq!(function.name, name);
    assert_eq!(function.output_name, output);
    assert_eq!(function.output_type, ScalarType::String);
    assert_eq!(
        function
            .parameters
            .iter()
            .map(|parameter| (parameter.name.as_str(), parameter.ty))
            .collect::<Vec<_>>(),
        parameters
    );
    // Registry expansion flattens nested scalar calls inside each definition.
    // Do not let the resolving oracle hide a change to this lowering route.
    assert!(
        function
            .body
            .nodes
            .values()
            .all(|node| !matches!(node, Node::UserFunctionCall { .. }))
    );
    assert_eq!(
        tree(&function.body, function.output, &function.parameters),
        *expected
    );
}

fn assert_nested_structure(project: &Project, multi: bool, expected: &Tree) {
    assert_eq!(
        project
            .root
            .bindings
            .iter()
            .map(|binding| binding.target_field.as_str())
            .collect::<Vec<_>>(),
        if multi {
            vec!["Choice", "Marker"]
        } else {
            vec!["Choice"]
        }
    );
    let functions = project
        .user_functions
        .values()
        .map(|function| (function.name.as_str(), function))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        functions.keys().copied().collect::<Vec<_>>(),
        if multi {
            vec!["Join", "Pick"]
        } else {
            vec!["ChooseNested", "Join", "Pick"]
        }
    );
    assert_eq!(functions.len(), project.user_functions.len());
    assert_nested_function(
        functions["Join"],
        "Join",
        &[("Tail", ScalarType::String), ("Head", ScalarType::String)],
        "Joined",
        &concat(variable("Head"), variable("Tail")),
    );
    assert_nested_function(
        functions["Pick"],
        "Pick",
        &[
            ("Suffix", ScalarType::String),
            ("Preferred", ScalarType::String),
            ("Gate", ScalarType::Bool),
            ("Fallback", ScalarType::String),
        ],
        "Picked",
        &branch(
            variable("Gate"),
            concat(variable("Preferred"), variable("Suffix")),
            concat(variable("Fallback"), variable("Suffix")),
        ),
    );
    let choice = project.root.bindings[0].node;
    if multi {
        assert!(matches!(project.graph.nodes[&choice], Node::If { .. }));
        assert!(
            project
                .graph
                .nodes
                .values()
                .all(|node| !matches!(node, Node::UserFunctionCall { .. }))
        );
        assert_eq!(tree(&project.graph, choice, &[]), *expected);
        let marker = project.root.bindings[1].node;
        assert_eq!(tree(&project.graph, marker, &[]), text("D"));
    } else {
        let Node::UserFunctionCall { function, args } = &project.graph.nodes[&choice] else {
            panic!("single-output outer definition must remain a first-class call");
        };
        let function = &project.user_functions[function];
        assert_nested_function(
            function,
            "ChooseNested",
            &[("Second", ScalarType::Bool), ("First", ScalarType::Bool)],
            "Choice",
            expected,
        );
        assert_eq!(args.len(), 2);
        assert_eq!(
            function
                .parameters
                .iter()
                .zip(args)
                .map(|(parameter, id)| (
                    parameter.name.as_str(),
                    parameter.ty,
                    tree(&project.graph, *id, &[]),
                ))
                .collect::<Vec<_>>(),
            vec![
                ("Second", ScalarType::Bool, variable("Second")),
                ("First", ScalarType::Bool, variable("First")),
            ]
        );
    }
    assert_eq!(
        resolved_tree(project, &project.graph, choice, &BTreeMap::new()),
        (ScalarType::String, expected.clone())
    );
    backend::assert_structure(project, expected);
}

#[test]
fn public_registry_resolves_parameterized_nested_definitions_and_both_outer_routes_twice() {
    let (expected, _, _) = nested_choice(None);
    for multi in [false, true] {
        let dir = Directory::new();
        for project in nested_twice(&dir, multi, None) {
            assert_nested_structure(&project, multi, &expected);
            assert_values(
                &project,
                multi,
                &[
                    (true, true, Some("A")),
                    (true, false, Some("A")),
                    (false, true, Some("B!")),
                    (false, false, Some("C?")),
                ],
            );
        }
    }
}

#[test]
fn public_nested_late_branch_and_default_fail_only_when_selected_twice() {
    for multi in [false, true] {
        for bad in [NestedFailure::Late, NestedFailure::Default] {
            let (expected, late, default) = nested_choice(Some(bad));
            let (selected_second, failure, other_value) = match bad {
                NestedFailure::Late => (true, late, "C?"),
                NestedFailure::Default => (false, default, "B!"),
            };
            let dir = Directory::new();
            for project in nested_twice(&dir, multi, Some(bad)) {
                assert_nested_structure(&project, multi, &expected);
                assert_values(
                    &project,
                    multi,
                    &[
                        (true, true, Some("A")),
                        (true, false, Some("A")),
                        (false, !selected_second, Some(other_value)),
                    ],
                );
                let error = engine::run(&project, &input(false, selected_second)).unwrap_err();
                let source = match (multi, error) {
                    (true, engine::EngineError::Function(source)) => source,
                    (
                        false,
                        engine::EngineError::UserFunctionBuiltin {
                            function,
                            node,
                            source,
                        },
                    ) => {
                        let function = &project.user_functions[&function];
                        assert_eq!(function.name, "ChooseNested");
                        assert_eq!(tree(&function.body, node, &function.parameters), failure);
                        source
                    }
                    (_, other) => panic!("unexpected nested selected-branch failure: {other:?}"),
                };
                assert_eq!(
                    source,
                    functions::FunctionError::InvalidArgument {
                        function: "format_guid_string",
                        message: "requires exactly 32 ASCII hexadecimal characters",
                    }
                );
            }
        }
    }
}
