use super::*;
use ir::ScalarType;

fn segment(name: &str, count: usize) -> SchemaNode {
    SchemaNode::group(
        name,
        (1..=count)
            .map(|n| SchemaNode::scalar(format!("{n:02}"), ScalarType::String))
            .collect(),
    )
}

fn schema() -> SchemaNode {
    let mut note = segment("NTE", 2);
    let SchemaKind::Group { children, .. } = &mut note.kind else {
        unreachable!()
    };
    children[1].repeating = true;
    let mut interchange = SchemaNode::group(
        "Interchange",
        vec![
            segment("ISA", 16),
            segment("GS", 8),
            segment("ST", 2),
            note,
            segment("SE", 2),
            segment("GE", 2),
            segment("IEA", 2),
        ],
    );
    interchange.repeating = true;
    SchemaNode::group("Envelope", vec![interchange])
}

fn wire(syntax: Separators, version: &str, control: &str, label: &str) -> String {
    let e = syntax.element.to_string();
    let t = syntax.segment;
    let c = syntax.component;
    let repeated = syntax
        .repetition
        .map_or("U".into(), |value| value.to_string());
    let isa = [
        "00",
        "          ",
        "00",
        "          ",
        "ZZ",
        "SENDER         ",
        "ZZ",
        "RECEIVER       ",
        "261025",
        "1200",
        &repeated,
        version,
        control,
        "0",
        "T",
        &c.to_string(),
    ]
    .join(&e);
    let body_repetition = syntax.repetition.unwrap_or('^');
    format!(
        "ISA{e}{isa}{t}GS{e}PO{e}SENDER{e}RECEIVER{e}20261025{e}1200{e}1{e}X{e}{version}0{t}ST{e}850{e}0001{t}NTE{e}{label}{c}DETAIL{e}LEFT{body_repetition}RIGHT{t}SE{e}3{e}0001{t}GE{e}1{e}1{t}IEA{e}1{e}{control}{t}"
    )
}

fn interchanges(value: &Instance) -> &[Instance] {
    match value.field("Interchange").unwrap() {
        Instance::Repeated(values) => values,
        _ => panic!("interchange array"),
    }
}

#[test]
fn different_interchange_separators_retain_every_record_and_scalar_component_join() {
    let first = Separators::default();
    let second = Separators {
        element: '|',
        component: '>',
        segment: '!',
        repetition: Some('?'),
        release: None,
    };
    let text = wire(first, "00501", "000000001", "FIRST")
        + "\r\n"
        + &wire(second, "00501", "000000002", "SECOND");
    assert_eq!(tokenize(&text).unwrap().len(), 14);
    for lenient in [false, true] {
        let result = from_str(&text, &schema(), lenient).unwrap();
        let items = interchanges(&result);
        assert_eq!(items.len(), 2);
        for (item, expected) in items.iter().zip(["FIRST:DETAIL", "SECOND>DETAIL"]) {
            let note = item.field("NTE").unwrap();
            assert_eq!(
                note.field("01"),
                Some(&Instance::Scalar(Value::String(expected.into())))
            );
            assert_eq!(
                note.field("02"),
                Some(&Instance::Repeated(vec![
                    Instance::Scalar(Value::String("LEFT".into())),
                    Instance::Scalar(Value::String("RIGHT".into())),
                ]))
            );
        }
    }
    assert!(matches!(
        from_str_with_separators(&text, &schema(), true, Some(first)),
        Err(EdiFormatError::X12SeparatorMismatch { .. })
    ));
}

#[test]
fn an_interchange_can_reuse_the_previous_interchanges_delimiters() {
    let second = Separators {
        element: '|',
        component: '~',
        segment: '*',
        repetition: Some(':'),
        release: None,
    };
    let text = wire(Separators::default(), "00501", "000000001", "FIRST")
        + &wire(second, "00501", "000000002", "SECOND");
    for lenient in [false, true] {
        let result = from_str(&text, &schema(), lenient).unwrap();
        let items = interchanges(&result);
        assert_eq!(items.len(), 2);
        let note = items[1].field("NTE").unwrap();
        assert_eq!(
            note.field("01"),
            Some(&Instance::Scalar(Value::String("SECOND~DETAIL".into())))
        );
        assert_eq!(
            note.field("02"),
            Some(&Instance::Repeated(vec![
                Instance::Scalar(Value::String("LEFT".into())),
                Instance::Scalar(Value::String("RIGHT".into())),
            ]))
        );
    }
}

#[test]
fn each_interchange_version_controls_its_own_repetition_semantics() {
    let old = Separators {
        repetition: None,
        ..Separators::default()
    };
    let new = Separators::default();
    let text = wire(old, "00401", "000000001", "OLD") + &wire(new, "00501", "000000002", "NEW");
    let result = from_str(&text, &schema(), false).unwrap();
    let items = interchanges(&result);
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[0].field("NTE").unwrap().field("02"),
        Some(&Instance::Repeated(vec![Instance::Scalar(Value::String(
            "LEFT^RIGHT".into()
        )),]))
    );
    assert_eq!(
        items[1].field("NTE").unwrap().field("02"),
        Some(&Instance::Repeated(vec![
            Instance::Scalar(Value::String("LEFT".into())),
            Instance::Scalar(Value::String("RIGHT".into())),
        ]))
    );
}

#[test]
fn malformed_later_interchange_never_becomes_a_partial_lenient_result() {
    let first = wire(Separators::default(), "00501", "000000001", "FIRST");
    for suffix in [
        "ISA|00|truncated!".to_string(),
        wire(Separators::default(), "ABCDE", "000000002", "SECOND"),
    ] {
        let text = first.clone() + &suffix;
        assert!(tokenize(&text).is_err());
        for lenient in [false, true] {
            assert!(from_str(&text, &schema(), lenient).is_err());
        }
    }
}
