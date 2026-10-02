use super::*;
use ir::SchemaKind;
use mapping::{IterationOutput, NodeId, SequenceExpr};

pub(super) const SAMPLE: &str = "SuppressNAFields.mfd";
pub(super) const INPUT: &str = "PeopleIncomplete.txt";

/// Read private field names, sentinel literals and defaults from the imported
/// graph. Recompute first-seen grouping and row expressions independently of
/// engine scope/frame resolution; no sample values or outputs are embedded.
pub(super) fn assert_case(
    project: &Project,
    source: &Instance,
    source_json: &str,
    expected: &Instance,
    expected_json: &serde_json::Value,
) -> TestResult<()> {
    assert_eq!(
        project.source_options.tabular_kind,
        Some(mapping::TabularBoundaryKind::Csv)
    );
    assert_eq!(project.source_options.has_header_row, Some(true));
    assert!(project.target_options.xml_document);
    assert!(project.extra_sources.is_empty() && project.extra_targets.is_empty());
    assert!(project.failure_rules.is_empty() && project.user_functions.is_empty());
    let SchemaKind::Group {
        children: source_fields,
        ..
    } = &project.source.kind
    else {
        panic!("flat row schema");
    };
    assert_eq!(source_fields.len(), 10);
    assert!(source_fields.iter().all(|field| !field.repeating
        && matches!(
            field.kind,
            SchemaKind::Scalar {
                ty: ScalarType::String
            }
        )));
    assert_eq!(project.graph.nodes.len(), 73);
    for (kind, count) in [
        ("source", 11),
        ("constant", 30),
        ("call", 16),
        ("conditional", 16),
    ] {
        assert_eq!(
            project
                .graph
                .nodes
                .values()
                .filter(|node| match kind {
                    "source" => matches!(node, Node::SourceField { .. }),
                    "constant" => matches!(node, Node::Const { .. }),
                    "call" => matches!(node, Node::Call { .. }),
                    "conditional" => matches!(node, Node::If { .. }),
                    _ => unreachable!(),
                })
                .count(),
            count
        );
    }
    for (function, count) in [("not_equal", 12), ("exists", 4)] {
        assert_eq!(
            project
                .graph
                .nodes
                .values()
                .filter(
                    |node| matches!(node, Node::Call { function: name, .. } if name == function)
                )
                .count(),
            count
        );
    }
    let root = &project.root;
    assert!(matches!(root.iteration, ScopeIteration::None));
    assert_eq!(root.bindings.len(), 1);
    let office = only_child(root);
    let department = only_child(office);
    let person = only_child(department);
    let first = only_child(person);
    assert!(first.children.is_empty());
    for scope in [office, department, person] {
        assert_eq!(scope.source(), Some([].as_slice()));
        assert_eq!(scope.iteration_output, IterationOutput::Repeated);
    }
    assert!(office.group_by.is_some() && department.group_by.is_some());
    assert!(!person.has_grouping() && !first.has_grouping());
    assert_eq!(office.bindings.len(), 1);
    assert_eq!(department.bindings.len(), 1);
    assert_eq!(person.bindings.len(), 7);
    assert_eq!(first.bindings.len(), 1);
    assert_eq!(first.iteration_output, IterationOutput::MappedSequence);
    let SequenceExpr::Generate {
        from: None,
        to,
        item,
    } = first.sequence().unwrap()
    else {
        panic!("presence produces a range with the default lower bound");
    };
    let Node::SourceField { path, frame: None } = &project.graph.nodes[item] else {
        panic!("generated range owns one unframed scalar item");
    };
    assert!(path.is_empty());
    assert_eq!(
        project
            .graph
            .nodes
            .values()
            .filter(|node| matches!(node, Node::SourceField { path, .. } if path.is_empty()))
            .count(),
        1
    );
    let Node::If {
        condition,
        then: then_node,
        else_: else_node,
    } = project.graph.nodes[to]
    else {
        panic!("range count stays a presence conditional");
    };
    assert!(
        matches!(&project.graph.nodes[&condition], Node::Call { function, args }
        if function == "exists" && args.as_slice() == [first.bindings[0].node])
    );
    assert!(matches!(
        project.graph.nodes[&then_node],
        Node::Const {
            value: Value::Int(1)
        }
    ));
    assert!(matches!(
        project.graph.nodes[&else_node],
        Node::Const {
            value: Value::Int(0)
        }
    ));
    for scope in [root, office, department, person, first] {
        assert!(scope.filter.is_none() && scope.post_group_filter.is_none());
        assert!(
            scope.group_adjacent_by.is_none()
                && scope.group_starting_with.is_none()
                && scope.group_ending_with.is_none()
                && scope.group_into_blocks.is_none()
        );
        assert!(
            scope.sort_by.is_none() && scope.sort_then_by.is_empty() && scope.windows.is_empty()
        );
        assert!(scope.dynamic_bindings.is_empty() && scope.dynamic_children.is_empty());
    }
    let transported = format_json::from_str(source_json, &project.source)?;
    assert_eq!(
        transported, *source,
        "flat source row presence survives JSON transport"
    );
    assert_eq!(
        engine::run(project, &transported)?,
        *expected,
        "transport preserves grouped omission/default behavior"
    );
    let rows = source.as_repeated().expect("flat input rows");
    assert!(!rows.is_empty());
    let manual = manual_output(project, rows);
    assert_eq!(
        manual, *expected,
        "independent current-row grouping and lazy conditional oracle"
    );
    let manual_json: serde_json::Value =
        serde_json::from_str(&format_json::to_string(&project.target, &manual)?)?;
    assert_eq!(
        manual_json, *expected_json,
        "typed target coercion and Null omission"
    );

    let mut omissions = 0;
    let mut present = 0;
    let mut defaulted = 0;
    let mut numeric_present = 0;
    let mut scalar_omitted = 0;
    let numeric_binding = person
        .bindings
        .iter()
        .find(|binding| {
            matches!(&project.graph.nodes[&binding.node], Node::If { else_: else_node, .. }
            if matches!(project.graph.nodes[else_node], Node::Const { value: Value::Int(_) }))
        })
        .expect("one explicit numeric fallback binding");
    let Node::If {
        condition: numeric_condition,
        else_: default_id,
        ..
    } = project.graph.nodes[&numeric_binding.node]
    else {
        unreachable!()
    };
    for row in rows {
        match expression(project, *to, row, 0) {
            Value::Int(0) => omissions += 1,
            Value::Int(1) => present += 1,
            _ => panic!("presence is exactly zero or one"),
        }
        match expression(project, numeric_condition, row, 0) {
            Value::Bool(false) => defaulted += 1,
            Value::Bool(true) => numeric_present += 1,
            _ => panic!("numeric default condition is boolean"),
        }
        scalar_omitted += person
            .bindings
            .iter()
            .filter(|binding| matches!(expression(project, binding.node, row, 0), Value::Null))
            .count();
    }
    assert!(omissions > 0 && present > 0 && scalar_omitted > 0);
    assert!(
        defaulted > 0 && numeric_present > 0,
        "both numeric fallback and retained lexical values execute"
    );
    assert_json_rows(project, rows, expected_json);
    let offices = groups(project, rows.iter().collect(), office.group_by.unwrap());
    assert_eq!(
        manual
            .field(&office.target_field)
            .and_then(Instance::as_repeated)
            .unwrap()
            .len(),
        offices.len()
    );
    assert!(
        offices.iter().any(|members| groups(
            project,
            members.clone(),
            department.group_by.unwrap()
        )
        .len()
            > 1),
        "nested grouping partitions multiple current-office departments"
    );

    // These valid graph edits deliberately change one behavior each. All
    // comparisons use the frozen independent oracle, never recomputed engine
    // output as the expected value.
    let mut collapsed = project.clone();
    collapsed.root.children[0].children[0].group_by = office.group_by;
    negative(
        &collapsed,
        source,
        &manual_json,
        "wrong parent grouping key",
    )?;
    let mut unconditional = project.clone();
    unconditional.graph.nodes.insert(
        *to,
        Node::Const {
            value: Value::Int(1),
        },
    );
    negative(
        &unconditional,
        source,
        &manual_json,
        "unconditional optional group",
    )?;
    let mut inverted = project.clone();
    let Node::If { condition, .. } = project.graph.nodes[&first.bindings[0].node] else {
        panic!("optional text uses a sentinel conditional")
    };
    let Node::Call { args, .. } = &project.graph.nodes[&condition] else {
        unreachable!()
    };
    inverted.graph.nodes.insert(
        condition,
        Node::Call {
            function: "equal".into(),
            args: args.clone(),
        },
    );
    negative(
        &inverted,
        source,
        &manual_json,
        "inverted sentinel condition",
    )?;
    let mut wrong_default = project.clone();
    let Node::Const {
        value: Value::Int(default),
    } = project.graph.nodes[&default_id]
    else {
        unreachable!()
    };
    wrong_default.graph.nodes.insert(
        default_id,
        Node::Const {
            value: Value::Int(default.checked_add(1).unwrap()),
        },
    );
    negative(
        &wrong_default,
        source,
        &manual_json,
        "changed numeric fallback",
    )?;
    Ok(())
}

fn only_child(scope: &Scope) -> &Scope {
    assert_eq!(scope.children.len(), 1);
    &scope.children[0]
}

fn expression(project: &Project, id: NodeId, row: &Instance, depth: usize) -> Value {
    assert!(
        depth < project.graph.nodes.len(),
        "bounded acyclic scalar oracle"
    );
    match &project.graph.nodes[&id] {
        Node::Const { value } => value.clone(),
        Node::SourceField { path, frame: None } => {
            assert_eq!(
                path.len(),
                1,
                "ordinary expressions read one field of the selected physical row"
            );
            row.field(&path[0])
                .and_then(Instance::as_scalar)
                .cloned()
                .unwrap_or(Value::Null)
        }
        Node::If {
            condition,
            then: then_node,
            else_: else_node,
        } => match expression(project, *condition, row, depth + 1) {
            Value::Bool(true) => expression(project, *then_node, row, depth + 1),
            Value::Bool(false) => expression(project, *else_node, row, depth + 1),
            _ => panic!("conditional oracle requires an explicit boolean"),
        },
        Node::Call { function, args } if function == "not_equal" => {
            assert_eq!(args.len(), 2);
            let left = expression(project, args[0], row, depth + 1);
            let right = expression(project, args[1], row, depth + 1);
            assert!(matches!(left, Value::String(_) | Value::Null));
            assert!(matches!(right, Value::String(_)));
            Value::Bool(left != right)
        }
        Node::Call { function, args } if function == "exists" => {
            assert_eq!(args.len(), 1);
            Value::Bool(!matches!(
                expression(project, args[0], row, depth + 1),
                Value::Null
            ))
        }
        _ => panic!(
            "oracle is restricted to imported string sentinels, presence and lazy conditionals"
        ),
    }
}

fn groups<'a>(project: &Project, rows: Vec<&'a Instance>, key: NodeId) -> Vec<Vec<&'a Instance>> {
    let mut groups: Vec<(Value, Vec<&Instance>)> = Vec::new();
    for row in rows {
        let value = expression(project, key, row, 0);
        if let Some((_, members)) = groups.iter_mut().find(|(key, _)| key == &value) {
            members.push(row);
        } else {
            groups.push((value, vec![row]));
        }
    }
    groups.into_iter().map(|(_, members)| members).collect()
}

fn fields(project: &Project, scope: &Scope, row: &Instance) -> Vec<(String, Instance)> {
    scope
        .bindings
        .iter()
        .map(|binding| {
            (
                binding.target_field.clone(),
                Instance::Scalar(expression(project, binding.node, row, 0)),
            )
        })
        .collect()
}

fn manual_output(project: &Project, rows: &[Instance]) -> Instance {
    let root = &project.root;
    let office = only_child(root);
    let department = only_child(office);
    let person = only_child(department);
    let first = only_child(person);
    let SequenceExpr::Generate { to, .. } = first.sequence().unwrap() else {
        unreachable!()
    };
    let mut root_fields = fields(project, root, &rows[0]);
    let offices = groups(project, rows.iter().collect(), office.group_by.unwrap())
        .into_iter()
        .map(|members| {
            let mut office_fields = fields(project, office, members[0]);
            let departments = groups(project, members, department.group_by.unwrap())
                .into_iter()
                .map(|members| {
                    let mut department_fields = fields(project, department, members[0]);
                    let people = members
                        .into_iter()
                        .map(|row| {
                            let mut person_fields = fields(project, person, row);
                            let first_items = match expression(project, *to, row, 0) {
                                Value::Int(0) => vec![],
                                Value::Int(1) => {
                                    vec![Instance::Group((fields(project, first, row)).into())]
                                }
                                _ => panic!("bounded optional singleton range"),
                            };
                            person_fields.push((
                                first.target_field.clone(),
                                Instance::MappedSequence(first_items),
                            ));
                            Instance::Group((person_fields).into())
                        })
                        .collect();
                    department_fields
                        .push((person.target_field.clone(), Instance::Repeated(people)));
                    Instance::Group((department_fields).into())
                })
                .collect();
            office_fields.push((
                department.target_field.clone(),
                Instance::Repeated(departments),
            ));
            Instance::Group((office_fields).into())
        })
        .collect();
    root_fields.push((office.target_field.clone(), Instance::Repeated(offices)));
    Instance::Group((root_fields).into())
}

fn negative(
    project: &Project,
    source: &Instance,
    baseline: &serde_json::Value,
    label: &str,
) -> TestResult<()> {
    assert!(
        engine::validate(project).is_empty(),
        "{label}: valid deliberate graph mutation"
    );
    let changed = engine::run(project, source)?;
    let json: serde_json::Value =
        serde_json::from_str(&format_json::to_string(&project.target, &changed)?)?;
    assert_ne!(
        &json, baseline,
        "{label}: independent oracle detects the changed output"
    );
    Ok(())
}

fn assert_json_rows(project: &Project, rows: &[Instance], json: &serde_json::Value) {
    let office = only_child(&project.root);
    let department = only_child(office);
    let person = only_child(department);
    let first = only_child(person);
    let person_schema = project
        .target
        .child(&office.target_field)
        .unwrap()
        .child(&department.target_field)
        .unwrap()
        .child(&person.target_field)
        .unwrap();
    let mut mapped_people = 0;
    let office_groups = groups(project, rows.iter().collect(), office.group_by.unwrap());
    let json_offices = json[&office.target_field].as_array().unwrap();
    assert_eq!(office_groups.len(), json_offices.len());
    for (members, json_office) in office_groups.into_iter().zip(json_offices) {
        let department_groups = groups(project, members, department.group_by.unwrap());
        let json_departments = json_office[&department.target_field].as_array().unwrap();
        assert_eq!(department_groups.len(), json_departments.len());
        for (members, json_department) in department_groups.into_iter().zip(json_departments) {
            let json_people = json_department[&person.target_field].as_array().unwrap();
            assert_eq!(members.len(), json_people.len());
            for (row, json_person) in members.into_iter().zip(json_people) {
                mapped_people += 1;
                for binding in &person.bindings {
                    let value = expression(project, binding.node, row, 0);
                    let actual = json_person.get(&binding.target_field);
                    match &value {
                        Value::Null => assert!(actual.is_none(), "suppressed scalars stay absent"),
                        Value::Int(value) => {
                            assert_eq!(actual.and_then(serde_json::Value::as_i64), Some(*value))
                        }
                        Value::String(text) => {
                            match person_schema.child(&binding.target_field).unwrap().kind {
                                SchemaKind::Scalar {
                                    ty: ScalarType::String,
                                } => assert_eq!(
                                    actual.and_then(serde_json::Value::as_str),
                                    Some(text.as_str())
                                ),
                                SchemaKind::Scalar {
                                    ty: ScalarType::Int,
                                } => assert_eq!(
                                    actual.and_then(serde_json::Value::as_i64),
                                    Some(text.trim().parse().unwrap())
                                ),
                                SchemaKind::Scalar {
                                    ty: ScalarType::Float,
                                } => assert_eq!(
                                    actual.and_then(serde_json::Value::as_f64),
                                    Some(text.trim().parse().unwrap())
                                ),
                                _ => panic!("closed scalar target domain"),
                            }
                        }
                        _ => panic!("closed row scalar domain"),
                    }
                }
                let selected = expression(project, first.bindings[0].node, row, 0);
                if matches!(selected, Value::Null) {
                    assert!(
                        json_person.get(&first.target_field).is_none(),
                        "zero-item optional group is omitted"
                    );
                } else {
                    let Value::String(text) = selected else {
                        panic!("optional text domain");
                    };
                    assert_eq!(
                        json_person
                            .get(&first.target_field)
                            .and_then(|group| group.get(&first.bindings[0].target_field))
                            .and_then(serde_json::Value::as_str),
                        Some(text.as_str()),
                        "singleton optional group retains the exact current-row text"
                    );
                }
            }
        }
    }
    assert_eq!(
        mapped_people,
        rows.len(),
        "one mapped person per physical source row"
    );
}
