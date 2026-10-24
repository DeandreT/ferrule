use super::*;

#[test]
fn csv_output_uses_a_bounded_writer_at_the_exact_limit() {
    let schema = ir::SchemaNode::group(
        "Row",
        vec![ir::SchemaNode::scalar("Text", ir::ScalarType::String)],
    );
    let output = ir::Instance::Repeated(vec![ir::Instance::Group(
        vec![(
            "Text".into(),
            ir::Instance::Scalar(ir::Value::String("quoted,value".into())),
        )]
        .into(),
    )]);
    let route = mapping::PrimaryDocumentRoute {
        format: PrimaryDocumentFormat::Csv,
        shape: PrimaryDocumentShape::Rows,
    };
    let options = mapping::FormatOptions {
        csv_utf8_bom: true,
        ..Default::default()
    };
    let expected = "\u{feff}Text\n\"quoted,value\"\n".as_bytes();
    let (bytes, records) = render_primary_output::<std::io::Error>(
        route,
        &schema,
        &options,
        &output,
        "",
        expected.len(),
    )
    .unwrap();
    assert_eq!(bytes, expected);
    assert_eq!(records, 1);
    for maximum in [0, expected.len() - 1] {
        assert!(
            matches!(render_primary_output::<std::io::Error>(route, &schema, &options, &output, "", maximum),
            Err(PrimaryDocumentError::OutputTooLarge { maximum: limit }) if limit == maximum)
        );
    }
}
