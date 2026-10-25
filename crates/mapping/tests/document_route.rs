use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    DocumentProject, EmptyFileSelection, FormatOptions, Graph, Node, PrimaryDocumentFormat,
    PrimaryDocumentRoute, PrimaryDocumentShape, PrimaryFileSelection, PrimaryRouteError,
    PrimaryRouteSide, PrimarySourcePlan, Project, Scope, TabularBoundaryKind,
};

const WILDCARD: PrimaryFileSelection = PrimaryFileSelection::FileNameWildcard {
    empty: EmptyFileSelection::Refuse,
};

fn document(
    source: (FormatOptions, Option<&str>),
    target: (FormatOptions, Option<&str>),
    selection: PrimaryFileSelection,
) -> DocumentProject {
    let schema = || {
        SchemaNode::group(
            "Root",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        )
    };
    DocumentProject::new(
        Project {
            source: schema(),
            target: schema(),
            source_path: source.1.map(Into::into),
            target_path: target.1.map(Into::into),
            source_options: source.0,
            target_options: target.0,
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

fn xml() -> FormatOptions {
    FormatOptions {
        xml_document: true,
        ..Default::default()
    }
}

fn csv() -> FormatOptions {
    FormatOptions {
        tabular_kind: Some(TabularBoundaryKind::Csv),
        ..Default::default()
    }
}

fn route(format: PrimaryDocumentFormat, shape: PrimaryDocumentShape) -> PrimaryDocumentRoute {
    PrimaryDocumentRoute { format, shape }
}

#[test]
fn explicit_identities_or_saved_paths_decide_the_route() {
    use PrimaryDocumentFormat::{Csv, Xml};
    use PrimaryDocumentShape::{Document, DocumentSet, Rows};
    let none = FormatOptions::default;
    for (source, target, selection, expected) in [
        ((csv(), None), (csv(), None), WILDCARD, route(Csv, Rows)),
        (
            (none(), Some("in.CSV")),
            (none(), Some("out.csv")),
            PrimaryFileSelection::Direct,
            route(Csv, Rows),
        ),
        (
            (xml(), None),
            (xml(), None),
            PrimaryFileSelection::Direct,
            route(Xml, Document),
        ),
        (
            (none(), Some("orders.xml")),
            (none(), Some("summary.xml")),
            WILDCARD,
            route(Xml, DocumentSet),
        ),
        (
            (
                FormatOptions {
                    local_xml_file_set: true,
                    ..xml()
                },
                None,
            ),
            (xml(), None),
            WILDCARD,
            route(Xml, DocumentSet),
        ),
    ] {
        assert_eq!(
            document(source, target, selection).primary_route(),
            Ok(expected)
        );
    }
}

#[test]
fn unrouted_formats_and_shapes_are_typed_refusals() {
    let none = FormatOptions::default;
    let refused = |source, target, selection| document(source, target, selection).primary_route();
    assert_eq!(
        refused((none(), None), (csv(), None), WILDCARD),
        Err(PrimaryRouteError::MissingIdentity {
            side: PrimaryRouteSide::Source
        })
    );
    assert_eq!(
        refused((none(), Some("in.xlsx")), (csv(), None), WILDCARD),
        Err(PrimaryRouteError::NotAdmitted {
            side: PrimaryRouteSide::Source,
            format: "XLSX"
        })
    );
    assert_eq!(
        refused((csv(), None), (none(), Some("out.db")), WILDCARD),
        Err(PrimaryRouteError::NotAdmitted {
            side: PrimaryRouteSide::Target,
            format: "SQLite"
        })
    );
    assert_eq!(
        refused((xml(), None), (csv(), None), WILDCARD),
        Err(PrimaryRouteError::TargetFormat {
            source: PrimaryDocumentFormat::Xml
        })
    );
    assert_eq!(
        refused((xml(), None), (none(), None), WILDCARD),
        Err(PrimaryRouteError::TargetFormat {
            source: PrimaryDocumentFormat::Xml
        })
    );
    assert_eq!(
        refused(
            (
                FormatOptions {
                    local_xml_file_set: true,
                    ..xml()
                },
                None
            ),
            (xml(), None),
            PrimaryFileSelection::Direct
        ),
        Err(PrimaryRouteError::DirectDocumentSet)
    );
    assert_eq!(
        refused(
            (
                FormatOptions {
                    xml_root_view_read_policy: true,
                    xml_allow_inactive_root_type_members: true,
                    ..xml()
                },
                None
            ),
            (xml(), None),
            WILDCARD
        ),
        Err(PrimaryRouteError::RootViewDocumentSet)
    );
}
