use super::*;

// Generated XML input does not admit mixed-content schemas, and JSON input
// materializes absent objects, so generated XmlMixedContent absence is covered
// by the codegen package tests instead.

/// One structured XML lookup owned by the active `Rows` item. `error` is the
/// generated Rust traversal failure `(path, segment, field)` for an absent
/// second row; generated C# reports the same path.
struct Variant {
    target: &'static str,
    node: Node,
    error: (&'static [&'static str], u64, &'static str),
    /// The interpreter's existing missing-source display.
    engine: &'static str,
    /// Whether the variant's absence comes from the nested-group fixture.
    nested: bool,
}

fn path(segments: &[&str]) -> Vec<String> {
    segments.iter().map(|segment| (*segment).into()).collect()
}

fn group(name: &str, fields: Vec<SchemaNode>) -> SchemaNode {
    SchemaNode::group(name, fields)
}

fn extra_schema() -> SchemaNode {
    group("Extra", vec![string("Flag")])
}

fn details_schema() -> SchemaNode {
    group("Details", vec![string("Code"), extra_schema()])
}

fn header_schema() -> SchemaNode {
    group("Header", vec![string("Code")])
}

fn serialize(frame: Option<&[&str]>, segments: &[&str], schema: SchemaNode) -> Node {
    Node::XmlSerialize {
        path: path(segments),
        frame: frame.map(path),
        schema: Box::new(schema),
        declaration: false,
        indent: false,
        namespace: None,
    }
}

fn xml_document() -> mapping::FormatOptions {
    mapping::FormatOptions {
        xml_document: true,
        ..Default::default()
    }
}

fn variants() -> Vec<Variant> {
    const DETAILS: &[&str] = &["Rows", "Details"];
    const EXTRA: &[&str] = &["Rows", "Details", "Extra"];
    vec![
        Variant {
            target: "Details",
            node: serialize(None, DETAILS, details_schema()),
            error: (DETAILS, 1, "Details"),
            engine: "Rows/Details",
            nested: false,
        },
        Variant {
            target: "FramedDetails",
            node: serialize(Some(&["Rows"]), &["Details"], details_schema()),
            error: (DETAILS, 1, "Details"),
            engine: "Rows/Details",
            nested: false,
        },
        Variant {
            target: "Extra",
            node: serialize(None, EXTRA, extra_schema()),
            error: (EXTRA, 2, "Extra"),
            engine: "Rows/Details/Extra",
            nested: true,
        },
        Variant {
            target: "FramedExtra",
            node: serialize(Some(&["Rows"]), &["Details", "Extra"], extra_schema()),
            error: (EXTRA, 2, "Extra"),
            engine: "Rows/Details/Extra",
            nested: true,
        },
    ]
}

/// Every variant is evaluated lazily for the row named `first`. Root `Fail`
/// selects one variant whose absent second-row lookup is also evaluated.
fn project(variants: &[Variant]) -> Project {
    let mut graph = Graph::default();
    let mut insert = |id, node| {
        graph.nodes.insert(id, node);
    };
    insert(
        1,
        Node::SourceField {
            path: path(&["Rows", "Name"]),
            frame: None,
        },
    );
    insert(
        2,
        Node::Const {
            value: Value::String("first".into()),
        },
    );
    insert(
        3,
        Node::Call {
            function: "equal".into(),
            args: vec![1, 2],
        },
    );
    insert(
        4,
        Node::Const {
            value: Value::String("skipped".into()),
        },
    );
    insert(
        5,
        Node::SourceField {
            path: path(&["Fail"]),
            frame: None,
        },
    );
    insert(6, serialize(None, &["Header"], header_schema()));
    let mut bindings = vec![
        Binding {
            target_field: "Name".into(),
            node: 1,
        },
        Binding {
            target_field: "Header".into(),
            node: 6,
        },
    ];
    let mut fields = vec![string("Name"), string("Header")];
    for (index, variant) in variants.iter().enumerate() {
        let base = 100 + 10 * index as u32;
        insert(base, variant.node.clone());
        insert(
            base + 1,
            Node::Const {
                value: Value::String(variant.target.into()),
            },
        );
        insert(
            base + 2,
            Node::Call {
                function: "equal".into(),
                args: vec![5, base + 1],
            },
        );
        insert(
            base + 3,
            Node::If {
                condition: base + 2,
                then: base,
                else_: 4,
            },
        );
        insert(
            base + 4,
            Node::If {
                condition: 3,
                then: base,
                else_: base + 3,
            },
        );
        bindings.push(Binding {
            target_field: variant.target.into(),
            node: base + 4,
        });
        fields.push(string(variant.target));
    }
    Project {
        source: group(
            "Source",
            vec![
                string("Fail"),
                header_schema(),
                group("Rows", vec![string("Name"), details_schema()]).repeating(),
            ],
        ),
        target: group("Target", vec![group("Out", fields).repeating()]),
        source_path: None,
        target_path: None,
        source_options: xml_document(),
        target_options: xml_document(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root: Scope {
            children: vec![Scope {
                target_field: "Out".into(),
                iteration: ScopeIteration::Source(path(&["Rows"])),
                bindings,
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn input(nested: bool, fail: Option<&str>) -> String {
    let fail = fail.map_or_else(String::new, |fail| format!("<Fail>{fail}</Fail>"));
    let second = if nested {
        "<Rows><Name>second</Name><Details><Code>B</Code></Details></Rows>"
    } else {
        "<Rows><Name>second</Name></Rows>"
    };
    format!(
        "<Source>{fail}<Header><Code>H</Code></Header><Rows><Name>first</Name>\
<Details><Code>A</Code><Extra><Flag>yes</Flag></Extra></Details></Rows>{second}</Source>"
    )
}

fn expected(variants: &[Variant]) -> Instance {
    let details = "<Details><Code>A</Code><Extra><Flag>yes</Flag></Extra></Details>";
    let extra = "<Extra><Flag>yes</Flag></Extra>";
    let first = [details, details, extra, extra];
    let row = |name: &str, values: [&str; 4]| {
        let mut fields = vec![
            (
                "Name".to_string(),
                Instance::Scalar(Value::String(name.into())),
            ),
            (
                "Header".to_string(),
                Instance::Scalar(Value::String("<Header><Code>H</Code></Header>".into())),
            ),
        ];
        for (variant, value) in variants.iter().zip(values) {
            fields.push((
                variant.target.to_string(),
                Instance::Scalar(Value::String(value.into())),
            ));
        }
        Instance::Group(fields.into())
    };
    Instance::Group(
        vec![(
            "Out".to_string(),
            Instance::Repeated(vec![row("first", first), row("second", ["skipped"; 4])]),
        )]
        .into(),
    )
}

#[test]
fn generated_structured_xml_lookups_keep_owned_absence() -> TestResult<()> {
    let variants = variants();
    let project = project(&variants);
    assert!(engine::validate(&project).is_empty());
    assert!(
        codegen::lower(&project)?.xml_boundary.is_some(),
        "the fixture must exercise generated XML input"
    );
    let mut cases = Vec::new();
    for nested in [false, true] {
        let input = input(nested, None);
        let source = format_xml::from_str(&input, &project.source)?;
        let output = engine::run(&project, &source)?;
        assert_eq!(output, expected(&variants));
        cases.push(serde_json::json!({
            "input": input,
            "expected_xml": format_xml::to_string(&project.target, &output)?,
        }));
    }
    for variant in &variants {
        let input = input(variant.nested, Some(variant.target));
        let source = format_xml::from_str(&input, &project.source)?;
        let result = engine::run(&project, &source);
        assert!(
            matches!(
                &result,
                Err(engine::EngineError::MissingSourceField(actual)) if actual == variant.engine
            ),
            "{}: {result:?}",
            variant.target
        );
        let (path, segment, field) = variant.error;
        cases.push(serde_json::json!({
            "input": input,
            "missing_source": {"path": path, "segment": segment, "field": field},
        }));
    }
    super::json_text_boundaries::run_generated_boundary_cases_with_harnesses(
        &project,
        &cases,
        "xml_owned_absence",
        include_str!("fixtures/xml_owned_absence_rust.rs.txt"),
        include_str!("fixtures/xml_owned_absence_csharp.cs.txt"),
    )
}
