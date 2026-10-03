use super::*;
use ir::SchemaKind;
use mapping::AggregateOp;

pub(super) const SAMPLE: &str = "OfficeAddressFromLines.mfd";
pub(super) const INPUT: &str = "BranchOfficesWithAddressLines.xml";

/// Derive private names, line indices and substring parameters at runtime.
/// The oracle selects from the exact current address/office independently of
/// aggregate context resolution; detached scalar calls share engine semantics.
pub(super) fn assert_case(
    project: &Project,
    source: &Instance,
    source_json: &str,
    expected: &Instance,
    expected_json: &serde_json::Value,
) -> TestResult<()> {
    assert!(project.source_options.xml_document);
    assert!(project.target_options.xml_document);
    assert!(project.extra_sources.is_empty());
    assert!(project.extra_targets.is_empty());
    assert!(project.failure_rules.is_empty());
    assert!(project.user_functions.is_empty());
    assert_eq!(project.graph.nodes.len(), 21);
    assert_eq!(
        project
            .graph
            .nodes
            .values()
            .filter(|node| matches!(node, Node::SourceField { .. }))
            .count(),
        5
    );
    assert_eq!(
        project
            .graph
            .nodes
            .values()
            .filter(|node| matches!(node, Node::Const { .. }))
            .count(),
        6
    );
    let aggregates = project
        .graph
        .nodes
        .values()
        .filter_map(|node| match node {
            Node::Aggregate {
                function,
                collection,
                value,
                expression,
                arg,
            } => {
                assert_eq!(*function, AggregateOp::ItemAt);
                assert!(value.is_empty());
                assert!(expression.is_none());
                let arg = arg.expect("every item-at retains its index input");
                index(project, arg);
                Some(collection)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(aggregates.len(), 5);
    assert_eq!(aggregates.iter().filter(|path| path.len() == 1).count(), 4);
    assert_eq!(aggregates.iter().filter(|path| path.len() == 2).count(), 1);
    let calls = project
        .graph
        .nodes
        .values()
        .filter_map(|node| match node {
            Node::Call { function, args } => Some((function.as_str(), args)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 5);
    for (name, count) in [
        ("substring_before", 1),
        ("substring_after", 2),
        ("substring", 2),
    ] {
        assert_eq!(
            calls
                .iter()
                .filter(|(function, _)| *function == name)
                .count(),
            count
        );
    }
    let mut split_index = None;
    let mut delimiter = None;
    for &(function, args) in &calls {
        assert!((2..=3).contains(&args.len()));
        if function == "substring" {
            assert!(
                matches!(&project.graph.nodes[&args[0]], Node::Call { function, .. }
                if function == "substring_after")
            );
            for arg in &args[1..] {
                index(project, *arg);
            }
        } else {
            assert_eq!(args.len(), 2);
            let Node::Aggregate { arg: Some(arg), .. } = project.graph.nodes[&args[0]] else {
                panic!("before/after consumes an explicit selected address line");
            };
            assert!(
                split_index.is_none_or(|previous| previous == arg),
                "all composite projections retain their shared selected-line index"
            );
            split_index = Some(arg);
            assert!(
                matches!(&project.graph.nodes[&args[1]], Node::Const { value: Value::String(value) }
                if !value.is_empty())
            );
            assert!(
                delimiter.is_none_or(|previous| previous == args[1]),
                "all before/after projections retain the same imported delimiter"
            );
            delimiter = Some(args[1]);
        }
    }

    let root = &project.root;
    assert!(matches!(root.iteration, ScopeIteration::None));
    assert_eq!(root.children.len(), 1);
    assert_eq!(root.bindings.len(), 1);
    let office = &root.children[0];
    let office_path = office.source().expect("current office iteration");
    assert_eq!(office_path.len(), 1);
    assert_eq!(office.children.len(), 1);
    assert_eq!(office.bindings.len(), 5);
    let address = &office.children[0];
    let address_path = address.source().expect("current address iteration");
    assert_eq!(address_path.len(), 1);
    assert!(address.children.is_empty());
    assert_eq!(address.bindings.len(), 4);
    let address_nodes = address
        .bindings
        .iter()
        .map(|binding| &project.graph.nodes[&binding.node])
        .collect::<Vec<_>>();
    assert_eq!(
        address_nodes
            .iter()
            .filter(|node| matches!(node, Node::Aggregate { .. }))
            .count(),
        1
    );
    assert_eq!(
        address_nodes
            .iter()
            .filter(
                |node| matches!(node, Node::Call { function, .. } if function == "substring_before")
            )
            .count(),
        1
    );
    let substring_args = address_nodes
        .iter()
        .filter_map(|node| match node {
            Node::Call { function, args } if function == "substring" => Some(args.len()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(substring_args.len(), 2);
    assert!(
        substring_args.contains(&2) && substring_args.contains(&3),
        "both open-ended and fixed-length substring projections are bound"
    );
    for scope in [root, office, address] {
        assert!(scope.filter.is_none());
        assert!(scope.post_group_filter.is_none());
        assert!(!scope.has_grouping());
        assert!(scope.sort_by.is_none());
        assert!(scope.sort_then_by.is_empty());
        assert!(scope.windows.is_empty());
        assert!(scope.dynamic_bindings.is_empty());
        assert!(scope.dynamic_children.is_empty());
    }
    let source_office_schema = schema_at(&project.source, office_path);
    let source_address_schema = schema_at(source_office_schema, address_path);
    assert!(source_office_schema.repeating && source_address_schema.repeating);
    let target_office_schema = project.target.child(&office.target_field).unwrap();
    let target_address_schema = target_office_schema.child(&address.target_field).unwrap();
    assert!(target_office_schema.repeating && target_address_schema.repeating);
    let SchemaKind::Group { children, .. } = &target_address_schema.kind else {
        panic!("address target group");
    };
    assert_eq!(children.len(), 4);
    assert_eq!(
        children
            .iter()
            .filter(|field| matches!(
                field.kind,
                SchemaKind::Scalar {
                    ty: ScalarType::String
                }
            ))
            .count(),
        3
    );
    assert_eq!(
        children
            .iter()
            .filter(|field| matches!(
                field.kind,
                SchemaKind::Scalar {
                    ty: ScalarType::Int
                }
            ))
            .count(),
        1
    );
    let location = office
        .bindings
        .iter()
        .filter(|binding| matches!(project.graph.nodes[&binding.node], Node::Aggregate { .. }))
        .collect::<Vec<_>>();
    assert_eq!(location.len(), 1);
    let location = location[0];
    let Node::Aggregate { collection, .. } = &project.graph.nodes[&location.node] else {
        unreachable!()
    };
    assert_eq!(&collection[..1], address_path);
    let line_path = &collection[1..];
    assert_eq!(line_path.len(), 1);
    assert!(
        aggregates
            .iter()
            .filter(|path| path.len() == 1)
            .all(|path| path.as_slice() == line_path)
    );
    let line_schema = schema_at(source_address_schema, line_path);
    assert!(line_schema.repeating);
    assert!(matches!(
        line_schema.kind,
        SchemaKind::Scalar {
            ty: ScalarType::String
        }
    ));

    let transported = format_json::from_str(source_json, &project.source)?;
    assert_eq!(
        transported, *source,
        "typed XML source survives JSON transport"
    );
    assert_eq!(
        engine::run(project, &transported)?,
        *expected,
        "JSON transport preserves item-at and substring outputs"
    );
    let root_binding = &root.bindings[0];
    assert_direct(
        project,
        root_binding,
        (source, source),
        None,
        expected,
        expected_json,
        &project.target,
    );
    let source_offices = rows_at(source, office_path);
    let target_offices = expected
        .field(&office.target_field)
        .and_then(Instance::as_repeated)
        .expect("mapped office sequence");
    let json_offices = expected_json
        .get(&office.target_field)
        .and_then(serde_json::Value::as_array)
        .expect("JSON office sequence");
    assert!(!source_offices.is_empty());
    assert_eq!(target_offices.len(), source_offices.len());
    assert_eq!(json_offices.len(), source_offices.len());
    let mut missing_addresses = 0;
    let mut mapped_addresses = 0;
    for (input, (output, json)) in source_offices
        .iter()
        .zip(target_offices.iter().zip(json_offices))
    {
        for binding in &office.bindings {
            if binding.node != location.node {
                assert_direct(
                    project,
                    binding,
                    (source, input),
                    Some(office_path),
                    output,
                    json,
                    target_office_schema,
                );
            }
        }
        let location_value = expression(project, location.node, input, 0)?;
        assert_value(
            output,
            json,
            &location.target_field,
            &location_value,
            target_office_schema.child(&location.target_field).unwrap(),
        );
        let source_addresses = rows_at(input, address_path);
        let target_addresses = output
            .field(&address.target_field)
            .and_then(Instance::as_repeated)
            .expect("address iteration retains an explicit empty or nonempty sequence");
        let json_addresses = json
            .get(&address.target_field)
            .and_then(serde_json::Value::as_array)
            .expect("JSON address sequence");
        assert_eq!(target_addresses.len(), source_addresses.len());
        assert_eq!(json_addresses.len(), source_addresses.len());
        if source_addresses.is_empty() {
            missing_addresses += 1;
            assert!(matches!(location_value, Value::Null));
            assert!(
                json.get(&location.target_field).is_none(),
                "a missing address collection does not create a parent scalar"
            );
            assert!(target_addresses.is_empty() && json_addresses.is_empty());
        }
        for (current, (mapped, json)) in source_addresses
            .iter()
            .zip(target_addresses.iter().zip(json_addresses))
        {
            assert!(!scalar_items(current, line_path).is_empty());
            mapped_addresses += 1;
            for binding in &address.bindings {
                let selected = expression(project, binding.node, current, 0)?;
                assert_value(
                    mapped,
                    json,
                    &binding.target_field,
                    &selected,
                    target_address_schema.child(&binding.target_field).unwrap(),
                );
            }
        }
    }
    assert!(
        mapped_addresses > 0 && missing_addresses > 0,
        "local input exercises both present and absent address collections"
    );

    // This validates the native XML adapter after interpreter execution. Both
    // generated hosts remain the existing JSON string hosts; no generated XML
    // codec or byte/typed host API coverage is claimed for this case.
    let xml = format_xml::to_string_with_options(
        &project.target,
        expected,
        &format_xml::XmlWriteOptions {
            declaration: false,
            indent: false,
            default_namespace: None,
            schema_hints: None,
        },
    )?;
    let xml_roundtrip = format_xml::from_str(&xml, &project.target)?;
    let xml_json: serde_json::Value =
        serde_json::from_str(&format_json::to_string(&project.target, &xml_roundtrip)?)?;
    // XML reads can materialize declared-but-unmapped repeating fields as empty
    // arrays. Compare every mapped field strictly, including omitted scalars.
    assert_eq!(
        mapped_json(root, &xml_json),
        mapped_json(root, expected_json),
        "native XML omission and mapped scalar adaptation agree with JSON"
    );
    Ok(())
}

fn assert_direct(
    project: &Project,
    binding: &Binding,
    contexts: (&Instance, &Instance),
    frame: Option<&[String]>,
    output: &Instance,
    json: &serde_json::Value,
    target: &SchemaNode,
) {
    let Node::SourceField {
        path,
        frame: actual,
    } = &project.graph.nodes[&binding.node]
    else {
        panic!("ordinary fields stay source projections");
    };
    assert_eq!(actual.as_deref(), frame);
    assert_eq!(path.len(), 1);
    let context = if frame.is_some() {
        contexts.1
    } else {
        contexts.0
    };
    let value = context
        .field(&path[0])
        .and_then(Instance::as_scalar)
        .expect("source scalar");
    assert_value(
        output,
        json,
        &binding.target_field,
        value,
        target.child(&binding.target_field).unwrap(),
    );
}

fn assert_value(
    output: &Instance,
    json: &serde_json::Value,
    field: &str,
    value: &Value,
    schema: &SchemaNode,
) {
    assert_eq!(
        output.field(field).and_then(Instance::as_scalar),
        Some(value),
        "current item/frame and substring composition preserve the raw scalar"
    );
    match value {
        Value::Null => assert!(json.get(field).is_none(), "absent scalars stay omitted"),
        Value::String(text)
            if matches!(
                schema.kind,
                SchemaKind::Scalar {
                    ty: ScalarType::Int
                }
            ) =>
        {
            assert_eq!(
                json.get(field).and_then(serde_json::Value::as_i64),
                Some(text.trim().parse().unwrap()),
                "integer target adaptation retains the selected lexical number"
            );
        }
        Value::String(text) => assert_eq!(
            json.get(field).and_then(serde_json::Value::as_str),
            Some(text.as_str())
        ),
        _ => panic!("unexpected scalar domain in address projection"),
    }
}

fn expression(
    project: &Project,
    id: mapping::NodeId,
    context: &Instance,
    depth: usize,
) -> TestResult<Value> {
    assert!(
        depth < project.graph.nodes.len(),
        "bounded acyclic oracle traversal"
    );
    match &project.graph.nodes[&id] {
        Node::Const { value } => Ok(value.clone()),
        Node::Aggregate {
            function: AggregateOp::ItemAt,
            collection,
            arg: Some(arg),
            ..
        } => Ok(scalar_items(context, collection)
            .get(index(project, *arg) - 1)
            .map(|value| (*value).clone())
            .unwrap_or(Value::Null)),
        Node::Call { function, args } => {
            let values = args
                .iter()
                .map(|arg| expression(project, *arg, context, depth + 1))
                .collect::<TestResult<Vec<_>>>()?;
            detached_call(function, values)
        }
        _ => panic!("address oracle accepts only the audited closed expression graph"),
    }
}

fn index(project: &Project, id: mapping::NodeId) -> usize {
    let Node::Const {
        value: Value::Float(number),
    } = project.graph.nodes[&id]
    else {
        panic!("imported scalar index/length is a numeric constant");
    };
    assert!(number.is_finite() && number.fract() == 0.0 && (1.0..=1_000_000.0).contains(&number));
    number as usize
}

fn rows_at<'a>(instance: &'a Instance, path: &[String]) -> &'a [Instance] {
    path.iter()
        .try_fold(instance, |current, part| current.field(part))
        .and_then(Instance::as_repeated)
        .unwrap_or(&[])
}

fn scalar_items<'a>(instance: &'a Instance, path: &[String]) -> Vec<&'a Value> {
    match instance {
        Instance::Repeated(items) => items
            .iter()
            .flat_map(|item| scalar_items(item, path))
            .collect(),
        Instance::Scalar(value) if path.is_empty() => vec![value],
        Instance::Group(_) if !path.is_empty() => instance
            .field(&path[0])
            .map(|next| scalar_items(next, &path[1..]))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn schema_at<'a>(root: &'a SchemaNode, path: &[String]) -> &'a SchemaNode {
    path.iter().fold(root, |schema, part| {
        schema.child(part).expect("retained schema path")
    })
}

fn detached_call(function: &str, values: Vec<Value>) -> TestResult<Value> {
    assert!((2..=3).contains(&values.len()));
    let args = (0..values.len())
        .map(|id| u32::try_from(id).unwrap())
        .collect::<Vec<_>>();
    let mut nodes = values
        .into_iter()
        .enumerate()
        .map(|(id, value)| (u32::try_from(id).unwrap(), Node::Const { value }))
        .collect::<BTreeMap<_, _>>();
    nodes.insert(
        3,
        Node::Call {
            function: function.into(),
            args,
        },
    );
    let oracle = Project {
        source: SchemaNode::group("Context", Vec::new()),
        target: SchemaNode::group(
            "Result",
            vec![SchemaNode::scalar("rendered", ScalarType::String)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph { nodes },
        root: Scope {
            bindings: vec![Binding {
                target_field: "rendered".into(),
                node: 3,
            }],
            ..Scope::default()
        },
    };
    let output = engine::run(&oracle, &Instance::Group((Vec::new()).into()))?;
    Ok(output
        .field("rendered")
        .and_then(Instance::as_scalar)
        .expect("detached scalar output")
        .clone())
}

fn mapped_json(scope: &Scope, value: &serde_json::Value) -> serde_json::Value {
    let object = value.as_object().expect("mapped group JSON");
    let mut mapped = serde_json::Map::new();
    for binding in &scope.bindings {
        if let Some(value) = object.get(&binding.target_field) {
            mapped.insert(binding.target_field.clone(), value.clone());
        }
    }
    for child in &scope.children {
        let values = object
            .get(&child.target_field)
            .and_then(serde_json::Value::as_array)
            .expect("mapped repeated child JSON");
        mapped.insert(
            child.target_field.clone(),
            serde_json::Value::Array(
                values
                    .iter()
                    .map(|value| mapped_json(child, value))
                    .collect(),
            ),
        );
    }
    serde_json::Value::Object(mapped)
}
