//! Nested joins whose singletons come from a static named source, sit at any
//! input position, or number more than one.

use super::*;

fn p(segments: &[&str]) -> Vec<String> {
    segments.iter().map(|segment| (*segment).into()).collect()
}

fn key(left: &[&str], left_path: &[&str], right_path: &[&str]) -> JoinConditions {
    JoinConditions::new(JoinKey::new(p(left), p(left_path), p(right_path)))
}

fn customer_number() -> JoinSource {
    JoinSource::singleton(p(&["CustomerNumber"]))
}

fn customers() -> JoinSource {
    JoinSource::new(p(&["Customers", "Customer"]))
}

fn region() -> JoinSource {
    JoinSource::singleton(p(&["Settings", "Region"]))
}

fn settings_source() -> NamedSource {
    NamedSource {
        name: "Settings".into(),
        path: "settings.xml".into(),
        schema: string_fields("Settings", &["Region"]),
        options: Default::default(),
        dynamic_path: None,
    }
}

/// Removes the `Offers` source and its `Promo` projection.
fn without_offers(project: &mut Project) {
    project
        .extra_sources
        .retain(|source| source.name != "Offers");
    project.graph.nodes.remove(&3);
    nested_scope(project)
        .bindings
        .retain(|binding| binding.target_field != "Promo");
}

/// Joins `plan` inside every order, with or without `Offers`, and adds the
/// static `Settings` source when the plan names it.
fn shape_project(plan: JoinPlan, offers: bool) -> Result<Project, Box<dyn Error>> {
    let mut project = three_project()?;
    if plan
        .sources()
        .any(|source| source.collection().first().map(String::as_str) == Some("Settings"))
    {
        project.extra_sources.push(settings_source());
    }
    if !offers {
        without_offers(&mut project);
    }
    replace_plan(&mut project, plan);
    Ok(project)
}

fn shape_inputs(project: &Project) -> (Instance, Vec<(String, Instance)>) {
    let (primary, mut extras) = three_inputs();
    extras.push(("Settings".into(), strings(&[("Region", "R1")])));
    extras.retain(|(name, _)| {
        project
            .extra_sources
            .iter()
            .any(|source| &source.name == name)
    });
    (primary, extras)
}

fn names(names: &[&str]) -> Vec<Instance> {
    names
        .iter()
        .map(|name| strings(&[("CustomerName", name)]))
        .collect()
}

fn offers(pairs: &[(&str, &str)]) -> Vec<Instance> {
    pairs
        .iter()
        .map(|(name, promo)| strings(&[("CustomerName", name), ("Promo", promo)]))
        .collect()
}

/// The five fixture orders with the given matches, in order.
fn output(matches: [Vec<Instance>; 5]) -> Instance {
    let orders = ["O1", "O2", "O3", "O4", "O5"]
        .into_iter()
        .zip(matches)
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

type JoinShape = (
    Option<Vec<String>>,
    JoinPlan,
    Vec<(String, Vec<String>, Vec<String>)>,
    Vec<String>,
);

/// The order driver, nested plan, joined bindings and static source names.
fn join_shape(project: &Project) -> Result<JoinShape, Box<dyn Error>> {
    let order = child(&project.root, "Order").ok_or("missing Order scope")?;
    let matches = child(order, "Match").ok_or("missing Match scope")?;
    let (owner, plan) = matches.join().ok_or("missing nested join owner")?;
    let mut bindings = matches
        .bindings
        .iter()
        .map(|binding| match project.graph.nodes.get(&binding.node) {
            Some(Node::JoinField {
                join,
                collection,
                path,
            }) if *join == owner => Ok((
                binding.target_field.clone(),
                collection.clone(),
                path.clone(),
            )),
            _ => Err("joined binding lost its exact owner"),
        })
        .collect::<Result<Vec<_>, _>>()?;
    bindings.sort();
    let joined = project
        .graph
        .nodes
        .values()
        .filter(|node| matches!(node, Node::JoinField { join, .. } if *join == owner))
        .count();
    assert_eq!(joined, bindings.len());
    let mut sources = Vec::new();
    for source in &project.extra_sources {
        assert!(source.dynamic_path.is_none());
        sources.push(source.name.clone());
    }
    Ok((
        order
            .source()
            .expect("legacy fixture selector")
            .map(<[String]>::to_vec),
        plan.clone(),
        bindings,
        sources,
    ))
}

/// Every join input is fed by one copy-all edge from the component with the
/// matching uid, in input order.
fn validate_inputs_xml(xml: &str, uids: &[&str]) -> Result<(), Box<dyn Error>> {
    let document = roxmltree::Document::parse(xml)?;
    let joins = document
        .descendants()
        .filter(|node| node.has_tag_name("component") && node.attribute("kind") == Some("32"))
        .collect::<Vec<_>>();
    assert_eq!(joins.len(), 1);
    let graph = document
        .descendants()
        .find(|node| node.has_tag_name("graph"))
        .ok_or("missing graph")?;
    let inputs = joins[0]
        .descendants()
        .filter_map(|node| node.attribute("inpkey"))
        .collect::<Vec<_>>();
    assert_eq!(inputs.len(), uids.len());
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
                .any(|node| node.attribute("outkey") == Some(output)),
            "input {input} is not fed by component {uid}"
        );
        let edge_key = edge
            .attribute("edgekey")
            .ok_or("missing structural feed edge")?;
        let connection = graph
            .children()
            .filter(|node| node.has_tag_name("edges"))
            .flat_map(|node| node.children())
            .find(|node| node.has_tag_name("edge") && node.attribute("edgekey") == Some(edge_key))
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

/// Runs `project`, then exports and reimports it twice under each profile,
/// checking the plan, the wiring and the output after every cycle.
fn roundtrip(project: &Project, expected: &Instance, uids: &[&str]) -> Result<(), Box<dyn Error>> {
    roundtrip_inputs(project, shape_inputs(project), expected, uids)
}

fn roundtrip_inputs(
    project: &Project,
    (primary, extras): (Instance, Vec<(String, Instance)>),
    expected: &Instance,
    uids: &[&str],
) -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    write(
        &directory.0.join("project-original.json"),
        &serde_json::to_string_pretty(project)?,
    )?;
    let validation = engine::validate(project);
    assert!(validation.is_empty(), "{validation:?}");
    assert_eq!(
        &engine::run_with_sources(project, &primary, extras.clone())?,
        expected
    );
    let shape = join_shape(project)?;
    write(
        &directory.0.join("orders.xml"),
        &format_xml::to_string(&project.source, &primary)?,
    )?;
    for source in &project.extra_sources {
        let (_, input) = extras
            .iter()
            .find(|(name, _)| name == &source.name)
            .ok_or("missing named input")?;
        write(
            &directory.0.join(&source.path),
            &format_xml::to_string(&source.schema, input)?,
        )?;
    }
    for (label, profile) in [
        ("default", mfd::ExportProfile::FerruleExtensions),
        ("native", mfd::ExportProfile::NativeMfd),
    ] {
        let mut current = project.clone();
        for cycle in 0..2 {
            let name = format!("{label}-{cycle}");
            let design = directory.0.join(format!("{name}.mfd"));
            let exported = mfd::export_with_profile(&current, &design, profile);
            retain(&directory.0, &format!("{name}-export.debug"), &exported)?;
            let report = exported?;
            assert!(report.warnings.is_empty(), "{name}: {:?}", report.warnings);
            assert!(report.is_native_compatible());
            validate_inputs_xml(&std::fs::read_to_string(&design)?, uids)?;
            let imported = mfd::import_with_profile(
                &design,
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable,
            )?;
            assert!(imported.report.executable);
            assert!(
                imported.imported.warnings.is_empty(),
                "{name}: {:?}",
                imported.imported.warnings
            );
            current = imported.imported.project;
            write(
                &directory.0.join(format!("{name}-after.json")),
                &serde_json::to_string_pretty(&current)?,
            )?;
            let validation = engine::validate(&current);
            assert!(validation.is_empty(), "{name}: {validation:?}");
            assert_eq!(join_shape(&current)?, shape, "{name}");
            let primary_path = directory.0.join(
                current
                    .source_path
                    .as_deref()
                    .ok_or("missing primary identity")?,
            );
            let decoded_primary =
                format_xml::from_str(&std::fs::read_to_string(&primary_path)?, &current.source)?;
            assert_eq!(decoded_primary, primary);
            let mut decoded_extras = Vec::new();
            for source in &current.extra_sources {
                let decoded = format_xml::from_str(
                    &std::fs::read_to_string(directory.0.join(&source.path))?,
                    &source.schema,
                )?;
                decoded_extras.push((source.name.clone(), decoded));
            }
            assert_eq!(decoded_extras, extras, "{name}");
            assert_eq!(
                &engine::run_with_sources(&current, &decoded_primary, decoded_extras)?,
                expected,
                "{name}"
            );
        }
    }
    Ok(())
}

#[test]
fn nested_two_input_join_with_a_named_first_singleton_roundtrips() -> Result<(), Box<dyn Error>> {
    let plan = JoinPlan::new(
        region(),
        customers(),
        key(&["Settings", "Region"], &[], &["Region"]),
    )?;
    let every = || names(&["Bee1", "Ay"]);
    roundtrip(
        &shape_project(plan, false)?,
        &output([every(), every(), every(), every(), every()]),
        &["4", "3"],
    )
}

#[test]
fn nested_two_input_join_with_a_trailing_primary_singleton_roundtrips() -> Result<(), Box<dyn Error>>
{
    let plan = JoinPlan::new(
        customers(),
        customer_number(),
        key(&["Customers", "Customer"], &["Number"], &[]),
    )?;
    roundtrip(
        &shape_project(plan, false)?,
        &output([
            names(&["Bee1", "Bee2"]),
            names(&["Ay"]),
            vec![],
            vec![],
            vec![],
        ]),
        &["3", "2"],
    )
}

#[test]
fn nested_three_input_join_with_middle_and_named_singletons_roundtrips()
-> Result<(), Box<dyn Error>> {
    roundtrip(
        &shape_project(middle_and_named_plan()?, false)?,
        &output([names(&["Bee1"]), names(&["Ay"]), vec![], vec![], vec![]]),
        &["3", "2", "4"],
    )
}

fn middle_and_named_plan() -> Result<JoinPlan, mapping::JoinPlanError> {
    JoinPlan::new(
        customers(),
        customer_number(),
        key(&["Customers", "Customer"], &["Number"], &[]),
    )?
    .then(region(), key(&["Customers", "Customer"], &["Region"], &[]))
}

#[test]
fn nested_four_input_join_with_a_named_last_singleton_roundtrips() -> Result<(), Box<dyn Error>> {
    let plan = three_plan()?.then(region(), key(&["Offers", "Offer"], &["Region"], &[]))?;
    roundtrip(
        &shape_project(plan, true)?,
        &output([
            offers(&[("Bee1", "P1"), ("Bee1", "P2")]),
            offers(&[("Ay", "PA")]),
            vec![],
            vec![],
            vec![],
        ]),
        &["2", "3", "4", "5"],
    )
}

#[test]
fn nested_named_singleton_sharing_a_repeating_component_roundtrips() -> Result<(), Box<dyn Error>> {
    let mut project = three_project()?;
    append_schema_field(
        &mut project.extra_sources[0].schema,
        SchemaNode::scalar("CustomerNumber", ScalarType::String),
    );
    let number = &["Customers", "CustomerNumber"];
    let plan = JoinPlan::new(
        JoinSource::singleton(p(number)),
        customers(),
        key(number, &[], &["Number"]),
    )?
    .then(
        JoinSource::new(p(&["Offers", "Offer"])),
        key(&["Customers", "Customer"], &["Region"], &["Region"]).and(JoinKey::new(
            p(number),
            vec![],
            p(&["Number"]),
        )),
    )?;
    replace_plan(&mut project, plan);
    let (primary, mut extras) = shape_inputs(&project);
    let (_, customers_input) = extras
        .iter_mut()
        .find(|(name, _)| name == "Customers")
        .ok_or("missing Customers input")?;
    let Instance::Group(fields) = customers_input else {
        return Err("Customers input is not a group".into());
    };
    fields.push((
        "CustomerNumber".into(),
        Instance::Scalar(Value::String("B".into())),
    ));
    // The singleton reads the named document, so every order sees `B`.
    let every = || offers(&[("Bee1", "P1"), ("Bee1", "P2"), ("Bee2", "P3")]);
    roundtrip_inputs(
        &project,
        (primary, extras),
        &output([every(), every(), every(), every(), every()]),
        &["3", "3", "4"],
    )
}

/// Removes every joined projection so a refused plan needs no matching binding.
fn without_projections(project: &mut Project) {
    project.graph.nodes.retain(|id, _| *id == 1);
    nested_scope(project).bindings.clear();
}

#[test]
fn nested_two_input_join_of_two_singletons_keeps_its_refusal() -> Result<(), Box<dyn Error>> {
    let reason = "nested join requires at least one repeating source";
    let mut project = shape_project(
        JoinPlan::new(
            customer_number(),
            region(),
            key(&["CustomerNumber"], &[], &[]),
        )?,
        false,
    )?;
    without_projections(&mut project);
    assert_refused(&project, reason, 0)?;
    let directory = TempDir::new()?;
    let warnings = mfd::export(&project, &directory.0.join("mapping.mfd"))?;
    assert!(
        warnings.contains(&format!(
            "inner join 777 is not exported: {reason}; its iteration and node connections are skipped"
        )),
        "{warnings:?}"
    );
    Ok(())
}

#[test]
fn nested_named_and_repositioned_singletons_keep_owner_shadow_and_anchor_guards()
-> Result<(), Box<dyn Error>> {
    let mut owner_collision = shape_project(middle_and_named_plan()?, false)?;
    owner_collision.extra_sources.push(NamedSource {
        name: "Order".into(),
        path: "shadow.xml".into(),
        schema: string_fields("Shadow", &["Id", "CustomerNumber"]),
        options: Default::default(),
        dynamic_path: None,
    });
    assert_refused(
        &owner_collision,
        "anchor `Order` names both a primary source child and a named source",
        0,
    )?;
    for active in [false, true] {
        let mut shadowed = shape_project(middle_and_named_plan()?, false)?;
        let collision = settings_source().schema;
        let SchemaKind::Group { children, .. } = &mut shadowed.source.kind else {
            panic!("fixture group required")
        };
        if active {
            let order = children
                .iter_mut()
                .find(|field| field.name == "Order")
                .ok_or("missing driver schema")?;
            append_schema_field(order, collision);
        } else {
            children.push(collision);
        }
        assert_refused(
            &shadowed,
            "source `Settings` is shadowed by a primary source frame",
            0,
        )?;
    }
    let mut stripped = shape_project(middle_and_named_plan()?, false)?;
    let SchemaKind::Group { children, .. } = &mut stripped.source.kind else {
        panic!("fixture group required")
    };
    children
        .iter_mut()
        .find(|field| field.name == "Order")
        .ok_or("missing driver schema")?
        .name = "Settings".into();
    stripped.root.children[0].iteration = ScopeIteration::Source(p(&["Settings"]));
    let Some(Node::SourceField { frame, .. }) = stripped.graph.nodes.get_mut(&1) else {
        panic!("fixture field required")
    };
    *frame = Some(p(&["Settings"]));
    assert_refused(
        &stripped,
        "collection would change when its enclosing anchor is removed",
        0,
    )
}
