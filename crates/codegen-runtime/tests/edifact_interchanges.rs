use codegen_runtime::{EdiDocumentBoundary, PrimaryEdiKind, validate_edifact_envelope};
use ir::{ScalarType, SchemaNode};

#[path = "../../format-edi/tests/fixtures/interchanges.rs"]
mod fixtures;

#[test]
fn rust_document_boundary_uses_each_interchanges_native_syntax() {
    let scalar = |name| SchemaNode::scalar(name, ScalarType::String);
    let schema = SchemaNode::group(
        "Document",
        vec![
            SchemaNode::group(
                "Interchanges",
                vec![
                    SchemaNode::group("UNB", vec![]),
                    SchemaNode::group("UNH", vec![]),
                    SchemaNode::group("BGM", vec![scalar("Code"), scalar("Data")]),
                    SchemaNode::group("UNT", vec![]),
                    SchemaNode::group("UNZ", vec![]),
                ],
            )
            .repeating(),
        ],
    );
    let boundary = EdiDocumentBoundary::input(
        &serde_json::to_string(&schema).unwrap(),
        "{}",
        PrimaryEdiKind::Edifact,
    )
    .unwrap();
    for case in fixtures::cases() {
        validate_edifact_envelope(case.wire.as_bytes()).unwrap();
        let parsed = boundary.parse(case.wire.as_bytes()).unwrap();
        let items = parsed.field("Interchanges").unwrap().as_repeated().unwrap();
        assert_eq!(items.len(), 2, "{}", case.name);
        for (at, item) in items.iter().enumerate() {
            let segment = item.field("BGM").unwrap();
            assert_eq!(
                segment.field("Code").unwrap().as_scalar(),
                Some(&ir::Value::String(case.codes[at].into()))
            );
            assert_eq!(
                segment.field("Data").unwrap().as_scalar(),
                Some(&ir::Value::String(case.values[at].into()))
            );
        }
        let output = boundary.serialize(&parsed, "2026-10-26T12:00:00Z").unwrap();
        assert_eq!(boundary.parse(&output).unwrap(), parsed);
    }
}
