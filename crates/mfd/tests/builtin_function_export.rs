//! Functions whose built-in `.mfd` equivalent differs in name, inputs or
//! meaning: export writes the native function or refuses before writing.

#[allow(dead_code, unused_imports)]
mod support;

use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FunctionId, FunctionParameter, FunctionParameterId, Graph, Node, Project, Scope,
    UserFunction,
};
use support::{ScalarContext, ScalarLiteral as Literal, ScalarMfdBuilder};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// An exported function component: name, library and connected input count.
type Component = (String, String, usize);

const CONTEXTS: [ScalarContext; 3] = [
    ScalarContext::Main,
    ScalarContext::UserDefined,
    ScalarContext::NestedUserDefined,
];

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_builtin_function_export_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(project: &Project) -> TestResult<Option<Value>> {
    let source = format_xml::from_str("<Source><Seed>fixture</Seed></Source>", &project.source)?;
    let output = engine::run(project, &source)?;
    Ok(output
        .field("Result")
        .and_then(Instance::as_scalar)
        .cloned())
}

/// Every kind-5 function component in an exported design.
fn function_components(xml: &str) -> Result<Vec<Component>, roxmltree::Error> {
    let document = roxmltree::Document::parse(xml)?;
    Ok(document
        .descendants()
        .filter(|node| node.has_tag_name("component") && node.attribute("kind") == Some("5"))
        .map(|node| {
            let inputs = node
                .children()
                .find(|child| child.has_tag_name("sources"))
                .map_or(0, |sources| {
                    sources
                        .children()
                        .filter(|pin| pin.has_tag_name("datapoint"))
                        .count()
                });
            (
                node.attribute("name").unwrap_or_default().to_string(),
                node.attribute("library").unwrap_or_default().to_string(),
                inputs,
            )
        })
        .collect())
}

/// Imports a one-function scenario, runs it, exports it, and checks that the
/// re-imported design gives the same result. Returns the result and the
/// exported function components.
fn round_trip(
    tag: &str,
    function: &str,
    library: &str,
    arguments: Vec<Literal>,
    output_type: &str,
    context: ScalarContext,
) -> TestResult<(Option<Value>, Vec<Component>)> {
    let fixture = ScalarMfdBuilder::new(tag, function, library, arguments, output_type)
        .context(context)
        .write()?;
    let imported = mfd::import(fixture.design())?;
    assert!(
        imported.warnings.is_empty(),
        "{tag} {context:?}: {:?}",
        imported.warnings
    );
    assert!(engine::validate(&imported.project).is_empty(), "{tag}");
    let value = run(&imported.project)?;

    let exported = fixture.design().with_file_name("roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &exported)?;
    assert!(warnings.is_empty(), "{tag} {context:?}: {warnings:?}");
    let xml = std::fs::read_to_string(&exported)?;
    assert!(xml.contains("SelectedLanguage=\"builtin\""), "{tag}");
    let reimported = mfd::import(&exported)?;
    assert!(
        reimported.warnings.is_empty(),
        "{tag} {context:?}: {:?}",
        reimported.warnings
    );
    assert_eq!(run(&reimported.project)?, value, "{tag} {context:?}");
    Ok((value, function_components(&xml)?))
}

#[test]
fn lang_positive_tests_for_numbers_above_zero_and_round_trips() -> TestResult {
    // Results observed natively for the same inputs.
    let cases = [
        (Literal::Decimal(-2.5), false),
        (Literal::Integer(0), false),
        (Literal::Integer(3), true),
        (Literal::string("-0"), false),
        (Literal::string("+5"), true),
        (Literal::string(" 5"), true),
        (Literal::string("-0.5"), false),
    ];
    for context in CONTEXTS {
        for (index, (argument, expected)) in cases.iter().enumerate() {
            let (value, functions) = round_trip(
                &format!("lang_positive_{index}"),
                "positive",
                "lang",
                vec![argument.clone()],
                "boolean",
                context,
            )?;
            assert_eq!(
                value,
                Some(Value::Bool(*expected)),
                "{argument:?} {context:?}"
            );
            let names = functions
                .iter()
                .map(|(name, library, _)| (name.as_str(), library.as_str()))
                .collect::<Vec<_>>();
            assert!(
                names.contains(&("greater", "core")) && names.contains(&("add", "core")),
                "{context:?}: {names:?}"
            );
            assert!(
                !names.iter().any(|(name, _)| *name == "positive"),
                "{context:?}: {names:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn lang_replace_substitutes_literal_text_and_round_trips() -> TestResult {
    // Results observed natively for the same inputs.
    let cases = [
        ("a.c abc", ".", "-", "a-c abc"),
        ("1*2(3)$4\\5", "*", "x", "1x2(3)$4\\5"),
        ("1*2(3)$4\\5", "(", "[", "1*2[3)$4\\5"),
        ("1*2(3)$4\\5", "$", "D", "1*2(3)D4\\5"),
        ("1*2(3)$4\\5", "\\", "/", "1*2(3)$4/5"),
        ("abc", "b", "$0$1\\", "a$0$1\\c"),
        ("aaa", "aa", "b", "ba"),
        ("a.b", ".", "", "ab"),
        ("ABC", "b", "x", "ABC"),
        ("a.b.c", ".", "..", "a..b..c"),
    ];
    for context in CONTEXTS {
        for (index, (value, search, replacement, expected)) in cases.into_iter().enumerate() {
            let (result, functions) = round_trip(
                &format!("lang_replace_{index}"),
                "replace",
                "lang",
                vec![
                    Literal::string(value),
                    Literal::string(search),
                    Literal::string(replacement),
                ],
                "string",
                context,
            )?;
            assert_eq!(
                result,
                Some(Value::String(expected.into())),
                "{value} {search} {replacement} {context:?}"
            );
            assert!(
                functions.contains(&("replace".into(), "lang".into(), 3))
                    && functions
                        .iter()
                        .all(|(name, library, _)| name != "replace" || library == "lang"),
                "{context:?}: {functions:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn two_argument_round_exports_as_round_precision() -> TestResult {
    for context in CONTEXTS {
        let (value, functions) = round_trip(
            "round_precision",
            "round-precision",
            "core",
            vec![Literal::Decimal(2.777777), Literal::Integer(2)],
            "decimal",
            context,
        )?;
        assert_eq!(value, Some(Value::Float(2.78)), "{context:?}");
        assert!(
            functions.contains(&("round-precision".into(), "core".into(), 2)),
            "{context:?}: {functions:?}"
        );
        assert!(
            !functions.iter().any(|(name, _, _)| name == "round"),
            "{context:?}: {functions:?}"
        );
    }
    let (value, functions) = round_trip(
        "round",
        "round",
        "core",
        vec![Literal::Decimal(10.5)],
        "decimal",
        ScalarContext::Main,
    )?;
    assert_eq!(value, Some(Value::Float(11.0)));
    assert_eq!(functions, [("round".into(), "core".into(), 1)]);
    Ok(())
}

#[test]
fn imported_rounding_matches_the_reference_and_round_trips() -> TestResult {
    // The first three rows are native results for these inputs (#1264).
    let cases = [
        (
            "round-precision",
            vec![Literal::Decimal(-0.125), Literal::Integer(2)],
            Value::Float(-0.12),
        ),
        ("round", vec![Literal::Decimal(-10.5)], Value::Float(-10.0)),
        (
            "round-precision",
            vec![Literal::string("1.005"), Literal::Integer(2)],
            Value::Float(1.01),
        ),
        ("round", vec![Literal::Decimal(2.5)], Value::Float(3.0)),
        ("round", vec![Literal::Decimal(-2.51)], Value::Float(-3.0)),
        (
            "round-precision",
            vec![Literal::Decimal(-2.5), Literal::Integer(0)],
            Value::Float(-2.0),
        ),
        (
            "round-precision",
            vec![Literal::Decimal(-1250.0), Literal::Integer(-2)],
            Value::Float(-1200.0),
        ),
        (
            "round-precision",
            vec![
                Literal::Decimal(4_503_599_627_370_495.5),
                Literal::Integer(0),
            ],
            Value::Float(4_503_599_627_370_496.0),
        ),
    ];
    for context in CONTEXTS {
        for (index, (function, arguments, expected)) in cases.iter().enumerate() {
            let function = *function;
            let fixture = ScalarMfdBuilder::new(
                format!("reference_round_{index}"),
                function,
                "core",
                arguments.clone(),
                "decimal",
            )
            .context(context)
            .write()?;
            let imported = mfd::import(fixture.design())?;
            let calls = imported
                .project
                .user_functions
                .values()
                .flat_map(|function| function.body.nodes.values())
                .chain(imported.project.graph.nodes.values())
                .filter_map(|node| match node {
                    Node::Call { function, .. } if function.starts_with("round") => {
                        Some(function.as_str())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            // A nested definition can also be retained beside its inlined copy.
            assert!(
                !calls.is_empty() && calls.iter().all(|call| *call == "round_half_ceiling"),
                "{function} {context:?}: {calls:?}"
            );

            let (value, functions) = round_trip(
                &format!("reference_round_{index}"),
                function,
                "core",
                arguments.clone(),
                "decimal",
                context,
            )?;
            assert_eq!(
                value.as_ref(),
                Some(expected),
                "{function} {arguments:?} {context:?}"
            );
            let exported = functions
                .iter()
                .filter(|(name, _, _)| name.starts_with("round"))
                .collect::<Vec<_>>();
            let native = (function.to_string(), "core".to_string(), arguments.len());
            assert!(
                !exported.is_empty() && exported.iter().all(|component| **component == native),
                "{context:?}: {exported:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn native_rounding_function_exports_under_both_arities() -> TestResult {
    let number = |value| Node::Const {
        value: Value::Float(value),
    };
    let precision = Node::Const {
        value: Value::Int(2),
    };
    for (arguments, name, expected) in [
        (vec![number(-10.5)], "round", Value::Float(-10.0)),
        (
            vec![number(-0.125), precision],
            "round-precision",
            Value::Float(-0.12),
        ),
    ] {
        let native = project(
            call_graph("round_half_ceiling", &arguments),
            ScalarType::Float,
        );
        assert_eq!(run(&native)?, Some(expected.clone()));
        let directory = TempDir::new()?;
        let path = directory.0.join("mapping.mfd");
        let warnings = mfd::export(&native, &path)?;
        assert!(warnings.is_empty(), "{warnings:?}");
        let components = function_components(&std::fs::read_to_string(&path)?)?;
        assert_eq!(components, [(name.into(), "core".into(), arguments.len())]);
        let reimported = mfd::import(&path)?;
        assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
        assert_eq!(run(&reimported.project)?, Some(expected), "{name}");
    }
    Ok(())
}

#[test]
fn format_functions_export_their_native_inputs() -> TestResult {
    for (function, value, picture, expected) in [
        (
            "format-date",
            "2010-12-01",
            "[D01] [MNn] [Y]",
            "01 December 2010",
        ),
        (
            "format-dateTime",
            "2010-12-01T15:02:39",
            "[H01]:[m01] [D01] [MNn] [Y]",
            "15:02 01 December 2010",
        ),
    ] {
        let (result, functions) = round_trip(
            function,
            function,
            "core",
            vec![
                Literal::string(value),
                Literal::string(picture),
                Literal::string(""),
            ],
            "string",
            ScalarContext::Main,
        )?;
        assert_eq!(result, Some(Value::String(expected.into())), "{function}");
        assert_eq!(functions, [(function.to_string(), "core".into(), 3)]);
    }
    Ok(())
}

fn text(value: &str) -> Node {
    Node::Const {
        value: Value::String(value.into()),
    }
}

/// A main graph or user-defined function body calling `function` on constants.
fn call_graph(function: &str, arguments: &[Node]) -> Graph {
    let mut nodes = BTreeMap::new();
    let mut args = Vec::new();
    for (id, argument) in (1..).zip(arguments) {
        nodes.insert(id, argument.clone());
        args.push(id);
    }
    nodes.insert(
        100,
        Node::Call {
            function: function.into(),
            args,
        },
    );
    Graph { nodes }
}

fn project(graph: Graph, output: ScalarType) -> Project {
    Project {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::scalar("Seed", ScalarType::String)],
        ),
        target: SchemaNode::group("Target", vec![SchemaNode::scalar("Result", output)]),
        source_path: Some("source.xml".into()),
        target_path: Some("target.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root: Scope {
            bindings: vec![Binding {
                target_field: "Result".into(),
                node: 100,
            }],
            ..Scope::default()
        },
    }
}

/// The same call inside a user-defined function that the main graph calls.
fn function_project(body: Graph, output: ScalarType) -> Project {
    let mut main = project(Graph::default(), output);
    main.graph.nodes.insert(1, text("unused"));
    main.graph.nodes.insert(
        100,
        Node::UserFunctionCall {
            function: FunctionId::new(1),
            args: vec![1],
        },
    );
    let mut body = body;
    body.nodes.insert(
        99,
        Node::FunctionParameter {
            parameter: FunctionParameterId::new(1),
        },
    );
    main.user_functions.insert(
        FunctionId::new(1),
        UserFunction {
            library: "user".into(),
            name: "Wrapped".into(),
            description: None,
            parameters: vec![FunctionParameter {
                id: FunctionParameterId::new(1),
                name: "unused".into(),
                ty: ScalarType::String,
            }],
            output_name: "result".into(),
            output_type: output,
            body,
            output: 100,
        },
    );
    main
}

fn assert_refused(project: &Project, function: &str, reason: &str, label: &str) -> TestResult {
    let directory = TempDir::new()?;
    let path = directory.0.join("mapping.mfd");
    for error in [
        mfd::preflight_export(project, &path)
            .map(|_| ())
            .unwrap_err(),
        mfd::export(project, &path).map(|_| ()).unwrap_err(),
        mfd::export_with_profile(project, &path, mfd::ExportProfile::NativeMfd)
            .map(|_| ())
            .unwrap_err(),
    ] {
        let mfd::MfdError::Unsupported(message) = &error else {
            panic!("{label}: {error:?}");
        };
        assert!(
            message.contains(&format!("`{function}`"))
                && message.contains("no built-in .mfd equivalent")
                && message.contains(reason),
            "{label}: {message}"
        );
    }
    assert_eq!(std::fs::read_dir(&directory.0)?.count(), 0, "{label}");
    Ok(())
}

#[test]
fn functions_without_built_in_equivalents_are_refused_before_writing() -> TestResult {
    let date = || text("2010-12-01");
    let picture = || text("[D01] [MNn] [Y]");
    let number = |value| Node::Const {
        value: Value::Float(value),
    };
    let cases: [(&str, Vec<Node>, ScalarType, &str); 10] = [
        (
            "round",
            vec![number(-10.5)],
            ScalarType::Float,
            "use `round_half_ceiling`",
        ),
        (
            "round",
            vec![number(1.005), number(2.0)],
            ScalarType::Float,
            "halves away from zero",
        ),
        (
            "positive",
            vec![Node::Const {
                value: Value::Int(3),
            }],
            ScalarType::Int,
            "greater than zero",
        ),
        (
            "trim",
            vec![text(" a ")],
            ScalarType::String,
            "Unicode whitespace",
        ),
        (
            "ends_with",
            vec![text("abc"), text("c")],
            ScalarType::Bool,
            "XSLT 2.0",
        ),
        (
            "matches",
            vec![text("abc"), text("b")],
            ScalarType::Bool,
            "whole text",
        ),
        (
            "replace",
            vec![text("abc"), text("b"), text("x")],
            ScalarType::String,
            "literal text",
        ),
        (
            "format_date",
            vec![date(), picture(), text(""), text("")],
            ScalarType::String,
            "calendar and place",
        ),
        (
            "format_datetime",
            vec![
                text("2010-12-01T15:02:39"),
                picture(),
                text(""),
                text(""),
                text(""),
            ],
            ScalarType::String,
            "calendar and place",
        ),
        (
            "format_time",
            vec![text("15:02:39"), text("[H01]:[m01]"), text("")],
            ScalarType::String,
            "value and format",
        ),
    ];
    for (function, arguments, output, reason) in cases {
        let main = project(call_graph(function, &arguments), output);
        assert!(engine::validate(&main).is_empty(), "{function}");
        assert_refused(&main, function, reason, &format!("{function} in main"))?;
        assert_refused(
            &function_project(call_graph(function, &arguments), output),
            function,
            reason,
            &format!("{function} in a user-defined function"),
        )?;
    }
    Ok(())
}

#[test]
fn refusal_names_the_owning_user_defined_function() -> TestResult {
    let project = function_project(call_graph("trim", &[text(" a ")]), ScalarType::String);
    let directory = TempDir::new()?;
    let error = mfd::export(&project, &directory.0.join("mapping.mfd")).unwrap_err();
    assert!(
        matches!(&error, mfd::MfdError::Unsupported(message) if message.contains("`Wrapped`")),
        "{error:?}"
    );
    assert!(Path::new(&directory.0).read_dir()?.next().is_none());
    Ok(())
}
