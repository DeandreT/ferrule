use ir::{Instance, Value};
use mapping::Node;

use super::{fixture, scalar};

#[test]
fn scalar_multi_output_udf_imports_and_runs() {
    let imported = mfd::import(&fixture("scalar-udf.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);

    let project = &imported.project;
    let contact = &project.root.children[0];
    assert_eq!(contact.target_field, "Contact");
    assert_eq!(contact.bindings.len(), 2);
    let functions: Vec<_> = contact
        .bindings
        .iter()
        .filter_map(|binding| match &project.graph.nodes[&binding.node] {
            Node::Call { function, .. } => Some(function.as_str()),
            _ => None,
        })
        .collect();
    assert!(functions.contains(&"substring_before"));
    assert!(functions.contains(&"substring_after"));

    let source = format_xml::read(&fixture("udf.xml"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    let contacts = target
        .field("Contact")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(contacts.len(), 2);
    assert_eq!(scalar(&contacts[0], "First"), Value::String("Ada".into()));
    assert_eq!(
        scalar(&contacts[0], "Last"),
        Value::String("Lovelace".into())
    );
    assert_eq!(scalar(&contacts[1], "First"), Value::String("Grace".into()));
    assert_eq!(scalar(&contacts[1], "Last"), Value::String("Hopper".into()));
}

#[test]
fn scalar_filter_udf_outputs_are_complementary_nullable_values() {
    let imported = mfd::import(&fixture("scalar-filter-udf.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let functions: Vec<_> = imported
        .project
        .graph
        .nodes
        .values()
        .filter_map(|node| match node {
            Node::Call { function, .. } => Some(function.as_str()),
            _ => None,
        })
        .collect();
    assert!(functions.contains(&"normalize_space"));
    assert!(functions.contains(&"is_empty"));
    assert!(
        imported
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::If { .. }))
    );

    let source = format_xml::from_str(
        "<Names><Person><Full>  Ada  </Full></Person><Person><Full> \t </Full></Person></Names>",
        &imported.project.source,
    )
    .unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    let contacts = target
        .field("Contact")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(contacts.len(), 2);
    assert_eq!(
        scalar(&contacts[0], "First"),
        Value::String("  Ada  ".into())
    );
    assert_eq!(scalar(&contacts[0], "Last"), Value::Null);
    assert_eq!(scalar(&contacts[1], "First"), Value::Null);
    assert_eq!(scalar(&contacts[1], "Last"), Value::String(" \t ".into()));

    let output = format_xml::to_string(&imported.project.target, &target).unwrap();
    assert_eq!(output.matches("<First>").count(), 1, "{output}");
    assert_eq!(output.matches("<Last>").count(), 1, "{output}");
}

#[test]
fn nullable_passthrough_udf_filters_a_nested_group_iteration() {
    let imported = mfd::import(&fixture("scalar-filter-iteration.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let validation = engine::validate(&imported.project);
    assert!(validation.is_empty(), "{validation:?}");

    let bucket = &imported.project.root.children[0];
    let result = &bucket.children[0];
    assert_eq!(result.target_field, "Result");
    assert_eq!(result.source(), Some(["Item".to_string()].as_slice()));
    assert!(result.filter.is_some());

    let source = format_xml::from_str(
        "<Groups><Bucket><Label>A</Label><Item><Name>First</Name><Value>1</Value></Item><Item><Name>   </Name><Value>2</Value></Item><Item><Name>Third</Name><Value>3</Value></Item></Bucket></Groups>",
        &imported.project.source,
    )
    .unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    let buckets = target
        .field("Bucket")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(buckets.len(), 1);
    let results = buckets[0]
        .field("Result")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(scalar(&results[0], "Name"), Value::String("First".into()));
    assert_eq!(scalar(&results[1], "Value"), Value::String("3".into()));
}
