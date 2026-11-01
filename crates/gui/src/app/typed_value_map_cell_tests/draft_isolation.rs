use super::*;

fn activate(app: &mut FerruleApp, document: MappingDocument) {
    match document {
        MappingDocument::Main => {}
        MappingDocument::Target(index) => {
            app.open_target_tab(index);
            assert!(app.ensure_target_canvas(index));
        }
        MappingDocument::Function(id) => {
            app.open_function_tab(id);
            assert!(app.ensure_function_canvas(id));
        }
    }
    app.mapping_workspace.active = document;
}

fn text_value(output: &egui::FullOutput, label: &str) -> String {
    let update = output.platform_output.accesskit_update.as_ref().unwrap();
    let nodes = update
        .nodes
        .iter()
        .filter_map(|(_, node)| {
            (node.role() == egui::accesskit::Role::TextInput
                && node.label() == Some(&format!("{label} value")))
            .then_some(node)
        })
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 1, "one current text field for {label}");
    nodes[0].value().unwrap_or_default().to_owned()
}

#[test]
fn value_map_cell_drafts_do_not_cross_document_replacements() {
    let mut retained = Retained::new();
    let (mut app, context) = setup(
        &retained,
        Value::Int(7),
        vec![(Value::Int(7), Value::Int(9))],
        Some(Value::Int(11)),
        ScalarType::Int,
    );
    let a = retained.path.join("a.json");
    let b = retained.path.join("b.json");
    app.document = DocumentLocation::untitled(a.clone());
    app.save_document_to(&a).unwrap();
    std::fs::copy(&a, &b).unwrap();
    std::fs::copy(
        crate::layout_store::layout_path(&a),
        crate::layout_store::layout_path(&b),
    )
    .unwrap();
    let unrelated = egui::Id::unique("unrelated-value-map-reload-state");
    context.data_mut(|data| data.insert_temp(unrelated, "keep".to_owned()));
    for document in DOCUMENTS {
        for (label, original) in [
            ("Entry 1 key", "7"),
            ("Entry 1 value", "9"),
            ("Default", "11"),
        ] {
            for draft in ["-", "123"] {
                app.load_project_from(&a);
                activate(&mut app, document);
                let output = settle(&mut app, &context, &retained, true);
                if visible(&output, "Input conversion").is_empty() {
                    open(&mut app, &context, &retained);
                }
                let before = cells(&app, document);
                enter_text(&mut app, &context, &retained, label, draft);
                let output = settle(&mut app, &context, &retained, true);
                assert_eq!(text_value(&output, label), draft);
                assert_eq!(
                    cells(&app, document),
                    before,
                    "unapplied draft must stay local"
                );
                let old_owner = canvas(&app, document).tools_id;
                app.load_project_from(&b);
                activate(&mut app, document);
                let output = settle(&mut app, &context, &retained, true);
                if visible(&output, "Input conversion").is_empty() {
                    open(&mut app, &context, &retained);
                }
                let output = settle(&mut app, &context, &retained, true);
                assert_ne!(canvas(&app, document).tools_id, old_owner);
                assert_eq!(
                    text_value(&output, label),
                    original,
                    "stale {document:?} {label} draft"
                );
                assert_eq!(cells(&app, document), before);
                let saved = crate::project_state::project_snapshot_key(&app.project);
                apply(&mut app, &context, &retained, label, false);
                assert_eq!(
                    crate::project_state::project_snapshot_key(&app.project),
                    saved
                );
                assert!(!app.can_undo());
                assert!(!app.is_dirty());
                assert_eq!(
                    context
                        .data(|data| data.get_temp::<String>(unrelated))
                        .as_deref(),
                    Some("keep")
                );
                close(&mut app, &context, &retained);
            }
        }
    }
    retained.complete = true;
}
