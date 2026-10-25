use codegen::{
    ArtifactSet, Binding, CsvOutputPolicy, Expression, ExpressionNode, IterationPlan, Program,
    TargetScope,
};
use ir::{ScalarType, SchemaNode, Value};

use crate::composition::{ClassPart, Library};
use crate::{emit, emit_with_csv_output, literal};

/// User text resembling the class header, a project compile entry and
/// generated node declarations.
const MARKERS: &str = "public static class GeneratedMapping\n{\n    <Compile Include=\"GeneratedMapping.cs\" />\n\n    private static global::Ferrule.Runtime.FerruleValue Node_1(\n}\n";
const HEADER: &str = "public static class GeneratedMapping\n{\n";
const PARTIAL_HEADER: &str = "public static partial class GeneratedMapping\n{\n";

fn text<'a>(artifacts: &'a ArtifactSet, path: &str) -> &'a str {
    let file = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == path)
        .unwrap_or_else(|| panic!("missing artifact {path}"));
    std::str::from_utf8(&file.contents).expect("generated text is UTF-8")
}

fn compile_items(project: &str) -> Vec<&str> {
    project
        .lines()
        .filter_map(|line| line.trim().strip_prefix("<Compile Include=\""))
        .filter_map(|item| item.strip_suffix("\" />"))
        .collect()
}

/// A flat CSV-shaped mapping whose column names and constants are markers.
fn marker_program() -> Program {
    let columns = [
        "public static class GeneratedMapping\n{",
        "Node_1(",
        "Plain",
    ];
    Program {
        xml_boundary: None,
        source: SchemaNode::group("Source", Vec::new()).repeating(),
        extra_sources: Vec::new(),
        target: SchemaNode::group(
            "Row",
            columns
                .iter()
                .map(|name| SchemaNode::scalar(*name, ScalarType::String))
                .collect(),
        ),
        expressions: (0..columns.len() as u32)
            .map(|id| ExpressionNode {
                id,
                expression: Expression::Const {
                    value: Value::String(MARKERS.into()),
                },
            })
            .collect(),
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: Some(IterationPlan::source(Vec::new())),
            construction: Default::default(),
            bindings: columns
                .iter()
                .enumerate()
                .map(|(id, name)| Binding {
                    target_field: (*name).into(),
                    expression: id as u32,
                    target_domain: ScalarType::String.into(),
                    repeating: false,
                })
                .collect(),
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    }
}

#[test]
fn library_renders_class_and_project_from_parts() {
    let part = || ClassPart {
        types: "public sealed record Marker;\n\n".into(),
        members: "    private const string Text = \"}\";\n".into(),
    };
    let ordinary = Library::new(part(), false)
        .into_artifacts()
        .expect("ordinary library");
    assert_eq!(
        text(&ordinary, "GeneratedMapping.cs"),
        "namespace Ferrule.Generated;\n\npublic sealed record Marker;\n\npublic static class GeneratedMapping\n{\n    private const string Text = \"}\";\n}\n"
    );
    assert_eq!(
        compile_items(text(&ordinary, "Ferrule.Generated.csproj")),
        [
            "GeneratedMapping.cs",
            "GeneratedTargetBuilder.cs",
            "Runtime/**/*.cs"
        ]
    );

    let mut layered = Library::new(part(), false);
    layered.add_companion(
        "GeneratedMapping.First.cs",
        ClassPart::members("    // first\n".into()),
    );
    layered.extend_mapping("    // extension\n".into());
    layered.add_companion(
        "GeneratedMapping.Second.cs",
        ClassPart {
            types: "public enum Second { A }\n\n".into(),
            members: "    // second\n".into(),
        },
    );
    let layered = layered.into_artifacts().expect("layered library");
    assert_eq!(
        text(&layered, "GeneratedMapping.cs"),
        "namespace Ferrule.Generated;\n\npublic sealed record Marker;\n\npublic static partial class GeneratedMapping\n{\n    private const string Text = \"}\";\n}\n\npublic static partial class GeneratedMapping\n{\n    // extension\n}\n"
    );
    assert_eq!(
        text(&layered, "GeneratedMapping.Second.cs"),
        "namespace Ferrule.Generated;\n\npublic enum Second { A }\n\npublic static partial class GeneratedMapping\n{\n    // second\n}\n"
    );
    // The most recently layered companion is compiled directly after the mapping.
    assert_eq!(
        compile_items(text(&layered, "Ferrule.Generated.csproj")),
        [
            "GeneratedMapping.cs",
            "GeneratedMapping.Second.cs",
            "GeneratedMapping.First.cs",
            "GeneratedTargetBuilder.cs",
            "Runtime/**/*.cs"
        ]
    );
}

#[test]
fn marker_literals_and_names_survive_companion_composition() {
    let program = marker_program();
    let ordinary = emit(&program).expect("ordinary marker mapping");
    let csv =
        emit_with_csv_output(&program, &CsvOutputPolicy::default()).expect("CSV marker mapping");
    let escaped = literal::string(MARKERS);
    for (artifacts, header, absent) in [
        (&ordinary, HEADER, PARTIAL_HEADER),
        (&csv, PARTIAL_HEADER, HEADER),
    ] {
        let mapping = text(artifacts, "GeneratedMapping.cs");
        assert_eq!(mapping.matches(header).count(), 1, "one class declaration");
        assert_eq!(mapping.matches(absent).count(), 0);
        assert_eq!(
            mapping.matches(&escaped).count(),
            3,
            "every constant is unchanged"
        );
    }
    assert_eq!(
        compile_items(text(&csv, "Ferrule.Generated.csproj")),
        [
            "GeneratedMapping.cs",
            "GeneratedMapping.Csv.cs",
            "GeneratedTargetBuilder.cs",
            "Runtime/**/*.cs"
        ]
    );
    let companion = text(&csv, "GeneratedMapping.Csv.cs");
    assert_eq!(companion.matches(PARTIAL_HEADER).count(), 1);
    for name in ["public static class GeneratedMapping\n{", "Node_1("] {
        assert!(
            companion.contains(&literal::string(name)),
            "column {name:?} is unchanged"
        );
    }
}
