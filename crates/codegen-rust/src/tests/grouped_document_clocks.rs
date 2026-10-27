#[path = "../../../../conformance/document-body-clocks/project.rs"]
#[allow(dead_code)]
mod body;
#[path = "../../../../conformance/document-clocks/project.rs"]
mod clocks;

#[test]
fn early_grouped_filename_order_does_not_change_ordinary_visitors() {
    let program = codegen::lower(&body::project(clocks::project())).unwrap();
    let options = crate::Options {
        package_name: "early-document-clock-test".into(),
        runtime_dependency: crate::RuntimeDependency::Version("0.1.0".into()),
    };
    let early = crate::document_visitor::emit_with_static_error_document_clocks(&program, &options)
        .unwrap();
    let late = crate::emit_with_document_clocks(&program, &options, None).unwrap();
    for (artifacts, filename_first) in [(early, true), (late, false)] {
        let source = artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "src/lib.rs")
            .unwrap();
        let text = std::str::from_utf8(&source.contents).unwrap();
        let method = &text[text.find("fn visit_document_scope").unwrap()..];
        let filename = method.find("let output_path =").unwrap();
        let body = method.find("let output =").unwrap();
        assert_eq!(filename < body, filename_first);
    }
}
