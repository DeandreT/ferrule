use super::*;
use ir::{ScalarType, Value};

#[path = "../../tests/fixtures/interchanges.rs"]
mod fixtures;

fn schema() -> SchemaNode {
    let scalar = |name| SchemaNode::scalar(name, ScalarType::String);
    SchemaNode::group(
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
    )
}

#[test]
fn each_interchange_owns_its_syntax_and_scalar_component_spelling() {
    for case in fixtures::cases() {
        validate_envelope(&case.wire).unwrap();
        let instance = from_str(&case.wire, &schema(), false).unwrap();
        let interchanges = instance
            .field("Interchanges")
            .unwrap()
            .as_repeated()
            .unwrap();
        assert_eq!(interchanges.len(), 2, "{}", case.name);
        for (at, interchange) in interchanges.iter().enumerate() {
            let bgm = interchange.field("BGM").unwrap();
            assert_eq!(
                bgm.field("Code").unwrap().as_scalar(),
                Some(&Value::String(case.codes[at].into())),
                "{}",
                case.name
            );
            assert_eq!(
                bgm.field("Data").unwrap().as_scalar(),
                Some(&Value::String(case.values[at].into())),
                "{}",
                case.name
            );
        }
        // Canonical writing stays schema-driven, not tied to the input UNA.
        let wire = to_string(&schema(), &instance).unwrap();
        assert_eq!(
            from_str(&wire, &schema(), false).unwrap(),
            instance,
            "{}",
            case.name
        );
    }
}

#[test]
fn repetition_version_and_advice_reset_at_each_boundary() {
    let wire = "UNA;|.!^$UNB|UNOC;4|S|R|D|A$UNH|M$BGM|A!^B^C$UNT|3|M$UNZ|1|A$\nUNB+UNOC:3+S+R+D+B'UNH+N'BGM+A*B'UNT+3+N'UNZ+1+B'";
    validate_envelope(wire).unwrap();
    let segments = tokenize(wire).unwrap();
    assert_eq!(
        segments[2].elements[0],
        vec![vec!["A^B".to_string()], vec!["C".to_string()]]
    );
    assert_eq!(segments[7].elements[0], vec![vec!["A*B".to_string()]]);
    let wire = "UNA;|.! $UNH|M$BGM|A!!$UNT|3|M$\nUNH+N'BGM+A??'UNT+3+N'";
    validate_envelope(wire).unwrap();
    let segments = tokenize(wire).unwrap();
    assert_eq!(segments[1].elements[0][0][0], "A!");
    assert_eq!(segments[4].elements[0][0][0], "A?");
}

#[test]
fn malformed_advice_references_and_mid_message_advice_stay_errors() {
    let prefix = "UNB+UNOC:3+S+R+D+A'UNH+M'BGM+X'UNT+3+M'UNZ+1+A'";
    for bad in [
        "UNA;|",
        "UNA;;.! $UNH|M$UNT|2|M$",
        "UNA;|.!^$BGM|X$",
        "UNA;|.! $UNB|UNOC;4|S|R|D|B$",
    ] {
        assert!(
            matches!(
                tokenize(&format!("{prefix}{bad}")),
                Err(EdiFormatError::NotEdifact(_))
            ),
            "{bad}"
        );
    }
    let valid = &fixtures::cases()[2].wire;
    assert!(matches!(
        validate_envelope(&valid.replace("UNZ;1;C", "UNZ;1;wrong")),
        Err(EdiFormatError::EdifactEnvelope { segment: 9, .. })
    ));
    let mid_message = "UNB+UNOC:3+S+R+D+A'UNH+M'UNA;|.! $BGM|X$UNT|3|M$UNZ|1|A$";
    assert!(validate_envelope(mid_message).is_err());
    // Schema strictness and opt-in envelope validation remain independent.
    let wrong_reference = prefix.replace("UNZ+1+A", "UNZ+1+wrong");
    from_str(&wrong_reference, &schema(), false).unwrap();
    assert!(matches!(
        validate_envelope(&wrong_reference),
        Err(EdiFormatError::EdifactEnvelope { .. })
    ));
}

#[test]
fn concatenation_does_not_widen_a_singular_schema() {
    let mut singular = schema();
    let ir::SchemaKind::Group { children, .. } = &mut singular.kind else {
        unreachable!()
    };
    children[0].repeating = false;
    for case in fixtures::cases() {
        assert!(
            matches!(
                from_str(&case.wire, &singular, false),
                Err(EdiFormatError::TrailingSegment { index: 5, .. })
            ),
            "{}",
            case.name
        );
    }
}

#[test]
fn trailing_unmentioned_segments_keep_the_existing_lenient_policy() {
    let wire = "UNB+UNOC:3+S+R+D+A'UNH+M'BGM+X'UNT+3+M'UNZ+1+A'ZZZ+unknown'";
    from_str(wire, &schema(), true).unwrap();
    assert!(matches!(
        from_str(wire, &schema(), false),
        Err(EdiFormatError::TrailingSegment { index: 5, .. })
    ));
    assert!(matches!(
        validate_envelope(wire),
        Err(EdiFormatError::EdifactEnvelope { segment: 5, .. })
    ));
}

#[test]
fn large_interchanges_are_not_aggregated_under_the_first_terminator() {
    let body = format!("BGM|220|{}!!VALUE!$LITERAL$", "X".repeat(100)).repeat(180);
    let custom = format!("UNA;|.! $UNB|UNOC;3|S|R|D|B$UNH|M${body}UNT|182|M$UNZ|1|B$");
    let default = "UNB+UNOC:3+S+R+D+A'UNH+N'BGM+220+VALUE'UNT+3+N'UNZ+1+A'";
    let mut schema = schema();
    let ir::SchemaKind::Group { children, .. } = &mut schema.kind else {
        unreachable!()
    };
    let ir::SchemaKind::Group { children, .. } = &mut children[0].kind else {
        unreachable!()
    };
    children[2].repeating = true;
    for wire in [
        format!("{default}\r\n{custom}"),
        format!("{custom}\r\n{default}"),
    ] {
        validate_envelope(&wire).unwrap();
        let parsed = from_str(&wire, &schema, false).unwrap();
        assert_eq!(
            parsed
                .field("Interchanges")
                .unwrap()
                .as_repeated()
                .unwrap()
                .len(),
            2
        );
    }
}
