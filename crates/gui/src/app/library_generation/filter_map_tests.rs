use super::*;
use mapping::{Scope, ScopeIteration, SequenceExpr};

fn descriptor() -> SequenceExpr {
    SequenceExpr::FilterMapV1(mapping::FilterMapV1 {
        source: Box::new(SequenceExpr::Generate {
            from: Some(1),
            to: 2,
            item: 10,
        }),
        item: 11,
        predicate: mapping::FunctionId::new(100),
        mapper: mapping::FunctionId::new(101),
        output_type: ir::ScalarType::Int,
        captures: Vec::new(),
    })
}

#[test]
fn qualified_backends_generation_keep_save_worker_and_frozen_language_guards() {
    for context in 0..3 {
        for language in [LibraryLanguage::Rust, LibraryLanguage::CSharp] {
            let mut app = FerruleApp::default();
            match context {
                0 => app.project.root.children.push(Scope {
                    target_field: "Rows".into(),
                    iteration: ScopeIteration::Sequence(descriptor()),
                    ..Default::default()
                }),
                1 => {
                    app.project.graph.nodes.insert(
                        50,
                        mapping::Node::SequenceItemAt {
                            sequence: descriptor(),
                            index: 1,
                        },
                    );
                }
                _ => app.project.failure_rules.push(mapping::FailureRule {
                    iteration: mapping::FailureIteration::Sequence {
                        sequence: descriptor(),
                    },
                    selection: mapping::FailureSelection::All,
                    message: None,
                }),
            }
            let before = mapping::project_file::encode_pretty(&app.project).unwrap();
            app.begin_library_generation();
            app.library_generation_draft.as_mut().unwrap().language = language;
            if language == LibraryLanguage::CSharp {
                app.library_generation_draft
                    .as_mut()
                    .unwrap()
                    .destination
                    .clear();
            }
            let gate = filter_map_generation_reason(&app.project, language);
            eprintln!(
                "generation feature-gate context={context} language={language:?} original={gate:?}"
            );
            assert_eq!(gate, None);
            app.request_library_generation(&egui::Context::default());
            let request_project = mapping::project_file::encode_pretty(&app.project).unwrap();
            let request_status = app.status.clone();
            let request_messages = app
                .diagnostics
                .items()
                .iter()
                .map(|entry| entry.message.clone())
                .collect::<Vec<_>>();
            let request_pending = (
                app.pending_library_generation.is_some(),
                app.pending_dialog.is_some(),
                app.pending_save_continuation.is_some(),
            );
            eprintln!(
                "generation request context={context} language={language:?} complete-project={request_project} status={request_status:?} diagnostics={:#?} pending={request_pending:?}",
                app.diagnostics.items()
            );
            app.generate_saved_library();
            let after = mapping::project_file::encode_pretty(&app.project).unwrap();
            eprintln!(
                "generation context={context} language={language:?} before={before}\nafter={after}\nstatus={} diagnostics={:#?} pending_worker={} pending_dialog={} pending_save={}",
                app.status,
                app.diagnostics.items(),
                app.pending_library_generation.is_some(),
                app.pending_dialog.is_some(),
                app.pending_save_continuation.is_some()
            );
            assert_eq!(before, after);
            assert!(app.pending_library_generation.is_none());
            assert!(app.pending_dialog.is_none());
            assert!(app.pending_save_continuation.is_none());
            let messages = app
                .diagnostics
                .items()
                .iter()
                .map(|entry| entry.message.as_str())
                .collect::<Vec<_>>();
            assert_eq!(app.status, "library generation failed");
            assert_eq!(request_project, before);
            assert_eq!(request_status, "library generation failed");
            assert_eq!(request_pending, (false, false, false));
            match language {
                LibraryLanguage::Rust => {
                    assert_eq!(
                        request_messages,
                        ["Choose the codegen-runtime folder for the Rust library."]
                    );
                    assert_eq!(messages, ["Save the mapping before generating a library."]);
                }
                LibraryLanguage::CSharp => {
                    assert_eq!(
                        request_messages,
                        ["Enter a new folder for the generated library."]
                    );
                    assert_eq!(messages, ["Save the mapping before generating a library."]);
                }
            }
            // A save continuation uses the original frozen request language,
            // even if the visible draft language has subsequently changed.
            for (visible, pending, expected_gate) in [
                (LibraryLanguage::CSharp, LibraryLanguage::Rust, None),
                (LibraryLanguage::Rust, LibraryLanguage::CSharp, None),
            ] {
                let draft = app.library_generation_draft.as_mut().unwrap();
                draft.language = visible;
                draft.pending_settings = Some(LibraryGenerationSettings {
                    language: pending,
                    destination: "fresh-library".into(),
                    runtime_path: "runtime".into(),
                    include_csv_output: false,
                });
                let actual_language = draft.generation_language_after_save();
                let actual_gate = filter_map_generation_reason(&app.project, actual_language);
                eprintln!(
                    "frozen-language visible={visible:?} pending={pending:?} original-effective={actual_language:?} original-gate={actual_gate:?} complete-project={after}"
                );
                assert_eq!(actual_language, pending);
                assert_eq!(actual_gate, expected_gate);
            }
        }
    }
    let legacy = crate::new_mapping::blank_project();
    for language in [LibraryLanguage::Rust, LibraryLanguage::CSharp] {
        let actual = filter_map_generation_reason(&legacy, language);
        eprintln!(
            "ordinary legacy generation language={language:?} availability original={actual:?}"
        );
        assert_eq!(actual, None);
    }
}
