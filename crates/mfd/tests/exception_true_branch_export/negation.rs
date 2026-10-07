use super::*;

#[test]
fn filter_only_negation_is_omitted_but_real_call_and_message_consumers_remain_connected() {
    let dir = Directory::new("ordinary-not-uses");
    for label in ["filter-only", "real-call", "real-message"] {
        let mut original = project(Some(3));
        if label == "real-call" {
            original.graph.nodes.insert(
                8,
                Node::Call {
                    function: "not".into(),
                    args: vec![4],
                },
            );
            original.root.children[0].bindings.push(Binding {
                target_field: "Allowed".into(),
                node: 8,
            });
        } else if label == "real-message" {
            original.failure_rules[0].message = Some(4);
        }
        let before = serde_json::to_vec(&original).unwrap();
        let mut projects = vec![(dir.child(&format!("{label}-original")), original.clone())];
        projects.extend(cycle_projects(&dir, label, &original));
        assert_eq!(serde_json::to_vec(&original).unwrap(), before);
        assert!(
            matches!(original.graph.nodes.get(&4), Some(Node::Call { function, args })
            if function == "not" && args.as_slice() == [2])
        );
        for (index, (path, current)) in projects.into_iter().enumerate() {
            let expected = if label == "real-call" {
                group(vec![(
                    "Row",
                    Instance::Repeated(vec![
                        group(vec![
                            ("Result", Instance::Scalar(Value::Int(10))),
                            ("Allowed", Instance::Scalar(Value::Bool(false))),
                        ]),
                        group(vec![
                            ("Result", Instance::Scalar(Value::Int(20))),
                            ("Allowed", Instance::Scalar(Value::Bool(false))),
                        ]),
                    ]),
                )])
            } else {
                expected_rows(&[10, 20])
            };
            assert_eq!(
                capture_run(&path, "all-false", &current, &all_false()),
                Ok(expected)
            );
            assert_eq!(
                capture_run(
                    &path,
                    "late-selected",
                    &current,
                    &input(vec![
                        item(10, Value::Bool(false), "unused"),
                        item(20, Value::Bool(true), "selected-second"),
                    ])
                ),
                Err(EngineError::MappingFailure {
                    rule: 1,
                    message: Some(
                        if label == "real-message" {
                            "false"
                        } else {
                            "selected-second"
                        }
                        .into()
                    ),
                }),
            );
            let design = path.join("design.mfd");
            if index == 0 {
                continue;
            }
            assert!(
                design.is_file(),
                "every roundtrip must retain its actual design"
            );
            let xml = std::fs::read_to_string(&design).unwrap();
            let document = roxmltree::Document::parse(&xml).unwrap();
            let ordinary_nots = document
                .descendants()
                .filter(|node| {
                    node.has_tag_name("component") && node.attribute("name") == Some("logical-not")
                })
                .collect::<Vec<_>>();
            assert_eq!(
                ordinary_nots.len(),
                match label {
                    "filter-only" => 0,
                    "real-call" => 2,
                    "real-message" => 1,
                    _ => unreachable!(),
                }
            );
            for ordinary_not in ordinary_nots {
                let output = pin(ordinary_not, "targets", 0);
                assert!(
                    document.descendants().any(|vertex| {
                        vertex.has_tag_name("vertex")
                            && vertex.attribute("vertexkey") == Some(output.as_str())
                            && vertex.descendants().any(|node| node.has_tag_name("edge"))
                    }),
                    "a real scalar consumer must stay connected"
                );
            }
        }
    }
}

#[test]
fn ordinary_false_filters_and_disconnected_graph_consumers_keep_their_negation() {
    let dir = Directory::new("ordinary-not-preservation");
    for label in ["ordinary-false", "disconnected-consumer"] {
        let mut original = project(Some(3));
        if label == "ordinary-false" {
            original.failure_rules[0].selection = FailureSelection::WhenFalse { predicate: 4 };
        } else {
            // Keep a declared graph consumer even though no target evaluates it.
            original.graph.nodes.insert(
                8,
                Node::Call {
                    function: "not".into(),
                    args: vec![4],
                },
            );
        }
        let validation = engine::validate(&original);
        retain(&dir.0, &format!("{label}-validation"), &validation);
        assert!(validation.is_empty(), "{validation:?}");
        for native in [false, true] {
            let path = dir.child(&format!(
                "{label}-{}",
                if native { "native" } else { "default" }
            ));
            let design = path.join("design.mfd");
            let before = serde_json::to_vec(&original).unwrap();
            std::fs::write(path.join("project.json"), &before).unwrap();
            let input_xml = format_xml::to_string(&original.source, &all_false());
            retain(&path, "source-xml-result", &input_xml);
            std::fs::write(path.join("input.xml"), input_xml.unwrap()).unwrap();
            let result = mfd::export_with_profile(
                &original,
                &design,
                if native {
                    ExportProfile::NativeMfd
                } else {
                    ExportProfile::FerruleExtensions
                },
            );
            retain(&path, "export-result", &result);
            assert!(
                result
                    .as_ref()
                    .is_ok_and(|report| report.warnings.is_empty()),
                "{result:?}"
            );
            assert_eq!(serde_json::to_vec(&original).unwrap(), before);
            let xml = std::fs::read_to_string(&design).unwrap();
            let document = roxmltree::Document::parse(&xml).unwrap();
            let ordinary_nots = document
                .descendants()
                .filter(|node| {
                    node.has_tag_name("component") && node.attribute("name") == Some("logical-not")
                })
                .collect::<Vec<_>>();
            assert_eq!(
                ordinary_nots.len(),
                if label == "ordinary-false" { 1 } else { 2 }
            );
            let filter = component(&document, "filter");
            if label == "ordinary-false" {
                let not_output = pin(ordinary_nots[0], "targets", 0);
                assert!(edge(&document, &not_output, &pin(filter, "sources", 1)));
                assert!(edge(
                    &document,
                    &pin(filter, "targets", 1),
                    &pin(component(&document, "exception"), "sources", 0)
                ));
            } else {
                assert!(
                    ordinary_nots
                        .iter()
                        .any(|first| ordinary_nots.iter().any(|second| {
                            first != second
                                && edge(
                                    &document,
                                    &pin(*first, "targets", 0),
                                    &pin(*second, "sources", 0),
                                )
                        }))
                );
            }
            let outcome = mfd::import_with_profile(
                &design,
                &ImportOptions::default().with_package_root(&dir.0),
                ImportProfile::Executable,
            );
            match &outcome {
                Ok(outcome) => {
                    retain(&path, "import-report", &outcome.report);
                    retain(&path, "import-warnings", &outcome.imported.warnings);
                    std::fs::write(
                        path.join("imported-project.json"),
                        serde_json::to_vec_pretty(&outcome.imported.project).unwrap(),
                    )
                    .unwrap();
                }
                Err(error) => retain(&path, "import-error", error),
            }
            let outcome = outcome.unwrap();
            assert!(outcome.report.executable && outcome.report.issues.is_empty());
            assert!(outcome.imported.warnings.is_empty());
            for (index, current) in [&original, &outcome.imported.project]
                .into_iter()
                .enumerate()
            {
                assert_eq!(
                    capture_run(&path, &format!("all-false-{index}"), current, &all_false()),
                    Ok(expected_rows(&[10, 20]))
                );
                assert_eq!(
                    capture_run(
                        &path,
                        &format!("late-selected-{index}"),
                        current,
                        &input(vec![
                            item(10, Value::Bool(false), "unused"),
                            item(20, Value::Bool(true), "second-blocked"),
                        ])
                    ),
                    Err(EngineError::MappingFailure {
                        rule: 1,
                        message: Some("second-blocked".into())
                    })
                );
            }
        }
    }
    let mut malformed_complement = project(Some(3));
    malformed_complement.graph.nodes.insert(
        4,
        Node::Call {
            function: "not".into(),
            args: vec![2, 2],
        },
    );
    refusal(
        &dir.child("wrong-complement-arity"),
        "wrong-complement-arity",
        &malformed_complement,
        "found 0",
    );
}
