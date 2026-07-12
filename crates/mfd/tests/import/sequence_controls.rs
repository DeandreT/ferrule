use super::*;

#[test]
fn group_starting_with_imports_executes_and_roundtrips() {
    let imported = mfd::import(&fixture("group-starting.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let group = imported
        .project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Group")
        .unwrap();
    assert!(group.group_by.is_none());
    assert!(group.group_into_blocks.is_none());
    let predicate = group.group_starting_with.unwrap();
    assert!(matches!(
        &imported.project.graph.nodes[&predicate],
        Node::SourceField { path, frame }
            if path == &["Start"]
                && frame.as_deref().is_some_and(|frame| frame.len() == 1 && frame[0] == "Row")
    ));

    let source =
        format_xml::read(&fixture("group-starting.xml"), &imported.project.source).unwrap();
    let output = engine::run(&imported.project, &source).unwrap();
    let groups = output
        .field("Group")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(groups.len(), 3);
    assert_eq!(scalar(&groups[0], "First"), Value::String("alpha".into()));
    assert_eq!(scalar(&groups[0], "Count"), Value::Int(1));
    assert_eq!(scalar(&groups[1], "First"), Value::String("beta".into()));
    assert_eq!(scalar(&groups[1], "Count"), Value::Int(2));
    assert_eq!(scalar(&groups[2], "First"), Value::String("delta".into()));
    assert_eq!(scalar(&groups[2], "Count"), Value::Int(1));

    let dir = TempDir::new("group_starting");
    let out = dir.0.join("group-starting.mfd");
    let warnings = mfd::export(&imported.project, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported = std::fs::read_to_string(&out).unwrap();
    assert_eq!(exported.matches("name=\"group-starting-with\"").count(), 1);
    let document = roxmltree::Document::parse(&exported).unwrap();
    let component = document
        .descendants()
        .find(|node| {
            node.has_tag_name("component") && node.attribute("name") == Some("group-starting-with")
        })
        .unwrap();
    let input_keys: Vec<&str> = component
        .descendants()
        .filter(|node| node.has_tag_name("sources"))
        .flat_map(|sources| sources.children())
        .filter(|node| node.has_tag_name("datapoint"))
        .filter_map(|node| node.attribute("key"))
        .collect();
    assert_eq!(input_keys.len(), 2);
    assert!(input_keys.iter().all(|input| {
        document
            .descendants()
            .any(|node| node.has_tag_name("edge") && node.attribute("vertexkey") == Some(*input))
    }));

    let reimported = mfd::import(&out).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    assert!(
        reimported.project.root.children[0]
            .group_starting_with
            .is_some()
    );
    assert_eq!(engine::run(&reimported.project, &source).unwrap(), output);
}

#[test]
fn malformed_group_starting_predicates_skip_the_affected_iteration() {
    let original = std::fs::read_to_string(fixture("group-starting.mfd")).unwrap();
    let variants = [
        (
            "missing",
            original.replace("      <edge from=\"12\" to=\"31\"/>\n", ""),
        ),
        (
            "dangling",
            original.replace("from=\"12\" to=\"31\"", "from=\"999\" to=\"31\""),
        ),
    ];
    for (name, design) in variants {
        let dir = TempDir::new(&format!("group_starting_{name}"));
        for schema in ["group-starting-source.xsd", "group-starting-target.xsd"] {
            std::fs::copy(fixture(schema), dir.0.join(schema)).unwrap();
        }
        let path = dir.0.join("group-starting.mfd");
        std::fs::write(&path, design).unwrap();
        let imported = mfd::import(&path).unwrap();
        assert_eq!(
            imported.warnings.len(),
            1,
            "{name}: {:?}",
            imported.warnings
        );
        assert!(
            imported.warnings[0].contains(
                "group-starting-with feeding `Group` has a missing or unsupported predicate; iteration skipped"
            ),
            "{name}: {:?}",
            imported.warnings
        );
        assert!(
            imported.project.root.children.is_empty(),
            "{name}: skipped iteration left a singular target scope"
        );
    }
}

#[test]
fn group_into_blocks_imports_executes_and_roundtrips() {
    let imported = mfd::import(&fixture("group-blocks.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let block = imported
        .project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Block")
        .unwrap();
    assert!(block.group_by.is_none());
    let size = block.group_into_blocks.unwrap();
    assert!(matches!(
        imported.project.graph.nodes[&size],
        Node::Const {
            value: Value::Int(2)
        }
    ));

    let source = format_xml::read(&fixture("group-blocks.xml"), &imported.project.source).unwrap();
    let output = engine::run(&imported.project, &source).unwrap();
    let blocks = output
        .field("Block")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(blocks.len(), 2);
    assert_eq!(scalar(&blocks[0], "First"), Value::String("alpha".into()));
    assert_eq!(scalar(&blocks[0], "Count"), Value::Int(2));
    assert_eq!(scalar(&blocks[1], "First"), Value::String("gamma".into()));
    assert_eq!(scalar(&blocks[1], "Count"), Value::Int(1));

    let dir = TempDir::new("group_blocks");
    let out = dir.0.join("group-blocks.mfd");
    let warnings = mfd::export(&imported.project, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported = std::fs::read_to_string(&out).unwrap();
    assert_eq!(exported.matches("name=\"group-into-blocks\"").count(), 1);
    let reimported = mfd::import(&out).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(
        reimported.project.root.children[0]
            .group_into_blocks
            .is_some()
    );
    assert_eq!(engine::run(&reimported.project, &source).unwrap(), output);
}

#[test]
fn malformed_group_block_sizes_skip_the_affected_iteration() {
    let original = std::fs::read_to_string(fixture("group-blocks.mfd")).unwrap();
    let variants = [
        (
            "missing",
            original.replace("      <edge from=\"30\" to=\"32\"/>\n", ""),
        ),
        (
            "dangling",
            original.replace("from=\"30\" to=\"32\"", "from=\"999\" to=\"32\""),
        ),
    ];
    for (name, design) in variants {
        let dir = TempDir::new(&format!("group_blocks_{name}"));
        for schema in ["group-blocks-source.xsd", "group-blocks-target.xsd"] {
            std::fs::copy(fixture(schema), dir.0.join(schema)).unwrap();
        }
        let path = dir.0.join("group-blocks.mfd");
        std::fs::write(&path, design).unwrap();
        let imported = mfd::import(&path).unwrap();
        assert_eq!(
            imported.warnings.len(),
            1,
            "{name}: {:?}",
            imported.warnings
        );
        assert!(
            imported.warnings[0].contains(
                "group-into-blocks feeding `Block` has a missing or unsupported block-size; iteration skipped"
            ),
            "{name}: {:?}",
            imported.warnings
        );
        assert!(
            imported.project.root.children.is_empty(),
            "{name}: skipped iteration left a singular target scope"
        );
    }
}

#[test]
fn group_block_and_take_counts_export_parent_position_dependencies() {
    let mut project = mfd::import(&fixture("group-blocks.mfd")).unwrap().project;
    let next = project.graph.nodes.keys().next_back().copied().unwrap() + 1;
    let block_position = next;
    let block_size = next + 1;
    let take_position = next + 2;
    let take_count = next + 3;
    project.graph.nodes.extend([
        (
            block_position,
            Node::Position {
                collection: vec!["Row".into()],
            },
        ),
        (
            block_size,
            Node::Call {
                function: "add".into(),
                args: vec![block_position, 0],
            },
        ),
        (
            take_position,
            Node::Position {
                collection: vec!["Row".into()],
            },
        ),
        (
            take_count,
            Node::Call {
                function: "add".into(),
                args: vec![take_position, 0],
            },
        ),
    ]);
    let block = &mut project.root.children[0];
    block.group_into_blocks = Some(block_size);
    block.take = Some(take_count);

    let dir = TempDir::new("group_block_position_export");
    let out = dir.0.join("group-block-position.mfd");
    let warnings = mfd::export(&project, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported = std::fs::read_to_string(out).unwrap();
    let doc = roxmltree::Document::parse(&exported).unwrap();
    let position_inputs: Vec<&str> = doc
        .descendants()
        .filter(|node| node.has_tag_name("component") && node.attribute("name") == Some("position"))
        .filter_map(|component| {
            component
                .children()
                .find(|node| node.has_tag_name("sources"))
                .and_then(|sources| {
                    sources
                        .children()
                        .find(|node| node.has_tag_name("datapoint"))
                })
                .and_then(|input| input.attribute("key"))
        })
        .collect();
    assert_eq!(position_inputs.len(), 2);
    for input in position_inputs {
        assert!(
            doc.descendants().any(|node| {
                node.has_tag_name("edge") && node.attribute("vertexkey") == Some(input)
            }),
            "position input {input} has no parent-context edge\n{exported}"
        );
    }
}

#[test]
fn distinct_values_imports_as_stable_sequence_and_roundtrips() {
    let imported = mfd::import(&fixture("distinct.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;
    let rows = project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Row")
        .unwrap();
    assert_eq!(
        rows.source().map(|path| path.to_vec()),
        Some(vec!["Item".into()])
    );
    let Node::Call { function, args } = &project.graph.nodes[&rows.filter.unwrap()] else {
        panic!("distinct filter should combine the design predicate with exists");
    };
    assert_eq!(function, "and");
    assert!(args.iter().any(|node| matches!(
        &project.graph.nodes[node],
        Node::Call { function, .. } if function == "not_equal"
    )));
    assert!(args.iter().any(|node| matches!(
        &project.graph.nodes[node],
        Node::Call { function, .. } if function == "exists"
    )));
    assert!(matches!(
        &project.graph.nodes[&rows.group_by.unwrap()],
        Node::SourceField { path, .. } if path == &["Category"]
    ));
    assert!(matches!(
        &project.graph.nodes[&rows.take.unwrap()],
        Node::Const {
            value: Value::Int(2)
        }
    ));

    let source = format_xml::read(&fixture("distinct.xml"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    let output = target.field("Row").and_then(Instance::as_repeated).unwrap();
    assert_eq!(output.len(), 2);
    assert_eq!(scalar(&output[0], "Category"), Value::String("B".into()));
    assert_eq!(
        scalar(&output[0], "FirstLabel"),
        Value::String("second".into())
    );
    assert_eq!(scalar(&output[1], "Category"), Value::String("A".into()));
    assert_eq!(
        scalar(&output[1], "FirstLabel"),
        Value::String("first".into())
    );

    let dir = TempDir::new("distinct");
    let out = dir.0.join("distinct.mfd");
    let warnings = mfd::export(project, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported = std::fs::read_to_string(&out).unwrap();
    assert!(exported.contains("name=\"group-by\""));
    let reimported = mfd::import(&out).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    let rerun = engine::run(&reimported.project, &source).unwrap();
    assert_eq!(target, rerun);
}

#[test]
fn unsupported_sequence_order_imports_with_one_actionable_warning() {
    let imported = mfd::import(&fixture("distinct-order.mfd")).unwrap();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("sequence into `Row`"));
    assert!(
        imported.warnings[0].contains("applies first-items before distinct-values"),
        "{:?}",
        imported.warnings
    );
    let rows = &imported.project.root.children[0];
    assert!(rows.group_by.is_some());
    assert!(rows.take.is_some());
}

#[test]
fn tokenizers_generate_distinct_scalar_sequences_and_roundtrip() {
    let imported = mfd::import(&fixture("tokenize.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;
    let word = project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Word")
        .unwrap();
    let pair = project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Pair")
        .unwrap();
    let Some(SequenceExpr::Tokenize {
        item: word_item, ..
    }) = word.sequence()
    else {
        panic!("Word should iterate tokenize output");
    };
    let Some(SequenceExpr::TokenizeByLength {
        item: pair_item, ..
    }) = pair.sequence()
    else {
        panic!("Pair should iterate tokenize-by-length output");
    };
    assert_ne!(word_item, pair_item);
    assert!(matches!(
        &project.graph.nodes[word_item],
        Node::SourceField { path, frame: None } if path.is_empty()
    ));
    assert!(matches!(
        &project.graph.nodes[pair_item],
        Node::SourceField { path, frame: None } if path.is_empty()
    ));

    let source = format_xml::read(&fixture("tokenize.xml"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    let words = target
        .field("Word")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(
        words
            .iter()
            .map(|row| scalar(row, "Value"))
            .collect::<Vec<_>>(),
        vec![
            Value::String("alpha".into()),
            Value::String("beta".into()),
            Value::String("gamma".into()),
        ]
    );
    let pairs = target
        .field("Pair")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(
        pairs
            .iter()
            .map(|row| scalar(row, "Value"))
            .collect::<Vec<_>>(),
        vec![Value::String("Aé".into()), Value::String("🙂Z".into())]
    );

    let dir = TempDir::new("tokenize");
    let out = dir.0.join("tokenize.mfd");
    let warnings = mfd::export(project, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported = std::fs::read_to_string(&out).unwrap();
    assert_eq!(exported.matches("name=\"tokenize\"").count(), 1);
    assert_eq!(exported.matches("name=\"tokenize-by-length\"").count(), 1);
    let reimported = mfd::import(&out).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    let rerun = engine::run(&reimported.project, &source).unwrap();
    assert_eq!(target, rerun);
}

#[test]
fn generated_integer_ranges_import_controls_execute_and_roundtrip() {
    let imported = mfd::import(&fixture("generate.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let project = &imported.project;
    let item = project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Item")
        .unwrap();
    let default = project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Default")
        .unwrap();
    assert!(matches!(
        item.sequence(),
        Some(SequenceExpr::Generate { from: Some(_), .. })
    ));
    assert!(matches!(
        default.sequence(),
        Some(SequenceExpr::Generate { from: None, .. })
    ));
    assert!(item.filter.is_some());
    assert!(item.group_by.is_some());
    assert!(item.sort_by.is_some());
    assert!(item.sort_descending);
    assert!(item.take.is_some());

    let source = format_xml::read(&fixture("generate.xml"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    let items = target
        .field("Item")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(
        items
            .iter()
            .map(|row| (scalar(row, "Value"), scalar(row, "Position")))
            .collect::<Vec<_>>(),
        vec![
            (Value::Int(6), Value::Int(1)),
            (Value::Int(5), Value::Int(2))
        ]
    );
    let defaults = target
        .field("Default")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(
        defaults
            .iter()
            .map(|row| scalar(row, "Value"))
            .collect::<Vec<_>>(),
        vec![Value::Int(1), Value::Int(2), Value::Int(3)]
    );

    let dir = TempDir::new("generate");
    let out = dir.0.join("generate.mfd");
    let warnings = mfd::export(project, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported = std::fs::read_to_string(&out).unwrap();
    assert_eq!(exported.matches("name=\"generate-sequence\"").count(), 2);
    for component in ["filter", "group-by", "sort", "first-items"] {
        assert_eq!(
            exported.matches(&format!("name=\"{component}\"")).count(),
            1,
            "{component} was not exported exactly once"
        );
    }
    let doc = roxmltree::Document::parse(&exported).unwrap();
    let component_pin = |name: &str, pins: &str| {
        doc.descendants()
            .find(|node| {
                node.is_element()
                    && node.tag_name().name() == "component"
                    && node.attribute("name") == Some(name)
            })
            .and_then(|component| {
                component
                    .children()
                    .find(|node| node.is_element() && node.tag_name().name() == pins)
            })
            .and_then(|pins| {
                pins.children()
                    .find(|node| node.is_element() && node.tag_name().name() == "datapoint")
            })
            .and_then(|pin| pin.attribute("key"))
            .unwrap()
            .to_string()
    };
    let controlled_output = component_pin("first-items", "targets");
    let position_input = component_pin("position", "sources");
    assert!(
        doc.descendants()
            .filter(|node| {
                node.is_element()
                    && node.tag_name().name() == "vertex"
                    && node.attribute("vertexkey") == Some(controlled_output.as_str())
            })
            .flat_map(|vertex| vertex.descendants())
            .any(|node| {
                node.is_element()
                    && node.tag_name().name() == "edge"
                    && node.attribute("vertexkey") == Some(position_input.as_str())
            }),
        "missing edge {controlled_output} -> {position_input}\n{exported}"
    );
    let reimported = mfd::import(&out).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    let reimported_item = reimported
        .project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Item")
        .unwrap();
    assert!(reimported_item.filter.is_some());
    assert!(reimported_item.group_by.is_some());
    assert!(reimported_item.sort_by.is_some());
    assert!(reimported_item.sort_descending);
    assert!(reimported_item.take.is_some());
    let rerun = engine::run(&reimported.project, &source).unwrap();
    assert_eq!(target, rerun);
}

#[test]
fn exporting_one_position_at_multiple_iteration_stages_warns_once() {
    let mut project = mfd::import(&fixture("generate.mfd")).unwrap().project;
    let item = project
        .root
        .children
        .iter_mut()
        .find(|scope| scope.target_field == "Item")
        .unwrap();
    let position = item
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Position")
        .unwrap()
        .node;
    item.sort_by = Some(position);

    let dir = TempDir::new("position_context_conflict");
    let warnings = mfd::export(&project, &dir.0.join("conflict.mfd")).unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].contains("used in multiple iteration stages or scopes"),
        "{warnings:?}"
    );
}

#[test]
fn noncanonical_ordinary_control_order_warns_once() {
    let imported = mfd::import(&fixture("control-order.mfd")).unwrap();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(
        imported.warnings[0]
            .contains("applies sort after filter, which cannot be represented exactly"),
        "{:?}",
        imported.warnings
    );
    let item = &imported.project.root.children[0];
    assert!(item.sequence().is_some());
    assert!(item.filter.is_some());
    assert!(item.sort_by.is_some());
}

#[test]
fn tokenizer_scalar_use_emits_an_actionable_warning() {
    let imported = mfd::import(&fixture("tokenize-scalar.mfd")).unwrap();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(
        imported.warnings[0]
            .contains("sequence function `tokenize` is not connected to a repeating target"),
        "{:?}",
        imported.warnings
    );
}
