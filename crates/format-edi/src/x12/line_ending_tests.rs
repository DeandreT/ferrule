use super::*;
use ir::ScalarType;

fn toy() -> (SchemaNode, Instance) {
    let note = SchemaNode::group(
        "NTE",
        (1..=3)
            .map(|i| SchemaNode::scalar(format!("E{i}"), ScalarType::String))
            .collect(),
    )
    .repeating();
    let row = |a: &str, c: &str| {
        Instance::Group(
            vec![
                ("E1".into(), Instance::Scalar(Value::String(a.into()))),
                ("E3".into(), Instance::Scalar(Value::String(c.into()))),
            ]
            .into(),
        )
    };
    (
        SchemaNode::group("Document", vec![note]),
        Instance::Group(
            vec![(
                "NTE".into(),
                Instance::Repeated(vec![row("alpha", "omega"), row("beta", "")]),
            )]
            .into(),
        ),
    )
}

#[test]
fn exact_suffixes_holes_repetition_terminator_conflicts_and_immutable_inputs() {
    let (schema, value) = toy();
    let before = (schema.clone(), value.clone());
    assert_eq!(
        to_string(&schema, &value).unwrap(),
        "NTE*alpha**omega~\nNTE*beta~\n"
    );
    let selected = OutputOptions::default().with_segment_line_ending(X12SegmentLineEnding::CrLf);
    assert_eq!(
        to_string_with_output_options(&schema, &value, selected).unwrap(),
        "NTE*alpha**omega~\r\nNTE*beta~\r\n"
    );
    assert_eq!(
        to_document_string_with_output_options(&schema, &value, selected).unwrap(),
        "NTE*alpha**omega~\r\nNTE*beta~\r\n"
    );
    for segment in ['\n', '\r'] {
        let syntax = Separators {
            segment,
            ..Default::default()
        };
        let expected = if segment == '\n' {
            "NTE*alpha**omega\nNTE*beta\n"
        } else {
            "NTE*alpha**omega\rNTE*beta\r"
        };
        assert_eq!(
            to_string_with_separators(&schema, &value, syntax).unwrap(),
            expected
        );
        assert!(matches!(
            to_string_with_output_options(&schema, &value, selected.with_separators(syntax)),
            Err(EdiFormatError::InvalidX12Separators(_))
        ));
    }
    assert_eq!((schema, value), before);
}

#[test]
fn exact_selected_wire_limit_checks_before_appending_next_segment() {
    let (schema, value) = toy();
    let expected = "NTE*alpha**omega~\r\nNTE*beta~\r\n";
    let options = OutputOptions::default().with_segment_line_ending(X12SegmentLineEnding::CrLf);
    assert_eq!(
        to_string_with_output_options(&schema, &value, options.with_maximum_bytes(expected.len()))
            .unwrap(),
        expected
    );
    assert!(
        matches!(to_string_with_output_options(&schema,&value,options.with_maximum_bytes(expected.len()-1)), Err(EdiFormatError::X12OutputLimit { bytes, maximum }) if bytes == expected.len() && maximum == expected.len()-1)
    );
    let empty = SchemaNode::group("Document", vec![]);
    assert_eq!(
        to_string_with_output_options(
            &empty,
            &Instance::Group(vec![].into()),
            options.with_maximum_bytes(0)
        )
        .unwrap(),
        ""
    );
}
