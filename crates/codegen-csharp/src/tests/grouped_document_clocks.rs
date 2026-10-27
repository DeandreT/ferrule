#[path = "../../../../conformance/document-body-clocks/project.rs"]
#[allow(dead_code)]
mod body;
#[path = "../../../../conformance/document-clocks/project.rs"]
mod clocks;

#[test]
fn early_grouped_filename_order_does_not_change_ordinary_visitors() {
    let program = codegen::lower(&body::project(clocks::project())).unwrap();
    let early = crate::emit_with_static_error_document_clocks(&program).unwrap();
    let late = crate::emit_with_document_clocks(&program, None).unwrap();
    for (artifacts, filename_first) in [(early, true), (late, false)] {
        let source = artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.cs")
            .unwrap();
        let text = std::str::from_utf8(&source.contents).unwrap();
        let method = &text[text.find("private static void VisitDocumentScope").unwrap()..];
        let filename = method.find("var output_path_").unwrap();
        let body = method.find(" = ScopeItem_").unwrap();
        assert_eq!(filename < body, filename_first);
    }
}
