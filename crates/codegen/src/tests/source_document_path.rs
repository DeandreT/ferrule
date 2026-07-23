use super::*;

#[test]
fn lowers_current_document_path_for_local_xml_file_sets() {
    let mut project = supported_project();
    project.source_path = Some("records-*.xml".into());
    project.source_options = mapping::FormatOptions {
        xml_document: true,
        local_xml_file_set: true,
        ..mapping::FormatOptions::default()
    };
    project.graph.nodes.insert(40, Node::SourceDocumentPath);
    project.root.bindings[1].node = 40;

    let Ok(program) = lower(&project) else {
        panic!("validated current-document-path lowers")
    };

    assert!(program.expressions.iter().any(|expression| {
        expression.id == 40 && expression.expression == Expression::SourceDocumentPath
    }));
}
