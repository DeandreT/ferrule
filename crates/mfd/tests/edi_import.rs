use std::path::{Path, PathBuf};

use mapping::Node;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn imports_edi_entry_tree_paths_and_honors_default_output() {
    let imported = mfd::import(&fixture("edi-entry-tree.mfd")).unwrap();
    let project = &imported.project;

    assert_eq!(project.source.name, "MFD-EDIFACT");
    assert_eq!(project.source_path.as_deref(), Some("orders.edi"));
    assert!(project.source_options.lenient_segments);
    assert_eq!(project.target.name, "People");
    assert_eq!(project.target_path.as_deref(), Some("people.xml"));
    assert_eq!(project.extra_targets.len(), 1);
    assert_eq!(project.extra_targets[0].name, "ignored");
    assert_eq!(project.extra_targets[0].schema.name, "Ignored");
    assert_eq!(project.extra_targets[0].root.bindings.len(), 1);
    assert_eq!(
        project.extra_targets[0].root.bindings[0].target_field,
        "Value"
    );

    let interchange = project.source.child("Interchange").unwrap();
    assert!(interchange.repeating);
    let group = interchange.child("Group").unwrap();
    assert!(group.repeating);
    let message = group.child("Message").unwrap();
    assert!(message.repeating);
    let sg2 = message.child("SG2").unwrap();
    assert!(sg2.repeating);
    assert!(message.child("SG3").is_none());

    let person = &project.root.children[0];
    assert_eq!(person.target_field, "Person");
    assert_eq!(
        person.source(),
        Some(
            ["Interchange", "Group", "Message", "SG2"]
                .map(String::from)
                .as_slice()
        )
    );
    assert!(person.bindings.iter().all(|binding| {
        matches!(
            project.graph.nodes.get(&binding.node),
            Some(Node::SourceField { .. })
        )
    }));

    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("entry-tree schema inferred") && warning.contains("execution is disabled")
    }));
}

#[test]
fn imports_unsupported_edi_dialect_as_non_executable_graph() {
    let imported = mfd::import(&fixture("edi-unsupported.mfd")).unwrap();

    assert_eq!(imported.project.source.name, "HL7");
    assert_eq!(
        imported.project.source_path.as_deref(),
        Some("messages.hl7")
    );
    assert_eq!(imported.project.graph.nodes.len(), 1);
    assert_eq!(imported.warnings.len(), 2, "{:?}", imported.warnings);
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("entry-tree schema inferred"))
    );
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("mapping graph was imported")
            && warning.contains("only EDIX12 and EDIFACT")
    }));
}
