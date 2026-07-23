use ir::{ScalarType, SchemaKind};
use mapping::EdiLexicalKind;

use super::*;

#[test]
fn message_config_expands_positions_and_wraps_envelope() {
    let directory = temp_directory("message");
    write(
        &directory.join("Defs.Segment"),
        r#"<Config><Elements>
            <Data name="F1" type="string"/>
            <Data name="F2" type="decimal" implicitDecimals="2"/>
            <Data name="FDecimal" type="decimal" minLength="1" maxLength="10"/>
            <Data name="FDate" type="date" minLength="8" maxLength="8"/>
            <Data name="FTime" type="time" minLength="4" maxLength="8"/>
            <Data name="FCode" type="string" minLength="2" maxLength="3"/>
            <Data name="FLong" type="string" minLength="0" maxLength="2147483647"/>
            <Composite name="C1" id="C1-GUIDE"><Data ref="F1"/><Data ref="F2"/></Composite>
            <Segment name="ISA"><Data ref="F1"/></Segment>
            <Segment name="GS"><Data ref="F1"/></Segment>
            <Segment name="ST"><Data ref="F1"/></Segment>
            <Segment name="N1" id="N1-GUIDE">
              <Composite ref="C1-GUIDE"/>
              <Data ref="F1" mergedEntries="2"><Values><Value Code="AA"/></Values></Data>
              <Data ref="FDate"/><Data ref="FTime" nodeName="Clock"/><Data ref="FDecimal"/>
              <Data ref="FCode" nodeName="Status"><Values><Value Code="AA"/><Value Code="BB"/></Values></Data>
              <Data ref="FLong"/>
              <Composite name="C-MISSING" minOccurs="0" maxOccurs="0"/>
            </Segment>
            <Segment name="SE"><Data ref="F2"/></Segment>
            <Segment name="GE"><Data ref="F1"/></Segment>
            <Segment name="IEA"><Data ref="F1"/></Segment>
        </Elements></Config>"#,
    );
    write(
        &directory.join("Envelope.Config"),
        r#"<Config><Format standard="X12"/><Include href="Defs.Segment"/>
          <Group name="Envelope"><Group name="Interchange" maxOccurs="unbounded">
            <Segment ref="ISA"/><Group name="Group" maxOccurs="unbounded">
              <Segment ref="GS"/><Select field="ST/F1"/><Segment ref="GE" minOccurs="0"/>
            </Group><Segment ref="IEA" minOccurs="0"/>
          </Group></Group></Config>"#,
    );
    let message_path = directory.join("850.Config");
    write(
        &message_path,
        r#"<Config><Format standard="X12"/><Include href="Defs.Segment"/>
          <Message><MessageType>850</MessageType><Group name="Message_850" maxOccurs="unbounded">
            <Segment ref="ST"/><Segment ref="N1-GUIDE" maxOccurs="3"/><Segment ref="SE"/>
          </Group></Message></Config>"#,
    );

    let compiled = import_config(&message_path, &[]).unwrap();
    assert!(compiled.implied_decimals.iter().any(|format| {
        format.places() == 2 && format.path().last().is_some_and(|segment| segment == "F2")
    }));
    assert!(compiled.lexical_formats.iter().any(|format| {
        format.kind() == EdiLexicalKind::CompactDate8
            && format
                .path()
                .last()
                .is_some_and(|segment| segment == "FDate")
    }));
    assert!(compiled.value_constraints.iter().any(|constraint| {
        constraint
            .path()
            .last()
            .is_some_and(|segment| segment == "Status")
            && constraint.min_chars() == 2
            && constraint.max_chars() == 3
            && constraint.allowed_values() == ["AA", "BB"]
    }));
    assert!(compiled.value_constraints.iter().any(|constraint| {
        constraint
            .path()
            .last()
            .is_some_and(|segment| segment == "FLong")
            && constraint.max_chars() == 2_147_483_647
    }));
    assert!(compiled.lexical_formats.iter().any(|format| {
        format.kind() == EdiLexicalKind::Decimal { max_chars: 10 }
            && format
                .path()
                .last()
                .is_some_and(|segment| segment == "FDecimal")
    }));
    assert!(compiled.lexical_formats.iter().any(|format| {
        format.kind()
            == EdiLexicalKind::CompactTime {
                min_digits: 4,
                max_digits: 8,
            }
            && format
                .path()
                .last()
                .is_some_and(|segment| segment == "Clock")
    }));
    let schema = compiled.schema;
    assert_eq!(schema.name, "Envelope");
    let message = at(&schema, &["Interchange", "Group", "Message"]);
    assert!(message.repeating);
    let amount = at(message, &["N1", "C1", "F2"]);
    assert!(matches!(
        amount.kind,
        SchemaKind::Scalar {
            ty: ScalarType::Float
        }
    ));
    assert_eq!(at(message, &["N1", "F1"]).fixed.as_deref(), Some("AA"));
    assert!(at(message, &["N1", "F1_2"]).name == "F1_2");
    assert!(matches!(
        at(message, &["N1", "C-MISSING"]).kind,
        SchemaKind::Group { ref children, .. } if children.is_empty()
    ));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn envelope_selection_finds_message_type_and_sets_trigger_qualifier() {
    let directory = temp_directory("selection");
    write(
        &directory.join("Defs.Segment"),
        r#"<Config><Elements>
          <Data name="F143" type="string"/><Data name="F1705" type="string"/>
          <Data name="F2" type="string"/>
          <Segment name="ISA"><Data ref="F2"/></Segment>
          <Segment name="GS"><Data ref="F2"/></Segment>
          <Segment name="ST"><Data ref="F143"/></Segment>
          <Segment name="SE"><Data ref="F2"/></Segment>
        </Elements></Config>"#,
    );
    let envelope = directory.join("Envelope.Config");
    write(
        &envelope,
        r#"<Config><Format standard="X12"/><Include href="Defs.Segment"/>
          <Group name="Envelope"><Group name="Interchange"><Segment ref="ISA"/>
            <Group name="Group"><Segment ref="GS"/><Select field="ST/F143" maxOccurs="unbounded"/></Group>
          </Group></Group></Config>"#,
    );
    write(
        &directory.join("dental.Config"),
        r#"<Config><Format standard="X12"/><Include href="Defs.Segment"/>
          <Message><MessageType>837-Q2</MessageType><Group name="Message_837-Q2">
            <Segment name="ST"><Condition path="F1705" value="005010X224A2"/>
              <Data ref="F143"><Values><Value Code="837"/></Values></Data>
              <Data ref="F1705"/>
            </Segment><Segment ref="SE"/>
          </Group></Message></Config>"#,
    );

    let schema = import_config(&envelope, &["837-Q2".into()]).unwrap().schema;
    let message = at(&schema, &["Interchange", "Group", "Message_837-Q2"]);
    assert_eq!(at(message, &["ST", "F143"]).fixed.as_deref(), Some("837"));
    assert_eq!(
        at(message, &["ST", "F1705"]).fixed.as_deref(),
        Some("005010X224A2")
    );
    assert!(message.repeating);
    std::fs::remove_dir_all(directory).unwrap();
}

fn at<'a>(node: &'a SchemaNode, path: &[&str]) -> &'a SchemaNode {
    path.iter().fold(node, |current, segment| {
        let SchemaKind::Group { children, .. } = &current.kind else {
            panic!("{} is scalar", current.name);
        };
        children
            .iter()
            .find(|child| child.name == *segment)
            .unwrap()
    })
}

fn temp_directory(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "ferrule_edi_config_{label}_{}_{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
}
