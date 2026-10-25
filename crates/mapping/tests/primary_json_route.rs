use ir::{ScalarType, SchemaNode, Value};
use mapping::*;

fn document(selection: PrimaryFileSelection) -> DocumentProject {
    let schema = SchemaNode::scalar("value", ScalarType::String);
    DocumentProject::new(
        Project {
            source: schema.clone(),
            target: schema,
            source_path: Some("input.json".into()),
            target_path: Some("output.json".into()),
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![],
            extra_targets: vec![],
            failure_rules: vec![],
            user_functions: Default::default(),
            graph: Graph {
                nodes: [(
                    1,
                    Node::Const {
                        value: Value::String("input".into()),
                    },
                )]
                .into(),
            },
            root: Scope::default(),
        },
        PrimarySourcePlan::new(1).with_selection(selection),
    )
}

#[test]
fn strict_json_shapes_and_identities() {
    for (selection, shape) in [
        (PrimaryFileSelection::Direct, PrimaryDocumentShape::Document),
        (
            PrimaryFileSelection::FileNameWildcard {
                empty: EmptyFileSelection::MapNoDocuments,
            },
            PrimaryDocumentShape::DocumentSet,
        ),
    ] {
        let document = document(selection);
        let expected = Ok(PrimaryDocumentRoute {
            format: PrimaryDocumentFormat::Json,
            shape,
        });
        assert_eq!(document.primary_route(), expected);
        let mut project = document.project().clone();
        project.source_options.json_document = true;
        project.target_options.json_document = true;
        project.source_path = None;
        project.target_path = None;
        assert_eq!(
            DocumentProject::new(project, document.primary_source().clone()).primary_route(),
            expected
        );
    }
}

#[test]
fn json5_and_json_lines_are_explicitly_not_admitted() {
    let base = document(PrimaryFileSelection::Direct);
    for (extension, format) in [
        ("json5", "JSON5"),
        ("jsonl", "JSON Lines"),
        ("ndjson", "JSON Lines"),
    ] {
        let mut project = base.project().clone();
        project.source_path = Some(format!("input.{extension}"));
        assert_eq!(
            DocumentProject::new(project, base.primary_source().clone()).primary_route(),
            Err(PrimaryRouteError::NotAdmitted {
                side: PrimaryRouteSide::Source,
                format
            })
        );
    }
    for json5 in [false, true] {
        let mut project = base.project().clone();
        project.source_options.json_document = true;
        project.source_options.json5 = json5;
        project.source_options.json_lines = !json5;
        assert_eq!(
            DocumentProject::new(project, base.primary_source().clone()).primary_route(),
            Err(PrimaryRouteError::NotAdmitted {
                side: PrimaryRouteSide::Source,
                format: if json5 { "JSON5" } else { "JSON Lines" }
            })
        );
    }
}
