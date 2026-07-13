use std::path::{Path, PathBuf};

use ir::{ScalarType, SchemaKind};
use mapping::{AggregateOp, Node};

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

#[test]
fn imports_fixed_rows_with_open_cells_as_a_transposed_table() {
    let imported = mfd::import(&fixture("xlsx-transposed.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    assert_eq!(project.source_path.as_deref(), Some("ledger.xlsx"));
    assert_eq!(
        project.source_options.xlsx_sheet.as_deref(),
        Some("Quarterly")
    );
    assert_eq!(project.source_options.xlsx_rows, vec![3, 7]);
    assert!(project.source_options.xlsx_columns.is_empty());
    assert!(project.source_options.xlsx_start_row.is_none());
    assert_eq!(project.source_options.has_header_row, Some(false));
    assert!(matches!(
        project.source.child("Category").map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::String
        })
    ));
    assert!(matches!(
        project.source.child("Range9").map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    ));
    assert!(matches!(
        project.source.child("n").map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    ));

    assert_eq!(project.root.source(), Some(&[][..]));
    assert!(project.root.bindings.iter().any(|binding| {
        binding.target_field == "Category"
            && matches!(
                project.graph.nodes.get(&binding.node),
                Some(Node::SourceField { path, .. }) if path == &["Category"]
            )
    }));
    let amount = project
        .root
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Amount")
        .unwrap();
    let argument = match project.graph.nodes.get(&amount.node) {
        Some(Node::Aggregate {
            function: AggregateOp::ItemAt,
            collection,
            value,
            arg: Some(argument),
            ..
        }) if collection.is_empty() && value == &["Range9"] => *argument,
        other => panic!("unexpected Amount expression: {other:?}"),
    };
    assert!(matches!(
        project.graph.nodes.get(&argument),
        Some(Node::SourceField { path, .. }) if path == &["n"]
    ));
    assert!(engine::validate(project).is_empty());
}
