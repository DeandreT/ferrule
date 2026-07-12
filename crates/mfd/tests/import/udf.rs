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
