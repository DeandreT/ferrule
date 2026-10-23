//! Child scope construction and XML type edits from the Inspector recorded
//! as typed commands, checked against the snapshot history they replace.

use editor_ui::{GraphAction, GraphDocument, ScopeLocation, TargetId};
use ir::{ScalarType, SchemaNode, Value};
use mapping::{NamedTarget, ScopeIteration, SequenceExpr};

use super::typed_edit_oracle::{
    assert_stale_undo_refused, assert_typed_edit_matches_snapshots, newest_command,
};
use super::*;

const ITEM: NodeId = 5;

fn tree_schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::scalar("Code", ScalarType::String),
            SchemaNode::group(
                "Header",
                vec![
                    SchemaNode::scalar("Title", ScalarType::String),
                    SchemaNode::group("Note", vec![SchemaNode::scalar("Text", ScalarType::String)]),
                ],
            ),
            SchemaNode::group(
                "Lines",
                vec![
                    SchemaNode::scalar("Value", ScalarType::Int),
                    SchemaNode::group(
                        "Detail",
                        vec![SchemaNode::scalar("Text", ScalarType::String)],
                    ),
                ],
            )
            .repeating(),
        ],
    )
}

fn scope(field: &str) -> Scope {
    Scope {
        target_field: field.into(),
        ..Scope::default()
    }
}

/// `Lines` iterates a generated sequence whose item node feeds its `Value`.
fn generated_lines() -> Scope {
    Scope {
        iteration: ScopeIteration::Sequence(SequenceExpr::Generate {
            from: Some(0),
            to: 1,
            item: ITEM,
        }),
        bindings: vec![mapping::Binding {
            target_field: "Value".into(),
            node: ITEM,
        }],
        ..scope("Lines")
    }
}

/// A primary target, and a named copy when `document` is a named target,
/// with `root` as their root scope; the root scope is selected.
fn tree_app(document: MappingDocument, root: Scope) -> FerruleApp {
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group("Input", Vec::new());
    app.project.target = tree_schema("Output");
    app.project.source_path = Some("input.json".into());
    app.project.target_path = Some("output.json".into());
    app.project.source_options.json_document = true;
    app.project.target_options.json_document = true;
    app.project.graph.nodes.extend([
        (
            0,
            Node::Const {
                value: Value::Int(1),
            },
        ),
        (
            1,
            Node::Const {
                value: Value::Int(3),
            },
        ),
        (
            ITEM,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
    ]);
    if let MappingDocument::Target(_) = document {
        app.project.extra_targets.push(NamedTarget {
            name: "Copy".into(),
            path: Some("copy.json".into()),
            schema: tree_schema("Copy"),
            options: app.project.target_options.clone(),
            root: root.clone(),
        });
    } else {
        app.project.root = root;
    }
    app.main_canvas = CanvasDocumentState::main(&app.project);
    if let MappingDocument::Target(index) = document {
        app.open_target_tab(index);
        assert!(app.ensure_target_canvas(index));
    }
    app.selected_scope.clear();
    app.mark_clean();
    app.rebase_history();
    app
}

fn graph_document(document: MappingDocument) -> GraphDocument {
    match document {
        MappingDocument::Target(_) => GraphDocument::Target(TargetId::Named("Copy".into())),
        _ => GraphDocument::Target(TargetId::Primary),
    }
}

const DOCUMENTS: [MappingDocument; 2] = [MappingDocument::Main, MappingDocument::Target(0)];

#[test]
fn child_scope_additions_name_the_parent_and_field_and_match_snapshots() {
    for document in DOCUMENTS {
        let build = || {
            tree_app(
                document,
                Scope {
                    children: vec![scope("Lines")],
                    ..Scope::default()
                },
            )
        };
        let mut app = assert_typed_edit_matches_snapshots(
            build,
            |app| app.add_selected_child_scope("Header"),
            "Add child scope",
        );
        let command = newest_command(&app);
        assert_eq!(command.document(), &graph_document(document));
        assert_eq!(
            command.action(),
            &GraphAction::AddScope {
                parent: ScopeLocation::root(),
                field: "Header".into(),
            }
        );
        let replaced = command
            .patch()
            .children()
            .map(|(location, _)| location.clone())
            .collect::<Vec<_>>();
        assert_eq!(replaced, [ScopeLocation::root()]);

        // Child scopes changed behind the history refuse the step.
        assert_stale_undo_refused(&mut app, |project| {
            let root = match document {
                MappingDocument::Target(index) => &mut project.extra_targets[index].root,
                _ => &mut project.root,
            };
            root.children[0].target_field = "Renamed".into();
        });
    }
}

#[test]
fn subtree_expansion_is_one_typed_command_matching_snapshots() {
    for document in DOCUMENTS {
        let build = || {
            tree_app(
                document,
                Scope {
                    children: vec![scope("Lines")],
                    ..Scope::default()
                },
            )
        };
        let app = assert_typed_edit_matches_snapshots(
            build,
            |app| app.expand_selected_target_subtree(),
            "Expand scope subtree",
        );
        let command = newest_command(&app);
        assert_eq!(command.document(), &graph_document(document));
        assert_eq!(
            command.action(),
            &GraphAction::ExpandScope {
                scope: ScopeLocation::root()
            }
        );
        assert_eq!(command.patch().nodes().count(), 0);
    }
}

#[test]
fn scope_removal_retires_owned_items_and_matches_snapshots() {
    for document in DOCUMENTS {
        let build = || {
            let mut app = tree_app(
                document,
                Scope {
                    children: vec![scope("Header"), generated_lines()],
                    ..Scope::default()
                },
            );
            app.selected_scope = vec![1];
            app
        };
        let app = assert_typed_edit_matches_snapshots(
            build,
            |app| {
                app.selected_scope = vec![1];
                app.remove_selected_target_scope();
            },
            "Remove scope",
        );
        let command = newest_command(&app);
        assert_eq!(
            command.action(),
            &GraphAction::RemoveScope {
                scope: ScopeLocation::of_children(&[1])
            }
        );
        assert_eq!(
            command.patch().removed_nodes().collect::<Vec<_>>(),
            [ITEM],
            "only the generated item retires with its scope"
        );
    }
}

fn xml_app(document: MappingDocument) -> FerruleApp {
    let mut app = FerruleApp {
        project: crate::target_xml_type::tests::project(),
        ..FerruleApp::default()
    };
    app.main_canvas = CanvasDocumentState::main(&app.project);
    if let MappingDocument::Target(index) = document {
        app.open_target_tab(index);
        assert!(app.ensure_target_canvas(index));
    }
    app.selected_scope = match document {
        MappingDocument::Target(_) => Vec::new(),
        _ => vec![0],
    };
    app.mark_clean();
    app.rebase_history();
    app
}

#[test]
fn xml_type_selection_and_inference_are_typed_commands_matching_snapshots() {
    use crate::target_xml_type::{
        Action,
        tests::{DERIVED, marker},
    };
    for document in DOCUMENTS {
        let scope = match document {
            MappingDocument::Target(_) => ScopeLocation::root(),
            _ => ScopeLocation::of_children(&[0]),
        };
        let typed_scope = |app: &FerruleApp| match document {
            MappingDocument::Target(index) => app.project.extra_targets[index].root.clone(),
            _ => app.project.root.children[0].clone(),
        };
        let app = assert_typed_edit_matches_snapshots(
            || xml_app(document),
            |app| app.apply_selected_target_xml_type(&Action::Declared(DERIVED.into())),
            "Set XML type",
        );
        let declared = marker(&typed_scope(&app));
        assert_eq!(
            newest_command(&app).action(),
            &GraphAction::SetXmlType {
                scope: scope.clone(),
                node: declared,
            }
        );
        assert_eq!(
            newest_command(&app)
                .patch()
                .added_nodes()
                .collect::<Vec<_>>(),
            [declared]
        );

        let app = assert_typed_edit_matches_snapshots(
            || xml_app(document),
            |app| app.apply_selected_target_xml_type(&Action::Infer),
            "Use XML type inference",
        );
        assert_eq!(
            newest_command(&app).action(),
            &GraphAction::InferXmlType { scope }
        );
        assert!(
            typed_scope(&app)
                .bindings
                .iter()
                .all(|binding| binding.target_field != ir::XML_TYPE_FIELD)
        );
    }
}
