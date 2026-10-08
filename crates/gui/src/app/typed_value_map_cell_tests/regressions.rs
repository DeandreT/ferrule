use super::*;

#[test]
fn typed_value_map_cell_edit_preserves_real_input_error_identity_before_lookup_in_each_context() {
    let mut retained = Retained::new();
    for document in DOCUMENTS {
        let (mut app, context) = setup(
            &retained,
            Value::Int(7),
            vec![(Value::String("7".into()), Value::String("mapped".into()))],
            Some(Value::String("fallback".into())),
            ScalarType::String,
        );
        app.mapping_workspace.active = document;
        open(&mut app, &context, &retained);
        choose_kind(&mut app, &context, &retained, "Entry 1 key", "int");
        apply(&mut app, &context, &retained, "Entry 1 key", false);
        let graph = selected_graph_mut(&mut app, document);
        graph.nodes.insert(
            0,
            Node::Call {
                function: "divide".into(),
                args: vec![5, 6],
            },
        );
        graph.nodes.insert(
            5,
            Node::Const {
                value: Value::Int(1),
            },
        );
        graph.nodes.insert(
            6,
            Node::Const {
                value: Value::Int(0),
            },
        );
        let validation = engine::validate(&app.project);
        let parsed = format_json::from_str(INPUT, &app.project.source);
        retained.record(
            "full-error-parse-validation",
            (&app.project, &validation, &parsed),
        );
        let actual = engine::run_outputs(&app.project, &parsed.unwrap());
        retained.record("full-original-input-error-result", &actual);
        assert!(validation.is_empty());
        let error = actual.unwrap_err();
        retained.record(
            "complete-error-display-cause",
            (
                &error,
                error.to_string(),
                std::error::Error::source(&error).map(|source| format!("{source:#?}\n{source}")),
            ),
        );
        if matches!(document, MappingDocument::Function(_)) {
            assert!(
                matches!(error,engine::EngineError::UserFunctionBuiltin{function,node:0,source:functions::FunctionError::DivideByZero} if function==FUNCTION)
            );
        } else {
            assert!(matches!(
                error,
                engine::EngineError::Function(functions::FunctionError::DivideByZero)
            ));
        }
    }
    retained.complete = true;
}

#[test]
fn typed_value_map_real_remove_invalidates_equal_row_key_and_value_drafts_before_surviving_apply()
-> anyhow::Result<()> {
    let mut retained = Retained::new();
    for document in DOCUMENTS {
        let table = vec![
            (Value::Int(7), Value::String("first".into())),
            (Value::Int(7), Value::String("second".into())),
            (Value::Int(7), Value::String("third".into())),
        ];
        let default = Some(Value::String("unused default".into()));
        let (mut app, context) = setup(
            &retained,
            Value::Int(7),
            table.clone(),
            default.clone(),
            ScalarType::String,
        );
        app.mapping_workspace.active = document;
        open(&mut app, &context, &retained);
        app.mark_clean();
        app.rebase_history();
        let before = original(&app, &retained, "remove-before-full-project");
        let before_layout = layout(&app);
        let mut expected_project = app.project.clone();
        let graph = match document {
            MappingDocument::Main | MappingDocument::Target(_) => &mut expected_project.graph,
            MappingDocument::Function(id) => {
                &mut expected_project.user_functions.get_mut(&id).unwrap().body
            }
        };
        let Node::ValueMap {
            table: expected_table,
            ..
        } = graph.nodes.get_mut(&1).unwrap()
        else {
            panic!("expected Value map")
        };
        expected_table.remove(0);
        choose_kind(&mut app, &context, &retained, "Entry 1 key", "string");
        enter_text(&mut app, &context, &retained, "Entry 1 key", "replacement");
        choose_kind(&mut app, &context, &retained, "Entry 1 value", "int");
        enter_text(&mut app, &context, &retained, "Entry 1 value", "99");
        assert_eq!(
            original(&app, &retained, "both-staged-removed-row-original"),
            before
        );
        assert_eq!(cells(&app, document), (table.clone(), default.clone()));
        assert!(!app.is_dirty() && !app.can_undo());
        let output = settle(&mut app, &context, &retained, true);
        let mut points = visible(&output, &char::from(lucide_icons::Icon::Trash2).to_string());
        retained.record(
            "full-remove-row-pointer-candidates",
            (&output.shapes, &points),
        );
        assert_eq!(points.len(), 3, "all three real row Remove buttons visible");
        points.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
        assert!(
            points[0].x < points[1].x
                && (points[0].y - points[1].y).abs() < 1.0
                && points[2].y > points[0].y,
            "actual noncompact grid first/second/third row actions"
        );
        click(&mut app, &context, &retained, true, points[0]);
        let surviving = vec![
            (Value::Int(7), Value::String("second".into())),
            (Value::Int(7), Value::String("third".into())),
        ];
        let after = original(&app, &retained, "remove-after-surviving-full-project");
        let expected = serde_json::to_value(&expected_project);
        retained.record("complete-independent-remove-project-codec", &expected);
        assert_eq!(after, expected?);
        assert_eq!(cells(&app, document), (surviving.clone(), default.clone()));
        assert!(app.is_dirty() && app.can_undo());
        assert_eq!(app.history.undo_len(), 1);
        apply(&mut app, &context, &retained, "Entry 1 key", false);
        apply(&mut app, &context, &retained, "Entry 1 value", false);
        assert_eq!(
            original(&app, &retained, "surviving-row-after-both-real-apply"),
            after
        );
        assert_eq!(cells(&app, document), (surviving.clone(), default.clone()));
        assert_eq!(app.history.undo_len(), 1);
        let (result, function) = pair(
            document,
            Value::String("second".into()),
            Value::String("first".into()),
        );
        outcomes(
            &app,
            &retained,
            result,
            function,
            if matches!(document, MappingDocument::Function(_)) {
                "{\n  \"Result\": \"first\",\n  \"Function\": \"second\"\n}\n"
            } else {
                "{\n  \"Result\": \"second\",\n  \"Function\": \"first\"\n}\n"
            },
        )?;
        close(&mut app, &context, &retained);
        assert_eq!(layout(&app), before_layout);
        history_key(&mut app, &context, &retained, false);
        assert_eq!(
            original(&app, &retained, "remove-real-undo-original"),
            before
        );
        assert_eq!(cells(&app, document), (table, default.clone()));
        assert!(!app.is_dirty());
        history_key(&mut app, &context, &retained, true);
        assert_eq!(
            original(&app, &retained, "remove-real-redo-original"),
            after
        );
        assert_eq!(cells(&app, document), (surviving, default));
        assert_eq!(layout(&app), before_layout);
    }
    retained.complete = true;
    Ok(())
}
