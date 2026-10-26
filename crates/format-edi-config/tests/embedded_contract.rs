use std::path::PathBuf;

use format_edi::{EdiFormatError, document};
use ir::{Instance, SchemaNode, Value};
use mapping::{EdiBoundaryKind, FormatOptions};

struct Directory(PathBuf);

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn compiled_metadata_survives_serialization_and_source_removal()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = Directory(std::env::temp_dir().join(format!(
        "ferrule_edi_embedded_contract_{}",
        std::process::id()
    )));
    std::fs::create_dir_all(&directory.0)?;
    std::fs::write(
        directory.0.join("Fields.Segment"),
        r#"<Config><Elements>
          <Data name="Text" type="string"/>
          <Data name="Amount" type="decimal" implicitDecimals="2" minLength="1" maxLength="6"/>
          <Data name="Date" type="date" minLength="8" maxLength="8"/>
          <Data name="FStatus" type="string" minLength="2" maxLength="2"/>
          <Segment name="MSH"><Data ref="Text" nodeName="Separator"/><Data ref="Text" nodeName="Encoding"/><Data ref="Text" nodeName="Sender"/></Segment>
          <Segment name="PID"><Data ref="Amount"/><Data ref="Date"/><Data ref="FStatus" nodeName="Status"><Values><Value Code="OK"/><Value Code="AA"/></Values></Data></Segment>
        </Elements></Config>"#,
    )?;
    let path = directory.0.join("Message.Config");
    std::fs::write(
        &path,
        r#"<Config><Format standard="HL7"/><Include href="Fields.Segment"/>
          <Message><MessageType>ADT</MessageType><Group name="Message"><Segment ref="MSH"/><Segment ref="PID"/></Group></Message>
        </Config>"#,
    )?;
    let compiled = format_edi_config::import_config(&path, &[])?;
    assert!(!compiled.implied_decimals.is_empty());
    assert!(!compiled.lexical_formats.is_empty());
    assert!(!compiled.value_constraints.is_empty());
    let options = FormatOptions {
        edi_kind: Some(EdiBoundaryKind::Hl7),
        edi_implied_decimals: compiled.implied_decimals,
        edi_lexical_formats: compiled.lexical_formats,
        edi_value_constraints: compiled.value_constraints,
        ..Default::default()
    };
    let embedded = serde_json::to_vec(&(compiled.schema.clone(), options.clone()))?;
    let (schema, restored): (SchemaNode, FormatOptions) = serde_json::from_slice(&embedded)?;
    assert_eq!(schema, compiled.schema);
    assert_eq!(restored, options);
    std::fs::remove_dir_all(&directory.0)?;
    assert!(!path.exists());
    drop(directory);

    let input = b"MSH|^~\\&|SOURCE\rPID|1234|20261026|OK\r";
    let instance = document::parse(&schema, &restored, EdiBoundaryKind::Hl7, input)?;
    assert_eq!(
        instance,
        Instance::Group(
            vec![
                (
                    "MSH".into(),
                    Instance::Group(
                        vec![
                            (
                                "Separator".into(),
                                Instance::Scalar(Value::String("|".into()))
                            ),
                            (
                                "Encoding".into(),
                                Instance::Scalar(Value::String("^~\\&".into()))
                            ),
                            (
                                "Sender".into(),
                                Instance::Scalar(Value::String("SOURCE".into()))
                            ),
                        ]
                        .into()
                    )
                ),
                (
                    "PID".into(),
                    Instance::Group(
                        vec![
                            ("Amount".into(), Instance::Scalar(Value::Float(12.34))),
                            (
                                "Date".into(),
                                Instance::Scalar(Value::String("20261026".into()))
                            ),
                            (
                                "Status".into(),
                                Instance::Scalar(Value::String("OK".into()))
                            ),
                        ]
                        .into()
                    )
                ),
            ]
            .into()
        )
    );
    assert_eq!(
        document::serialize(&schema, &restored, EdiBoundaryKind::Hl7, &instance, "")?,
        b"MSH|^~\\&|SOURCE\rPID|12.34|20261026|OK\r"
    );
    // Lexical formatting must still fail before the later code-list check.
    let invalid = document::parse(
        &schema,
        &restored,
        EdiBoundaryKind::Hl7,
        b"MSH|^~\\&|SOURCE\rPID|1234|bad|NO\r",
    )?;
    assert!(matches!(
        document::serialize(&schema, &restored, EdiBoundaryKind::Hl7, &invalid, ""),
        Err(EdiFormatError::LexicalFormatValue { path, .. }) if path == "PID/Date"
    ));
    let invalid = document::parse(
        &schema,
        &restored,
        EdiBoundaryKind::Hl7,
        b"MSH|^~\\&|SOURCE\rPID|1234|20261026|NO\r",
    )?;
    let result = document::serialize(&schema, &restored, EdiBoundaryKind::Hl7, &invalid, "");
    assert!(
        matches!(result, Err(EdiFormatError::Validation(_))),
        "{result:?}"
    );
    Ok(())
}
