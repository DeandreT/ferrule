use super::*;
use ir::{Instance, ScalarType, SchemaNode};
use std::cell::RefCell;

fn descriptor() -> String {
    let fields = |name: &str, count: usize| {
        SchemaNode::group(
            name,
            (1..=count)
                .map(|i| SchemaNode::scalar(format!("E{i}"), ScalarType::String))
                .collect(),
        )
    };
    let schema = SchemaNode::group(
        "Envelope",
        vec![
            SchemaNode::group(
                "Interchange",
                vec![
                    fields("ISA", 16).repeating(),
                    SchemaNode::group(
                        "Group",
                        vec![
                            fields("GS", 8).repeating(),
                            SchemaNode::group(
                                "Message_850",
                                vec![fields("ST", 2), fields("SE", 2)],
                            )
                            .repeating(),
                            fields("GE", 2).repeating(),
                        ],
                    )
                    .repeating(),
                    fields("IEA", 2).repeating(),
                ],
            )
            .repeating(),
        ],
    );
    serde_json::json!({"schema":codegen_schema::encode(&schema,1_048_576).unwrap(),"version":"006040","envelope_profile":"mapforce_interchanges","separators":null,"constraints":[],"implied_decimals":[],"lexical_formats":[],"autocomplete":null,"lenient_segments":false}).to_string()
}
fn source(descriptor: &str, path: &str) -> ParsedX12Source {
    ParsedX12Source {
        descriptor: descriptor.into(),
        document: ir::DocumentMember::new(path, Instance::Group(vec![].into())).unwrap(),
    }
}

#[test]
fn callbacks_are_lazy_and_each_input_is_closed_before_the_next_request() {
    let descriptor = descriptor();
    let events = RefCell::new(Vec::new());
    let mut inputs = X12SourceAcquisition::new(
        &descriptor,
        || {
            events.borrow_mut().push("filename".to_string());
            Ok::<_, &str>("pattern".to_string())
        },
        |path| {
            events.borrow_mut().push(format!("select:{path}"));
            Ok(vec![Ok("a".to_string()), Ok("b".to_string())].into_iter())
        },
        |path| {
            events.borrow_mut().push(format!("load:{path}"));
            Ok::<_, &str>(source(&descriptor, path))
        },
        2,
    )
    .unwrap();
    assert!(events.borrow().is_empty());
    assert_eq!(inputs.next().unwrap().unwrap().path(), "a");
    assert_eq!(*events.borrow(), ["filename", "select:pattern", "load:a"]);
    assert_eq!(inputs.next().unwrap().unwrap().path(), "b");
    assert!(inputs.next().is_none());
    assert!(inputs.next().is_none());
    assert_eq!(
        *events.borrow(),
        ["filename", "select:pattern", "load:a", "load:b"]
    );
}

#[test]
fn request_load_and_descriptor_errors_end_only_this_acquisition() {
    let descriptor = descriptor();
    let events = RefCell::new(Vec::new());
    let mut request = X12SourceAcquisition::new(
        &descriptor,
        || Ok::<_, &str>("p".into()),
        |_| Ok(vec![Ok("a".into()), Err("request"), Ok("never".into())].into_iter()),
        |path| {
            events.borrow_mut().push(path.to_string());
            Ok(source(&descriptor, path))
        },
        3,
    )
    .unwrap();
    assert!(request.next().unwrap().is_ok());
    assert!(matches!(
        request.next(),
        Some(Err(X12AcquisitionError::Request("request")))
    ));
    assert!(request.next().is_none());
    assert_eq!(*events.borrow(), ["a"]);
    let mut load = X12SourceAcquisition::new(
        &descriptor,
        || Ok::<_, &str>("p".into()),
        |_| Ok(vec![Ok("a".into()), Ok("never".into())].into_iter()),
        |_| Err::<ParsedX12Source, _>("load"),
        3,
    )
    .unwrap();
    assert!(matches!(
        load.next(),
        Some(Err(X12AcquisitionError::Load { source: "load", .. }))
    ));
    assert!(load.next().is_none());
    let mut mismatch = X12SourceAcquisition::new(
        &descriptor,
        || Ok::<_, &str>("p".into()),
        |_| Ok::<_, &str>(vec![Ok("a".into())].into_iter()),
        |path| Ok(source(&(descriptor.clone() + " "), path)),
        1,
    )
    .unwrap();
    assert!(matches!(
        mismatch.next(),
        Some(Err(X12AcquisitionError::DescriptorMismatch { .. }))
    ));
    assert!(mismatch.next().is_none());
}

#[test]
fn limits_and_filename_errors_do_not_load_inputs() {
    let descriptor = descriptor();
    let mut failure = X12SourceAcquisition::new(
        &descriptor,
        || Err::<String, _>("filename"),
        |_| Ok::<_, &str>(std::iter::empty()),
        |_| panic!("load"),
        1,
    )
    .unwrap();
    assert!(matches!(
        failure.next(),
        Some(Err(X12AcquisitionError::Filename("filename")))
    ));
    assert!(failure.next().is_none());
    let mut zero = X12SourceAcquisition::new(
        &descriptor,
        || Ok::<_, &str>("p".into()),
        |_| Ok::<_, &str>(vec![Ok("a".into())].into_iter()),
        |_| panic!("load"),
        0,
    )
    .unwrap();
    assert!(matches!(
        zero.next(),
        Some(Err(X12AcquisitionError::Documents))
    ));
    assert!(zero.next().is_none());
    let mut empty = X12SourceAcquisition::new(
        &descriptor,
        || Ok::<_, &str>("p".into()),
        |_| Ok::<_, &str>(std::iter::empty()),
        |_| panic!("load"),
        0,
    )
    .unwrap();
    assert!(empty.next().is_none());
    assert!(matches!(
        X12SourceAcquisition::new(
            &descriptor,
            || Ok::<_, &str>("p".into()),
            |_| Ok::<_, &str>(std::iter::empty()),
            |_| panic!("load"),
            MAX_X12_SOURCE_DOCUMENTS + 1
        ),
        Err(X12AcquisitionError::InvalidLimit)
    ));
}

#[test]
fn native_source_binding_keeps_encoding_and_rejects_ambiguous_descriptors() {
    let boundary = descriptor();
    let native = serde_json::json!({"boundary":boundary,"source_encoding":"latin1"}).to_string();
    let mut inputs = X12SourceAcquisition::new(
        &native,
        || Ok::<_, &str>("p".into()),
        |_| Ok::<_, &str>(vec![Ok("a".into()), Ok("b".into())].into_iter()),
        |path| Ok(source(if path == "a" { &native } else { &boundary }, path)),
        2,
    )
    .unwrap();
    assert!(inputs.next().unwrap().is_ok());
    assert!(matches!(
        inputs.next(),
        Some(Err(X12AcquisitionError::DescriptorMismatch { .. }))
    ));
    assert!(inputs.next().is_none());
    for invalid in [
        format!(
            r#"{{"boundary":{0},"source_encoding":"latin1","source_encoding":"utf8"}}"#,
            serde_json::to_string(&boundary).unwrap()
        ),
        format!(
            r#"{{"boundary":{0},"boundary":{0},"source_encoding":"latin1"}}"#,
            serde_json::to_string(&boundary).unwrap()
        ),
        serde_json::json!({"boundary":boundary,"source_encoding":"latin1","extra":true})
            .to_string(),
        serde_json::json!({"boundary":boundary,"source_encoding":"windows1252"}).to_string(),
        serde_json::json!({"boundary":boundary}).to_string(),
    ] {
        assert!(source_mapforce::source_profile(&invalid).is_err());
    }
}
