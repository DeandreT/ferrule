use std::path::{Path, PathBuf};

use ir::{ScalarType, SchemaKind};
use mapping::Node;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn imports_static_flat_xlsx_table_with_sparse_columns() {
    let imported = mfd::import(&fixture("xlsx-flat.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    assert_eq!(project.source_path.as_deref(), Some("scores.xlsx"));
    assert_eq!(project.source_options.xlsx_sheet.as_deref(), Some("Data"));
    assert_eq!(project.source_options.xlsx_start_row, Some(2));
    assert_eq!(project.source_options.xlsx_columns, vec![2, 4]);
    assert_eq!(project.source_options.has_header_row, Some(true));
    assert!(matches!(
        project.source.child("Name").map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::String
        })
    ));
    assert!(matches!(
        project.source.child("Age").map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    ));

    let person = project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Person")
        .unwrap();
    assert_eq!(person.source().map(|path| path.to_vec()), Some(Vec::new()));
    assert!(person.bindings.iter().any(|binding| {
        binding.target_field == "Name"
            && matches!(
                project.graph.nodes.get(&binding.node),
                Some(Node::SourceField { path, .. }) if path == &["Name"]
            )
    }));
    assert!(person.bindings.iter().any(|binding| {
        binding.target_field == "Age"
            && matches!(
                project.graph.nodes.get(&binding.node),
                Some(Node::SourceField { path, .. }) if path == &["Age"]
            )
    }));
    assert!(engine::validate(project).is_empty());
}
