use super::*;
fn text(s: &str) -> Instance {
    Instance::Scalar(Value::String(s.into()))
}
fn fixture(value: &str) -> (String, Instance) {
    let mut schema = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("Text", ir::ScalarType::String),
            SchemaNode::group("Empty", vec![]),
            SchemaNode::scalar("Item", ir::ScalarType::String),
        ],
    );
    schema.xml_namespace = ir::XmlNamespace::qualified("urn:fixture");
    let body = Instance::Group(
        vec![
            ("Text".into(), text(value)),
            ("Empty".into(), Instance::Group(vec![].into())),
            (
                "Item".into(),
                Instance::MappedSequence(vec![text("one"), text("")]),
            ),
        ]
        .into(),
    );
    (
        codegen_schema::encode(&schema, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap(),
        body,
    )
}
#[derive(Default)]
struct Output {
    bytes: Vec<u8>,
    writes: Vec<usize>,
    flushes: usize,
    fail_write: Option<usize>,
    fail_flush: Option<usize>,
    chunk: Option<usize>,
}
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.writes.push(bytes.len());
        if self.fail_write == Some(self.writes.len()) {
            return Err(std::io::Error::other("write failure"));
        }
        let count = self.chunk.unwrap_or(bytes.len()).min(bytes.len());
        self.bytes.extend_from_slice(&bytes[..count]);
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.flushes += 1;
        if self.fail_flush == Some(self.flushes) {
            Err(std::io::Error::other("flush failure"))
        } else {
            Ok(())
        }
    }
}
#[test]
fn successful_stream_matches_memory_and_flushes_without_owning_output() {
    let (schema, body) = fixture("alpha");
    let hint = Some("urn:fixture file:///C:/schema.xsd");
    let document = prepare_mapforce_xml_document(&schema, &body, hint).unwrap();
    let mut output = Output::default();
    document.write_to(&mut output).unwrap();
    assert_eq!(
        output.bytes,
        super::super::serialize_mapforce_xml_document(&schema, &body, hint).unwrap()
    );
    assert_eq!(output.writes, [247]);
    assert_eq!(output.flushes, 4);
    output.write_all(b"host still owns it").unwrap();
    let mut short = Output {
        chunk: Some(17),
        ..Default::default()
    };
    document.write_to(&mut short).unwrap();
    assert_eq!(short.bytes, output.bytes[..247]);
    assert_eq!(short.flushes, 4);
}
#[test]
fn invalid_text_retains_only_native_committed_buffers() {
    for (prefix, length) in [
        ("x".to_string(), 184),
        ("x".repeat(5960), 6143),
        ("&".repeat(2048), 6144),
        ("é".repeat(4096), 6144),
        ("x".repeat(12288), 12286),
    ] {
        let (schema, body) = fixture(&(prefix + "\0suffix"));
        let document = prepare_mapforce_xml_document(
            &schema,
            &body,
            Some("urn:fixture file:///C:/schema.xsd"),
        )
        .unwrap();
        let mut output = Output::default();
        assert!(matches!(
            document.write_to(&mut output),
            Err(MapForceXmlWriteError::Format(MapForceXmlError::Character(
                0
            )))
        ));
        assert_eq!(output.bytes.len(), length);
        assert_eq!(output.flushes, 2);
    }
}
#[test]
fn io_failure_is_not_retried_by_cleanup_and_prepared_document_is_reusable() {
    let (schema, body) = fixture("alpha");
    let document = prepare_mapforce_xml_document(&schema, &body, None).unwrap();
    let mut failed = Output {
        fail_write: Some(1),
        ..Default::default()
    };
    assert!(document.write_to(&mut failed).is_err());
    assert_eq!(failed.writes.len(), 1);
    assert_eq!(failed.flushes, 2);
    assert!(failed.bytes.is_empty());
    for (fail, count) in [(1, 3), (2, 4), (3, 4), (4, 4)] {
        let mut failed = Output {
            fail_flush: Some(fail),
            ..Default::default()
        };
        assert!(document.write_to(&mut failed).is_err());
        assert_eq!(failed.flushes, count);
    }
    document.write_to(&mut Output::default()).unwrap();
}
