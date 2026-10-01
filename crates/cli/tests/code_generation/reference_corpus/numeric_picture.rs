use super::*;
use ir::SchemaKind;

pub(super) const SAMPLE: &str = "C#/FormatNumber.mfd";
pub(super) const INPUT: &str = "sales-report.xml";

/// Metadata and independent source-context assertions use the imported design,
/// never embedded sample pictures, scaling constants, row values or output bytes.
pub(super) fn assert_case(
    project: &Project,
    source: &Instance,
    source_json: &str,
    expected: &Instance,
    expected_json: &serde_json::Value,
) -> TestResult<Vec<u8>> {
    assert!(project.extra_sources.is_empty());
    assert!(project.extra_targets.is_empty());
    assert!(project.failure_rules.is_empty());
    assert!(project.user_functions.is_empty());
    assert!(project.source_options.xml_document);
    assert_eq!(
        project.target_options.tabular_kind,
        Some(mapping::TabularBoundaryKind::Csv)
    );
    assert_eq!(project.target_options.delimiter, Some(','));
    assert_eq!(project.target_options.has_header_row, Some(true));
    assert!(!project.target_options.csv_utf8_bom);
    assert!(
        !project.target.repeating,
        "CSV repetition belongs to the boundary"
    );
    let SchemaKind::Group { children, .. } = &project.target.kind else {
        panic!("numeric picture target is a flat CSV group");
    };
    assert_eq!(children.len(), 4);
    assert!(children.iter().all(|field| !field.repeating
        && !field.attribute
        && matches!(
            field.kind,
            SchemaKind::Scalar {
                ty: ScalarType::String
            }
        )));

    let ScopeIteration::Source(iteration) = &project.root.iteration else {
        panic!("numeric picture case needs source iteration");
    };
    assert_eq!(iteration.len(), 2, "multi-hop repetition stays explicit");
    assert!(!project.source.repeating);
    for depth in 1..=iteration.len() {
        assert!(schema_at(&project.source, &iteration[..depth]).repeating);
    }
    assert!(project.root.filter.is_none());
    assert!(project.root.post_group_filter.is_none());
    assert!(!project.root.has_grouping());
    assert!(project.root.sort_by.is_none());
    assert!(project.root.sort_then_by.is_empty());
    assert!(project.root.windows.is_empty());
    assert!(project.root.children.is_empty());
    assert!(project.root.dynamic_children.is_empty());
    assert!(project.root.dynamic_bindings.is_empty());
    assert_eq!(project.root.bindings.len(), children.len());

    let source_roundtrip = format_json::from_str(source_json, &project.source)?;
    assert_eq!(
        source_roundtrip, *source,
        "typed XML source survives the generated JSON bridge"
    );
    assert_eq!(
        engine::run(project, &source_roundtrip)?,
        *expected,
        "source transport preserves numeric formatting and enclosing context"
    );

    assert_eq!(project.graph.nodes.len(), 8);
    let call = |name: &str| {
        let calls = project
            .graph
            .nodes
            .iter()
            .filter_map(|(&id, node)| match node {
                Node::Call { function, args } if function == name => Some((id, args)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 1, "one {name} call in imported pipeline");
        calls[0]
    };
    let (formatter, format_args) = call("format_number");
    let (multiply, scale_args) = call("multiply");
    assert_eq!(format_args.len(), 2);
    assert_eq!(
        format_args[0], multiply,
        "formatting consumes scaled numeric input"
    );
    assert_eq!(scale_args.len(), 2);
    let picture = match &project.graph.nodes[&format_args[1]] {
        Node::Const {
            value: value @ Value::String(text),
        } if !text.is_empty() => value,
        _ => panic!("numeric picture comes from an imported nonempty constant"),
    };
    let scale_nodes = scale_args
        .iter()
        .filter_map(|id| match &project.graph.nodes[id] {
            Node::Const {
                value: Value::Float(scale),
            } => Some(*scale),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(scale_nodes.len(), 1);
    let scale = scale_nodes[0];
    assert!(scale.is_finite() && scale > 0.0);
    assert_ne!(scale, 1.0, "scaling must be observable");
    let amount_nodes = scale_args
        .iter()
        .filter_map(|id| match &project.graph.nodes[id] {
            Node::SourceField {
                path,
                frame: Some(frame),
            } => Some((path, frame)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(amount_nodes.len(), 1);
    let (amount_path, amount_frame) = amount_nodes[0];
    assert_eq!(amount_frame, iteration);
    assert_eq!(amount_path.len(), 1);
    let amount_schema = schema_at(schema_at(&project.source, amount_frame), amount_path);
    assert!(amount_schema.attribute);
    assert!(matches!(
        amount_schema.kind,
        SchemaKind::Scalar {
            ty: ScalarType::Float
        }
    ));

    let formatted_bindings = project
        .root
        .bindings
        .iter()
        .filter(|binding| binding.node == formatter)
        .collect::<Vec<_>>();
    assert_eq!(formatted_bindings.len(), 1);
    let formatted_field = &formatted_bindings[0].target_field;
    let direct = project
        .root
        .bindings
        .iter()
        .filter_map(|binding| match &project.graph.nodes[&binding.node] {
            Node::SourceField {
                path,
                frame: Some(frame),
            } => {
                assert_eq!(path.len(), 1);
                assert!(frame == iteration || frame == &iteration[..1]);
                let leaf = schema_at(schema_at(&project.source, frame), path);
                assert!(leaf.attribute);
                assert!(matches!(
                    leaf.kind,
                    SchemaKind::Scalar {
                        ty: ScalarType::String
                    }
                ));
                Some((binding, path, frame))
            }
            Node::Call { .. } if binding.node == formatter => None,
            _ => panic!("each unformatted column is an explicitly framed source field"),
        })
        .collect::<Vec<_>>();
    assert_eq!(direct.len(), 3);
    assert_eq!(
        direct
            .iter()
            .filter(|(_, _, frame)| frame.len() == 1)
            .count(),
        2,
        "both enclosing date fields must broadcast from the parent frame"
    );
    assert_eq!(
        direct
            .iter()
            .filter(|(_, _, frame)| frame.len() == 2)
            .count(),
        1
    );
    assert_eq!(
        project
            .graph
            .nodes
            .values()
            .filter(|node| matches!(node, Node::SourceField { .. }))
            .count(),
        4
    );
    assert_eq!(
        project
            .graph
            .nodes
            .values()
            .filter(|node| matches!(node, Node::Const { .. }))
            .count(),
        2
    );

    let parents = source
        .field(&iteration[0])
        .and_then(Instance::as_repeated)
        .expect("outer source periods");
    assert!(!parents.is_empty());
    let contexts = parents
        .iter()
        .flat_map(|parent| {
            parent
                .field(&iteration[1])
                .and_then(Instance::as_repeated)
                .expect("nested region records")
                .iter()
                .map(move |record| (parent, record))
        })
        .collect::<Vec<_>>();
    let rows = expected.as_repeated().expect("mapped CSV records");
    let json_rows = expected_json.as_array().expect("schema-shaped CSV records");
    assert!(!rows.is_empty());
    assert_eq!(
        rows.len(),
        contexts.len(),
        "one row for each nested source record"
    );
    assert_eq!(json_rows.len(), rows.len());
    let mut formatter_oracle = formatter_oracle(picture);
    let mut visibly_scaled = 0;
    for ((parent, record), (row, json)) in contexts.iter().zip(rows.iter().zip(json_rows)) {
        let raw = record
            .field(&amount_path[0])
            .and_then(Instance::as_scalar)
            .expect("numeric amount in each current region record");
        let Value::Float(amount) = raw else {
            panic!("typed source amount must remain a number");
        };
        let scaled = amount * scale;
        assert!(scaled.is_finite());
        let formatted = format_value(&mut formatter_oracle, Value::Float(scaled))?;
        let Value::String(text) = &formatted else {
            panic!("formatter returns a string");
        };
        assert!(!text.is_empty());
        assert_eq!(
            row.field(formatted_field).and_then(Instance::as_scalar),
            Some(&formatted),
            "formatter/scaling pipeline reads each current nested amount"
        );
        assert_eq!(
            json.get(formatted_field)
                .and_then(serde_json::Value::as_str),
            Some(text.as_str())
        );
        if formatted != format_value(&mut formatter_oracle, raw.clone())? {
            visibly_scaled += 1;
        }
        for (binding, path, frame) in &direct {
            let context = if frame.len() == 1 { parent } else { record };
            let value = context
                .field(&path[0])
                .and_then(Instance::as_scalar)
                .expect("explicit frame has its source scalar");
            assert_eq!(
                row.field(&binding.target_field)
                    .and_then(Instance::as_scalar),
                Some(value),
                "source frame broadcast and record ordering are preserved"
            );
        }
    }
    assert!(
        visibly_scaled > 0,
        "sample input distinguishes scaled and unscaled pictures"
    );

    // Generated hosts return typed JSON. This is native CSV adapter serialization
    // after generated execution, not a generated native CSV codec.
    corpus_csv_bytes(project, expected)
}

fn schema_at<'a>(root: &'a SchemaNode, path: &[String]) -> &'a SchemaNode {
    path.iter().fold(root, |node, segment| {
        node.child(segment)
            .unwrap_or_else(|| panic!("missing schema segment {segment}"))
    })
}

// The test package already depends on engine. This tiny detached oracle uses
// the imported picture with a constant input, so its value cannot read the
// mapping's source frames or scaling edge. No extra function-crate dependency.
fn formatter_oracle(picture: &Value) -> Project {
    Project {
        source: SchemaNode::group("Context", Vec::new()),
        target: SchemaNode::group(
            "Result",
            vec![SchemaNode::scalar("formatted", ScalarType::String)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (0, Node::Const { value: Value::Null }),
                (
                    1,
                    Node::Const {
                        value: picture.clone(),
                    },
                ),
                (
                    2,
                    Node::Call {
                        function: "format_number".into(),
                        args: vec![0, 1],
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "formatted".into(),
                node: 2,
            }],
            ..Scope::default()
        },
    }
}

fn format_value(oracle: &mut Project, value: Value) -> TestResult<Value> {
    oracle.graph.nodes.insert(0, Node::Const { value });
    let output = engine::run(oracle, &Instance::Group(Vec::new()))?;
    Ok(output
        .field("formatted")
        .and_then(Instance::as_scalar)
        .expect("detached scalar formatter output")
        .clone())
}
