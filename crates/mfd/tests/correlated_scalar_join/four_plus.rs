use super::*;

fn four_plus_plan(fifth: bool) -> Result<JoinPlan, mapping::JoinPlanError> {
    let plan = three_plan()?.then(
        JoinSource::new(vec!["Flags".into(), "Flag".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["Offers".into(), "Offer".into()],
            vec!["Promo".into()],
            vec!["Promo".into()],
        ))
        .and(JoinKey::new(
            vec!["Customers".into(), "Customer".into()],
            vec!["Region".into()],
            vec!["Region".into()],
        ))
        .and(JoinKey::new(
            vec!["CustomerNumber".into()],
            vec![],
            vec!["Number".into()],
        )),
    )?;
    if fifth {
        plan.then(
            JoinSource::new(vec!["Badges".into(), "Badge".into()]),
            JoinConditions::new(JoinKey::new(
                vec!["Flags".into(), "Flag".into()],
                vec!["Tag".into()],
                vec!["Tag".into()],
            ))
            .and(JoinKey::new(
                vec!["Offers".into(), "Offer".into()],
                vec!["Promo".into()],
                vec!["Promo".into()],
            )),
        )
    } else {
        Ok(plan)
    }
}

fn match_schema(project: &mut Project) -> &mut SchemaNode {
    let SchemaKind::Group { children, .. } = &mut project.target.kind else {
        panic!("fixture target required")
    };
    let SchemaKind::Group { children, .. } = &mut children[0].kind else {
        panic!("fixture Order required")
    };
    &mut children[1]
}

fn four_plus_project(fifth: bool) -> Result<Project, mapping::JoinPlanError> {
    let mut project = three_project()?;
    let mut number = SchemaNode::scalar("Number", ScalarType::String).nillable();
    assert!(number.set_xml_optional(true));
    project.extra_sources.push(NamedSource {
        name: "Flags".into(),
        path: "flags.xml".into(),
        schema: SchemaNode::group(
            "Flags",
            vec![
                SchemaNode::group(
                    "Flag",
                    vec![
                        SchemaNode::scalar("Promo", ScalarType::String),
                        number,
                        SchemaNode::scalar("Region", ScalarType::String),
                        SchemaNode::scalar("Tag", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        ),
        options: Default::default(),
        dynamic_path: None,
    });
    append_schema_field(
        match_schema(&mut project),
        SchemaNode::scalar("Flag", ScalarType::String),
    );
    project.graph.nodes.insert(
        4,
        Node::JoinField {
            join: JoinId::new(777),
            collection: vec!["Flags".into(), "Flag".into()],
            path: vec!["Tag".into()],
        },
    );
    nested_scope(&mut project).bindings.push(Binding {
        target_field: "Flag".into(),
        node: 4,
    });
    if fifth {
        project.extra_sources.push(NamedSource {
            name: "Badges".into(),
            path: "badges.xml".into(),
            schema: SchemaNode::group(
                "Badges",
                vec![string_fields("Badge", &["Tag", "Promo", "Label"]).repeating()],
            ),
            options: Default::default(),
            dynamic_path: None,
        });
        append_schema_field(
            match_schema(&mut project),
            SchemaNode::scalar("Badge", ScalarType::String),
        );
        project.graph.nodes.insert(
            5,
            Node::JoinField {
                join: JoinId::new(777),
                collection: vec!["Badges".into(), "Badge".into()],
                path: vec!["Label".into()],
            },
        );
        nested_scope(&mut project).bindings.push(Binding {
            target_field: "Badge".into(),
            node: 5,
        });
    }
    replace_plan(&mut project, four_plus_plan(fifth)?);
    Ok(project)
}

fn four_plus_inputs(fifth: bool) -> (Instance, Vec<(String, Instance)>) {
    let (primary, mut extras) = three_inputs();
    let flags = [
        ("P2", Value::String("B".into()), "R1", "Two1"),
        ("P1", Value::String("B".into()), "R1", "One1"),
        ("P1", Value::String("B".into()), "R1", "One2"),
        ("P3", Value::String("B".into()), "R2", "Three"),
        ("PA", Value::String("A".into()), "R1", "AyFlag"),
        ("P1", Value::String("B".into()), "Wrong", "WrongRegion"),
        ("P1", Value::String("X".into()), "R1", "WrongKey"),
        ("P1", Value::Null, "R1", "NullKey"),
        ("P1", Value::XmlNil(XmlNil), "R1", "NilKey"),
    ]
    .into_iter()
    .map(|(promo, number, region, tag)| {
        Instance::Group(
            vec![
                (
                    "Promo".into(),
                    Instance::Scalar(Value::String(promo.into())),
                ),
                ("Number".into(), Instance::Scalar(number)),
                (
                    "Region".into(),
                    Instance::Scalar(Value::String(region.into())),
                ),
                ("Tag".into(), Instance::Scalar(Value::String(tag.into()))),
            ]
            .into(),
        )
    })
    .collect();
    extras.push((
        "Flags".into(),
        Instance::Group(vec![("Flag".into(), Instance::Repeated(flags))].into()),
    ));
    if fifth {
        extras.push((
            "Badges".into(),
            Instance::Group(
                vec![(
                    "Badge".into(),
                    Instance::Repeated(vec![
                        strings(&[("Tag", "One1"), ("Promo", "P1"), ("Label", "X")]),
                        strings(&[("Tag", "One1"), ("Promo", "P1"), ("Label", "Y")]),
                        strings(&[("Tag", "One2"), ("Promo", "P1"), ("Label", "Z")]),
                        strings(&[("Tag", "Two1"), ("Promo", "P2"), ("Label", "W")]),
                        strings(&[("Tag", "Three"), ("Promo", "P3"), ("Label", "Q")]),
                        strings(&[("Tag", "AyFlag"), ("Promo", "PA"), ("Label", "A")]),
                        strings(&[("Tag", "One1"), ("Promo", "wrong"), ("Label", "WrongPromo")]),
                    ]),
                )]
                .into(),
            ),
        ));
    }
    (primary, extras)
}

fn literal_four_plus_output(fifth: bool) -> Instance {
    let (first, second) = if fifth {
        (
            vec![
                strings(&[
                    ("CustomerName", "Bee1"),
                    ("Promo", "P1"),
                    ("Flag", "One1"),
                    ("Badge", "X"),
                ]),
                strings(&[
                    ("CustomerName", "Bee1"),
                    ("Promo", "P1"),
                    ("Flag", "One1"),
                    ("Badge", "Y"),
                ]),
                strings(&[
                    ("CustomerName", "Bee1"),
                    ("Promo", "P1"),
                    ("Flag", "One2"),
                    ("Badge", "Z"),
                ]),
                strings(&[
                    ("CustomerName", "Bee1"),
                    ("Promo", "P2"),
                    ("Flag", "Two1"),
                    ("Badge", "W"),
                ]),
                strings(&[
                    ("CustomerName", "Bee2"),
                    ("Promo", "P3"),
                    ("Flag", "Three"),
                    ("Badge", "Q"),
                ]),
            ],
            vec![strings(&[
                ("CustomerName", "Ay"),
                ("Promo", "PA"),
                ("Flag", "AyFlag"),
                ("Badge", "A"),
            ])],
        )
    } else {
        (
            vec![
                strings(&[("CustomerName", "Bee1"), ("Promo", "P1"), ("Flag", "One1")]),
                strings(&[("CustomerName", "Bee1"), ("Promo", "P1"), ("Flag", "One2")]),
                strings(&[("CustomerName", "Bee1"), ("Promo", "P2"), ("Flag", "Two1")]),
                strings(&[("CustomerName", "Bee2"), ("Promo", "P3"), ("Flag", "Three")]),
            ],
            vec![strings(&[
                ("CustomerName", "Ay"),
                ("Promo", "PA"),
                ("Flag", "AyFlag"),
            ])],
        )
    };
    let orders = [
        ("O1", first),
        ("O2", second),
        ("O3", vec![]),
        ("O4", vec![]),
        ("O5", vec![]),
    ]
    .into_iter()
    .map(|(id, matches)| {
        Instance::Group(
            vec![
                ("Id".into(), Instance::Scalar(Value::String(id.into()))),
                ("Match".into(), Instance::Repeated(matches)),
            ]
            .into(),
        )
    })
    .collect();
    Instance::Group(vec![("Order".into(), Instance::Repeated(orders))].into())
}

fn validate_four_plus_owner(project: &Project, fifth: bool) -> Result<(), Box<dyn Error>> {
    let order = child(&project.root, "Order").ok_or("missing Order scope")?;
    assert_eq!(order.source(), Some(["Order".to_string()].as_slice()));
    let matches = child(order, "Match").ok_or("missing Match scope")?;
    let (owner, plan) = matches.join().ok_or("missing nested join owner")?;
    assert_eq!(plan, &four_plus_plan(fifth)?);
    let mut bindings = vec![
        (
            "CustomerName",
            vec!["Customers".to_string(), "Customer".to_string()],
            vec!["Name".to_string()],
        ),
        (
            "Promo",
            vec!["Offers".to_string(), "Offer".to_string()],
            vec!["Promo".to_string()],
        ),
        (
            "Flag",
            vec!["Flags".to_string(), "Flag".to_string()],
            vec!["Tag".to_string()],
        ),
    ];
    if fifth {
        bindings.push((
            "Badge",
            vec!["Badges".to_string(), "Badge".to_string()],
            vec!["Label".to_string()],
        ));
    }
    assert_eq!(matches.bindings.len(), bindings.len());
    for (field, collection, path) in &bindings {
        let binding = matches
            .bindings
            .iter()
            .find(|binding| binding.target_field == *field)
            .ok_or("missing joined binding")?;
        let Some(Node::JoinField {
            join,
            collection: actual_collection,
            path: actual_path,
        }) = project.graph.nodes.get(&binding.node)
        else {
            return Err("joined binding lost its exact owner".into());
        };
        assert_eq!(*join, owner);
        assert_eq!(actual_collection, collection);
        assert_eq!(actual_path, path);
    }
    assert_eq!(
        project
            .graph
            .nodes
            .values()
            .filter(|node| matches!(node, Node::JoinField { join, .. } if *join == owner))
            .count(),
        bindings.len()
    );
    let names = if fifth {
        vec!["Customers", "Offers", "Flags", "Badges"]
    } else {
        vec!["Customers", "Offers", "Flags"]
    };
    assert_eq!(project.extra_sources.len(), names.len());
    for (source, name) in project.extra_sources.iter().zip(names) {
        assert_eq!(source.name, name);
        assert_eq!(source.schema.name, name);
        assert!(source.dynamic_path.is_none());
    }
    Ok(())
}

fn validate_four_plus_xml(xml: &str, fifth: bool) -> Result<(), Box<dyn Error>> {
    let document = roxmltree::Document::parse(xml)?;
    let components = document
        .descendants()
        .filter(|node| node.has_tag_name("component") && node.attribute("kind") == Some("32"))
        .collect::<Vec<_>>();
    assert_eq!(components.len(), 1);
    let join = components[0];
    let mut names = vec![
        "dynamic_tree_node0",
        "dynamic_tree_node1",
        "dynamic_tree_node2",
        "dynamic_tree_node3",
    ];
    let mut pairs = vec![
        ("0", "1"),
        ("1", "2"),
        ("0", "2"),
        ("2", "3"),
        ("1", "3"),
        ("0", "3"),
    ];
    if fifth {
        names.push("dynamic_tree_node4");
        pairs.extend([("3", "4"), ("2", "4")]);
    }
    assert_eq!(
        join.descendants()
            .filter_map(|node| node.attribute("name"))
            .filter(|name| name.starts_with("dynamic_tree_node"))
            .collect::<Vec<_>>(),
        names
    );
    let actual_pairs = join
        .descendants()
        .filter(|node| node.has_tag_name("keypair"))
        .map(|pair| {
            let first = pair
                .children()
                .find(|node| node.has_tag_name("first-key"))
                .and_then(|node| node.attribute("input-index"));
            let second = pair
                .children()
                .find(|node| node.has_tag_name("second-key"))
                .and_then(|node| node.attribute("input-index"));
            (first, second)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        actual_pairs,
        pairs
            .iter()
            .map(|(left, right)| (Some(*left), Some(*right)))
            .collect::<Vec<_>>()
    );
    let graph = document
        .descendants()
        .find(|node| node.has_tag_name("graph"))
        .ok_or("missing graph")?;
    let inputs = join
        .descendants()
        .filter_map(|node| node.attribute("inpkey"))
        .collect::<Vec<_>>();
    assert_eq!(inputs.len(), names.len());
    let uids = if fifth {
        vec!["2", "3", "4", "5", "6"]
    } else {
        vec!["2", "3", "4", "5"]
    };
    for (input, uid) in inputs.into_iter().zip(uids) {
        let feeds = graph
            .descendants()
            .filter(|node| node.has_tag_name("vertex"))
            .filter_map(|vertex| {
                vertex
                    .descendants()
                    .find(|node| {
                        node.has_tag_name("edge") && node.attribute("vertexkey") == Some(input)
                    })
                    .map(|edge| (vertex, edge))
            })
            .collect::<Vec<_>>();
        assert_eq!(feeds.len(), 1);
        let (vertex, edge) = feeds[0];
        let output = vertex
            .attribute("vertexkey")
            .ok_or("missing source output")?;
        let source = document
            .descendants()
            .find(|node| node.has_tag_name("component") && node.attribute("uid") == Some(uid))
            .ok_or("missing exact source component")?;
        assert!(
            source
                .descendants()
                .any(|node| node.attribute("outkey") == Some(output))
        );
        let key = edge
            .attribute("edgekey")
            .ok_or("missing structural feed edge")?;
        let connection = graph
            .children()
            .filter(|node| node.has_tag_name("edges"))
            .flat_map(|node| node.children())
            .find(|node| node.has_tag_name("edge") && node.attribute("edgekey") == Some(key))
            .ok_or("missing feed metadata")?;
        assert!(
            connection
                .descendants()
                .any(|node| node.has_tag_name("dataconnection")
                    && node.attribute("type") == Some("2"))
        );
    }
    Ok(())
}

fn roundtrip_four_plus(fifth: bool, cycles: usize) -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let original = four_plus_project(fifth)?;
    let (primary, extras) = four_plus_inputs(fifth);
    let expected = literal_four_plus_output(fifth);
    write(
        &directory.0.join("project-original.json"),
        &serde_json::to_string_pretty(&original)?,
    )?;
    retain(
        &directory.0,
        "typed-inputs-original.debug",
        &(&primary, &extras),
    )?;
    retain(&directory.0, "literal-output.debug", &expected)?;
    let validation = engine::validate(&original);
    let lowering = codegen::lower(&original);
    let outcome = engine::run_with_sources(&original, &primary, extras.clone());
    retain(&directory.0, "original-validation.debug", &validation)?;
    retain(&directory.0, "original-lowering.debug", &lowering)?;
    retain(&directory.0, "original-outcome.debug", &outcome)?;
    assert!(validation.is_empty(), "{validation:?}");
    assert!(lowering.is_ok(), "{lowering:?}");
    assert_eq!(outcome?, expected);
    validate_four_plus_owner(&original, fifth)?;
    write(
        &directory.0.join("orders.xml"),
        &format_xml::to_string(&original.source, &primary)?,
    )?;
    for source in &original.extra_sources {
        let input = extras
            .iter()
            .find(|(name, _)| name == &source.name)
            .ok_or("missing named input")?;
        write(
            &directory.0.join(&source.path),
            &format_xml::to_string(&source.schema, &input.1)?,
        )?;
    }
    for (label, profile) in [
        ("default", mfd::ExportProfile::FerruleExtensions),
        ("native", mfd::ExportProfile::NativeMfd),
    ] {
        let mut current = original.clone();
        for cycle in 0..cycles {
            let name = format!("{label}-{cycle}");
            write(
                &directory.0.join(format!("{name}-before.json")),
                &serde_json::to_string_pretty(&current)?,
            )?;
            let design = directory.0.join(format!("{name}.mfd"));
            let exported = mfd::export_with_profile(&current, &design, profile);
            retain(&directory.0, &format!("{name}-export.debug"), &exported)?;
            let report = exported?;
            assert!(report.warnings.is_empty(), "{:?}", report.warnings);
            assert!(report.is_native_compatible());
            validate_four_plus_xml(&std::fs::read_to_string(&design)?, fifth)?;
            let imported = mfd::import_with_profile(
                &design,
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable,
            );
            match &imported {
                Ok(outcome) => retain(
                    &directory.0,
                    &format!("{name}-import.debug"),
                    &(
                        &outcome.report,
                        &outcome.imported.warnings,
                        &outcome.imported.mapping_path,
                        &outcome.imported.project,
                    ),
                )?,
                Err(error) => retain(&directory.0, &format!("{name}-import.debug"), error)?,
            }
            let imported = imported?;
            assert!(imported.report.executable);
            assert!(
                imported.imported.warnings.is_empty(),
                "{:?}",
                imported.imported.warnings
            );
            current = imported.imported.project;
            write(
                &directory.0.join(format!("{name}-after.json")),
                &serde_json::to_string_pretty(&current)?,
            )?;
            let validation = engine::validate(&current);
            retain(
                &directory.0,
                &format!("{name}-validation.debug"),
                &validation,
            )?;
            assert!(validation.is_empty(), "{validation:?}");
            validate_four_plus_owner(&current, fifth)?;
            let path = directory.0.join(
                current
                    .source_path
                    .as_deref()
                    .ok_or("missing primary identity")?,
            );
            let identity = (
                path.canonicalize()?,
                directory.0.join("orders.xml").canonicalize()?,
            );
            retain(
                &directory.0,
                &format!("{name}-primary-identity.debug"),
                &identity,
            )?;
            assert_eq!(identity.0, identity.1);
            let decoded_primary =
                format_xml::from_str(&std::fs::read_to_string(&path)?, &current.source);
            retain(
                &directory.0,
                &format!("{name}-decoded-primary.debug"),
                &decoded_primary,
            )?;
            let decoded_primary = decoded_primary?;
            assert_eq!(decoded_primary, primary);
            let mut decoded_extras = Vec::new();
            for source in &current.extra_sources {
                let original_source = original
                    .extra_sources
                    .iter()
                    .find(|original| original.name == source.name)
                    .ok_or("missing source identity")?;
                let path = directory.0.join(&source.path);
                let identity = (
                    path.canonicalize()?,
                    directory.0.join(&original_source.path).canonicalize()?,
                );
                retain(
                    &directory.0,
                    &format!("{name}-{}-identity.debug", source.name),
                    &identity,
                )?;
                assert_eq!(identity.0, identity.1);
                let decoded =
                    format_xml::from_str(&std::fs::read_to_string(&path)?, &source.schema);
                retain(
                    &directory.0,
                    &format!("{name}-decoded-{}.debug", source.name),
                    &decoded,
                )?;
                let decoded = decoded?;
                let original_input = extras
                    .iter()
                    .find(|(input_name, _)| input_name == &source.name)
                    .ok_or("missing original named input")?;
                assert_eq!(decoded, original_input.1);
                decoded_extras.push((source.name.clone(), decoded));
            }
            let outcome = engine::run_with_sources(&current, &decoded_primary, decoded_extras);
            retain(&directory.0, &format!("{name}-outcome.debug"), &outcome)?;
            assert_eq!(outcome?, expected);
        }
    }
    Ok(())
}

#[test]
fn nested_four_input_join_keeps_literal_duplicates_current_item_keys_and_two_profile_cycles()
-> Result<(), Box<dyn Error>> {
    roundtrip_four_plus(false, 2)
}

#[test]
fn nested_fifth_input_join_keeps_ordered_earlier_owners_and_reimport_shapes()
-> Result<(), Box<dyn Error>> {
    roundtrip_four_plus(true, 1)
}

#[test]
fn nested_four_input_export_retains_primary_last_source_shadow_anchor_and_alias_guards()
-> Result<(), Box<dyn Error>> {
    let mut wrong_primary = four_plus_project(false)?;
    wrong_primary.extra_sources.push(NamedSource {
        name: "Order".into(),
        path: "shadow.xml".into(),
        schema: string_fields("Shadow", &["Id", "CustomerNumber"]),
        options: Default::default(),
        dynamic_path: None,
    });
    assert_refused(
        &wrong_primary,
        "singleton and anchor must belong to the primary source component",
        0,
    )?;
    for active in [false, true] {
        let mut shadowed = four_plus_project(false)?;
        let collision = shadowed.extra_sources[2].schema.clone();
        if active {
            let SchemaKind::Group { children, .. } = &mut shadowed.source.kind else {
                panic!("fixture group required")
            };
            append_schema_field(&mut children[1], collision);
        } else {
            append_schema_field(&mut shadowed.source, collision);
        }
        assert_refused(
            &shadowed,
            "source `Flags` is shadowed by a primary source frame",
            0,
        )?;
    }
    let mut stripped = four_plus_project(false)?;
    let SchemaKind::Group { children, .. } = &mut stripped.source.kind else {
        panic!("fixture group required")
    };
    children[1].name = "Flags".into();
    stripped.root.children[0].iteration = ScopeIteration::Source(vec!["Flags".into()]);
    let Some(Node::SourceField { frame, .. }) = stripped.graph.nodes.get_mut(&1) else {
        panic!("fixture field required")
    };
    *frame = Some(vec!["Flags".into()]);
    assert_refused(
        &stripped,
        "collection would change when its enclosing anchor is removed",
        0,
    )?;
    let mut alias = four_plus_project(false)?;
    let flags = alias.extra_sources.pop().ok_or("missing Flags fixture")?;
    let SchemaKind::Group { mut children, .. } = flags.schema.kind else {
        panic!("fixture group required")
    };
    append_schema_field(&mut alias.extra_sources[1].schema, children.remove(0));
    let plan = three_plan()?.then(
        JoinSource::new(vec!["Offers".into(), "Flag".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["Offers".into(), "Offer".into()],
            vec!["Promo".into()],
            vec!["Promo".into()],
        )),
    )?;
    replace_plan(&mut alias, plan);
    let Some(Node::JoinField { collection, .. }) = alias.graph.nodes.get_mut(&4) else {
        panic!("fixture projection required")
    };
    *collection = vec!["Offers".into(), "Flag".into()];
    assert_refused(&alias, "distinct named source components", 0)
}

#[test]
fn nested_four_input_dynamic_last_source_is_a_hard_prepublication_refusal()
-> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let mut project = four_plus_project(false)?;
    project.graph.nodes.insert(
        99,
        Node::Const {
            value: Value::String("flags.xml".into()),
        },
    );
    project.extra_sources[2].dynamic_path = Some(mapping::DynamicSourcePath {
        node: 99,
        iteration: vec!["Order".into()],
    });
    write(
        &directory.0.join("dynamic-project.json"),
        &serde_json::to_string_pretty(&project)?,
    )?;
    let destination = directory.0.join("mapping.mfd");
    write(&destination, "sentinel")?;
    for profile in [
        mfd::ExportProfile::FerruleExtensions,
        mfd::ExportProfile::NativeMfd,
    ] {
        let before = files(&directory.0)?;
        let result = mfd::export_with_profile(&project, &destination, profile);
        let after = files(&directory.0)?;
        retain(
            &directory.0,
            &format!("dynamic-{profile:?}-outcome.debug"),
            &result,
        )?;
        retain(
            &directory.0,
            &format!("dynamic-{profile:?}-inventory.debug"),
            &(&before, &after),
        )?;
        assert!(
            matches!(&result, Err(mfd::MfdError::Unsupported(message)) if message == "inner join 777 references dynamic additional source `Flags`; kind-32 export requires static source components"),
            "{result:?}"
        );
        assert_eq!(before, after);
    }
    Ok(())
}

#[test]
fn nested_four_input_malformed_owner_metadata_is_skipped_and_strictly_refused()
-> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let project = four_plus_project(false)?;
    let design = directory.0.join("good.mfd");
    let exported = mfd::export_with_profile(&project, &design, mfd::ExportProfile::NativeMfd);
    retain(&directory.0, "good-export.debug", &exported)?;
    assert!(exported?.warnings.is_empty());
    let xml = std::fs::read_to_string(&design)?;
    let document = roxmltree::Document::parse(&xml)?;
    let first = document
        .descendants()
        .find(|node| node.has_tag_name("first-key"))
        .ok_or("missing first key")?;
    let range = first.range();
    let original = &xml[range.clone()];
    assert_eq!(original.matches("input-index=\"0\"").count(), 1);
    for (label, index, reason) in [
        ("unknown", "4", "input index 4 is out of range for 4 inputs"),
        (
            "future",
            "2",
            "join input 1 must have an equality with an earlier input",
        ),
    ] {
        let mut malformed = xml.clone();
        malformed.replace_range(
            range.clone(),
            &original.replace("input-index=\"0\"", &format!("input-index=\"{index}\"")),
        );
        let path = directory.0.join(format!("{label}.mfd"));
        write(&path, &malformed)?;
        let before = files(&directory.0)?;
        let best = mfd::import_with_profile(
            &path,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::BestEffort,
        );
        let strict = mfd::import_with_profile(
            &path,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        );
        let after = files(&directory.0)?;
        match &best {
            Ok(outcome) => retain(
                &directory.0,
                &format!("{label}-best.debug"),
                &(
                    &outcome.report,
                    &outcome.imported.warnings,
                    &outcome.imported.project,
                ),
            )?,
            Err(error) => retain(&directory.0, &format!("{label}-best.debug"), error)?,
        }
        match &strict {
            Ok(outcome) => retain(
                &directory.0,
                &format!("{label}-strict.debug"),
                &(
                    &outcome.report,
                    &outcome.imported.warnings,
                    &outcome.imported.project,
                ),
            )?,
            Err(error) => retain(&directory.0, &format!("{label}-strict.debug"), error)?,
        }
        retain(
            &directory.0,
            &format!("{label}-inventory.debug"),
            &(&before, &after),
        )?;
        assert_eq!(before, after, "import refusal published physical artifacts");
        let best = best?;
        assert!(!best.report.executable);
        assert!(
            best.imported
                .warnings
                .iter()
                .any(|warning| warning.contains(reason)),
            "{:?}",
            best.imported.warnings
        );
        assert!(
            !best
                .imported
                .project
                .root
                .children
                .iter()
                .flat_map(|scope| &scope.children)
                .any(|scope| scope.join().is_some())
        );
        assert!(
            !best
                .imported
                .project
                .graph
                .nodes
                .values()
                .any(|node| matches!(node, Node::JoinField { .. } | Node::JoinPosition { .. }))
        );
        assert!(
            matches!(strict, Err(mfd::MfdError::IncompatibleImport(report)) if !report.executable && report.issues.iter().any(|issue| issue.message.contains(reason))),
            "strict import did not retain the malformed join diagnosis"
        );
    }
    Ok(())
}
