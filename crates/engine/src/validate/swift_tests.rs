use super::tests::valid_project;
use super::validate;
use ir::{ScalarType, SchemaNode};
use mapping::{
    Binding, EdiImpliedDecimal, FormatOptions, NamedTarget, Project, Scope, ScopeConstruction,
    SwiftCharset, SwiftFieldLayout, SwiftMessageLayout, SwiftMtDirection, SwiftMtLayout,
    SwiftMtMessageHeaders, SwiftMtOutputOptions, SwiftValueExpr,
};

const BASIC: &str = "F01BANKBEBBAXXX0000000001";
const INPUT_TAIL: &str = "BANKDEFFXXXXN";

fn layout() -> SwiftMtLayout {
    SwiftMtLayout::new(vec![SwiftMessageLayout::new(
        "MT950",
        vec![SwiftFieldLayout::new(
            "20",
            vec!["MT950".into(), "20".into()],
            false,
            SwiftValueExpr::Capture {
                path: Vec::new(),
                min: 1,
                max: 16,
                charset: SwiftCharset::Text,
            },
        )],
    )])
    .unwrap()
}

fn schema() -> SchemaNode {
    let mut message = SchemaNode::group(
        "Message",
        vec![
            SchemaNode::group(
                "Application Header",
                vec![
                    SchemaNode::group("Input", Vec::new()),
                    SchemaNode::group("Output", Vec::new()),
                ],
            ),
            SchemaNode::group("MT950", vec![SchemaNode::scalar("20", ScalarType::String)]),
        ],
    );
    message.repeating = true;
    SchemaNode::group("SWIFT", vec![message])
}

fn headers(basic: &str, tail: &str) -> SwiftMtOutputOptions {
    SwiftMtOutputOptions::new(
        SwiftMtDirection::Input,
        vec![SwiftMtMessageHeaders {
            basic_header: basic.into(),
            application_header: tail.into(),
        }],
    )
    .unwrap()
}

fn options() -> FormatOptions {
    FormatOptions {
        swift_mt: Some(layout()),
        swift_mt_output: Some(headers(BASIC, INPUT_TAIL)),
        ..FormatOptions::default()
    }
}

/// One statement per run, with field 20 taken from the source `name`.
fn root() -> Scope {
    Scope {
        children: vec![Scope {
            target_field: "Message".into(),
            children: vec![Scope {
                target_field: "MT950".into(),
                bindings: vec![Binding {
                    target_field: "20".into(),
                    node: 0,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    }
}

fn swift_project() -> Project {
    let mut project = valid_project();
    project.target = schema();
    project.target_options = options();
    project.root = root();
    project
}

fn issues_at(project: &Project, location: &str) -> Vec<String> {
    validate(project)
        .into_iter()
        .filter(|issue| issue.location == location)
        .map(|issue| issue.message)
        .collect()
}

#[test]
fn configured_swift_targets_validate() {
    assert!(validate(&swift_project()).is_empty());

    let mut named = valid_project();
    named.extra_targets.push(NamedTarget {
        name: "statement".into(),
        path: None,
        schema: schema(),
        options: options(),
        root: root(),
    });
    assert!(validate(&named).is_empty());
}

#[test]
fn swift_targets_without_usable_headers_are_refused() {
    let mut missing = swift_project();
    missing.target_options.swift_mt_output = None;
    assert!(
        issues_at(&missing, "target format options")
            .iter()
            .any(|message| message.contains("no `swift_mt_output` message headers"))
    );

    let mut invalid = swift_project();
    invalid.target_options.swift_mt_output = Some(headers(BASIC, "bankdeffxxxx"));
    assert!(
        issues_at(&invalid, "target format options")
            .iter()
            .any(|message| message.contains("input application header"))
    );

    let mut named = valid_project();
    named.extra_targets.push(NamedTarget {
        name: "statement".into(),
        path: None,
        schema: schema(),
        options: FormatOptions {
            swift_mt_output: Some(headers("F01SHORT", INPUT_TAIL)),
            ..options()
        },
        root: root(),
    });
    assert!(
        issues_at(&named, "extra target `statement` format options")
            .iter()
            .any(|message| message.contains("basic header"))
    );
}

#[test]
fn headers_belong_to_swift_targets_only() {
    let mut source = valid_project();
    source.source_options.swift_mt_output = Some(headers(BASIC, INPUT_TAIL));
    assert!(
        issues_at(&source, "source format options")
            .iter()
            .any(|message| message.contains("valid only for mapping targets"))
    );

    let mut without_layout = valid_project();
    without_layout.target_options.swift_mt_output = Some(headers(BASIC, INPUT_TAIL));
    assert!(
        issues_at(&without_layout, "target format options")
            .iter()
            .any(|message| message.contains("requires an embedded `swift_mt` layout"))
    );
}

#[test]
fn implied_decimals_must_name_a_decimal_capture() {
    let mut project = swift_project();
    project.target_options.edi_implied_decimals = vec![
        EdiImpliedDecimal::new(vec!["Message".into(), "MT950".into(), "20".into()], 2).unwrap(),
    ];
    assert!(
        issues_at(&project, "target format options")
            .iter()
            .any(|message| message.contains("does not name a decimal capture"))
    );
}

#[test]
fn targets_producing_separate_documents_cannot_share_headers() {
    let mut primary = swift_project();
    primary.root.construction = ScopeConstruction::CollectDocuments;
    assert!(primary.root.produces_documents());
    assert!(
        issues_at(&primary, "target format options")
            .iter()
            .any(|message| message.contains("cannot be shared by the separate documents"))
    );

    let mut named = valid_project();
    let mut root = root();
    root.construction = ScopeConstruction::CollectDocuments;
    named.extra_targets.push(NamedTarget {
        name: "statement".into(),
        path: None,
        schema: schema(),
        options: options(),
        root,
    });
    assert!(
        issues_at(&named, "extra target `statement` format options")
            .iter()
            .any(|message| message.contains("cannot be shared by the separate documents"))
    );
}

/// `root()` plus an `Application Header` scope constructing `groups`.
fn with_mapped_header(groups: &[&str]) -> Project {
    let mut project = swift_project();
    let header = Scope {
        target_field: "Application Header".into(),
        children: groups
            .iter()
            .map(|group| Scope {
                target_field: (*group).into(),
                ..Scope::default()
            })
            .collect(),
        ..Scope::default()
    };
    project.root.children[0].children.insert(0, header);
    project
}

#[test]
fn a_mapped_direction_must_not_contradict_the_configuration() {
    for groups in [
        &[][..],
        &["Input"],
        &["Input", "Output"],
        &["Output", "Input"],
    ] {
        let project = with_mapped_header(groups);
        assert!(validate(&project).is_empty(), "{groups:?}");
    }
    let contradicting = with_mapped_header(&["Output"]);
    assert!(
        issues_at(&contradicting, "target format options")
            .iter()
            .any(|message| message.contains(
                "constructs only the Output application header, but `swift_mt_output` writes Input messages"
            ))
    );
}

/// A layout reusing tag `20` for a second field, like MT940's `:86:` at
/// line and statement level, with `bound` fields mapped from the source.
fn reused_tag_project(bound: &[&str]) -> Project {
    let mut project = swift_project();
    let mut layout_fields = layout().messages()[0].fields().to_vec();
    layout_fields.push(SwiftFieldLayout::new(
        "20",
        vec!["MT950".into(), "Again".into()],
        false,
        SwiftValueExpr::Capture {
            path: Vec::new(),
            min: 1,
            max: 16,
            charset: SwiftCharset::Text,
        },
    ));
    project.target_options.swift_mt =
        Some(SwiftMtLayout::new(vec![SwiftMessageLayout::new("MT950", layout_fields)]).unwrap());
    if let ir::SchemaKind::Group { children, .. } = &mut project.target.kind
        && let ir::SchemaKind::Group { children, .. } = &mut children[0].kind
        && let ir::SchemaKind::Group { children, .. } = &mut children[1].kind
    {
        children.push(SchemaNode::scalar("Again", ScalarType::String));
    }
    project.root.children[0].children[0].bindings = bound
        .iter()
        .map(|field| Binding {
            target_field: (*field).into(),
            node: 0,
        })
        .collect();
    project
}

#[test]
fn reused_tags_are_refused_only_when_the_mapping_fills_a_later_field() {
    for bound in [&[][..], &["20"]] {
        let project = reused_tag_project(bound);
        assert!(validate(&project).is_empty(), "{bound:?}");
    }
    for bound in [&["20", "Again"][..], &["Again"]] {
        let issues = issues_at(&reused_tag_project(bound), "target format options");
        assert!(
            issues.iter().any(|message| message.contains(
                "SWIFT field `20` at `MT950/Again` reuses a tag that an earlier layout field reads"
            )),
            "{bound:?}: {issues:?}"
        );
    }
}

#[test]
fn swift_targets_refuse_json_tuple_frames() {
    let mut project = swift_project();
    let frame = SchemaNode::group(
        "point",
        vec![
            SchemaNode::scalar("item1", ScalarType::String),
            SchemaNode::scalar("item2", ScalarType::Int),
        ],
    )
    .with_json_tuple(ir::JsonTupleLayout {
        length: ir::ItemCountRange::new(0, Some(2)),
        tail: ir::JsonTupleTail::Closed,
    })
    .unwrap();
    if let ir::SchemaKind::Group { children, .. } = &mut project.target.kind {
        children.push(frame);
    }
    let issues = issues_at(&project, "target format options");
    assert!(
        issues.iter().any(|message| message.contains("tuple frame")),
        "{issues:?}"
    );
}
