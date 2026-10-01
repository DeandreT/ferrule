use mapping::{
    FileCodecError, PdfAnchorAssignment, PdfAnchorAxis, PdfCapture, PdfCaptureAlgorithm,
    PdfCommand, PdfCoordinate, PdfEdgeFind, PdfEdgeRows, PdfGroup, PdfLayout, PdfMetricMatch,
    PdfPageSelection, PdfReference, PdfRegion, PdfTextCase, PdfTextGroup, PdfTextGroupOutput,
    PdfTextGroups, PdfTextMatch, PdfTextProperties, PdfTextRows, PdfVerticalBoundaryFind,
    pdf_layout_file,
};
use serde_json::Value;
use std::collections::BTreeMap;

fn high() -> f64 {
    f64::from_bits(0x0031_fa18_2c40_c60e)
}
fn low() -> f64 {
    f64::from_bits(0x0031_fa18_2c40_c60d)
}
fn region() -> PdfRegion {
    PdfRegion {
        left: PdfCoordinate::new(PdfReference::Left, high()),
        top: PdfCoordinate::new(PdfReference::Top, low()),
        right: PdfCoordinate::new(PdfReference::Right, -0.0),
        bottom: PdfCoordinate::new(PdfReference::Bottom, 0.0),
    }
}
fn capture(name: &str, region: PdfRegion) -> PdfCommand {
    PdfCommand::Capture(PdfCapture {
        name: name.into(),
        region,
        algorithm: PdfCaptureAlgorithm::default(),
    })
}
fn repeated(name: &str) -> Vec<PdfCommand> {
    vec![PdfCommand::GroupPerPage(PdfGroup {
        name: name.into(),
        region: region(),
        children: vec![capture("text", region())],
    })]
}
fn complete_layout() -> PdfLayout {
    PdfLayout::new(
        "PDF",
        PdfPageSelection::First,
        vec![
            capture("main", region()),
            PdfCommand::Anchor(PdfAnchorAssignment {
                name: "anchor".into(),
                axis: PdfAnchorAxis::Horizontal,
                at: PdfCoordinate::new(PdfReference::Left, high()),
            }),
            PdfCommand::BoundaryFindVertical(PdfVerticalBoundaryFind {
                region: region(),
                begin_anchor: "start".into(),
                end_anchor: "end".into(),
                find: PdfEdgeFind {
                    fill: high(),
                    prominence: -0.0,
                },
            }),
            PdfCommand::TextGroups(PdfTextGroups {
                region: region(),
                groups: vec![PdfTextGroup {
                    output: PdfTextGroupOutput::Repeated {
                        name: "Matches".into(),
                    },
                    matcher: PdfTextMatch {
                        needle: "__ferrule_file FERRULE-F64-BITS:deadbeef".into(),
                        case: PdfTextCase::Sensitive,
                        flexible_whitespace: true,
                        properties: PdfTextProperties {
                            font_face: None,
                            cell_height: Some(PdfMetricMatch {
                                value: high(),
                                deviation: low(),
                            }),
                            baseline_angle: Some(PdfMetricMatch {
                                value: -0.0,
                                deviation: high(),
                            }),
                        },
                    },
                    children: vec![capture("matched", region())],
                }],
            }),
            PdfCommand::EdgeRows(PdfEdgeRows {
                region: region(),
                find: PdfEdgeFind {
                    fill: high(),
                    prominence: low(),
                },
                minimum_extent: Some(high()),
                fallback_anchor: Some(region()),
                children: repeated("EdgeRow"),
            }),
            PdfCommand::TextRows(PdfTextRows {
                region: region(),
                minimum_extent: Some(low()),
                children: repeated("TextRow"),
            }),
        ],
    )
    .unwrap()
}
fn bits(value: &Value, path: &str, result: &mut BTreeMap<String, u64>) {
    match value {
        Value::Number(number) if number.is_f64() => {
            result.insert(path.into(), number.as_f64().unwrap().to_bits());
        }
        Value::Array(items) => {
            for (index, value) in items.iter().enumerate() {
                bits(value, &format!("{path}/{index}"), result);
            }
        }
        Value::Object(fields) => {
            for (key, value) in fields {
                bits(value, &format!("{path}/{key}"), result);
            }
        }
        _ => (),
    }
}
fn snapshot(layout: &PdfLayout) -> BTreeMap<String, u64> {
    let mut result = BTreeMap::new();
    bits(&serde_json::to_value(layout).unwrap(), "", &mut result);
    result
}
#[test]
fn preserves_offsets_extents_edge_thresholds_metrics_and_signed_zero() {
    let original = complete_layout();
    let ordinary = serde_json::to_string(&original).unwrap();
    let reopened: PdfLayout = serde_json::from_str(&ordinary).unwrap();
    assert_ne!(
        snapshot(&original),
        snapshot(&reopened),
        "ordinary layout parse drifts"
    );
    let encoded = pdf_layout_file::encode_pretty(&original).unwrap();
    let envelope: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(envelope["__ferrule_file"]["kind"], "pdf_layout");
    assert_eq!(envelope["__ferrule_file"]["version"], 2);
    let decoded = pdf_layout_file::decode_bytes(encoded.as_bytes()).unwrap();
    assert_eq!(snapshot(&decoded), snapshot(&original));
    assert_eq!(serde_json::to_string(&decoded).unwrap(), ordinary);
    assert_eq!(pdf_layout_file::encode_pretty(&decoded).unwrap(), encoded);
    let floats = snapshot(&decoded);
    for path in [
        "/commands/0/region/left/offset",
        "/commands/1/at/offset",
        "/commands/2/find/fill",
        "/commands/3/groups/0/matcher/properties/cell_height/value",
        "/commands/3/groups/0/matcher/properties/cell_height/deviation",
        "/commands/3/groups/0/matcher/properties/baseline_angle/value",
        "/commands/3/groups/0/matcher/properties/baseline_angle/deviation",
        "/commands/4/find/prominence",
        "/commands/4/minimum_extent",
        "/commands/5/minimum_extent",
    ] {
        assert!(floats.contains_key(path), "missing {path}");
    }
    assert_eq!(floats["/commands/2/find/prominence"], (-0.0_f64).to_bits());
    assert!(serde_json::from_str::<PdfLayout>(&encoded).is_err());
}
#[test]
fn stable_layout_uses_ordinary_legacy_wire_and_error_behavior() {
    let layout = PdfLayout::new(
        "PDF",
        PdfPageSelection::First,
        vec![capture("text", PdfRegion::full())],
    )
    .unwrap();
    let ordinary = format!("{}\n", serde_json::to_string_pretty(&layout).unwrap());
    assert_eq!(pdf_layout_file::encode_pretty(&layout).unwrap(), ordinary);
    assert_eq!(
        snapshot(&pdf_layout_file::decode_str(&ordinary).unwrap()),
        snapshot(&layout)
    );
    let ignored = format!("{{\"ignored\":1e400,{}", &ordinary[1..]);
    assert!(serde_json::from_str::<PdfLayout>(&ignored).is_ok());
    assert!(pdf_layout_file::decode_str(&ignored).is_ok());
    let malformed = r#"{"root_name":"PDF","page_selection":{"kind":"first"},"commands":[]}"#;
    let legacy = serde_json::from_str::<PdfLayout>(malformed)
        .unwrap_err()
        .to_string();
    let Err(FileCodecError::Deserialization(actual)) = pdf_layout_file::decode_str(malformed)
    else {
        panic!("legacy error category");
    };
    assert_eq!(actual.to_string(), legacy);
}
#[test]
fn rejects_stale_malformed_nonfinite_wrong_kind_and_oversized_metadata() {
    let encoded = pdf_layout_file::encode_pretty(&complete_layout()).unwrap();
    let envelope: Value = serde_json::from_str(&encoded).unwrap();
    for bad in [
        "7ff0000000000000",
        "7ff8000000000000",
        "0031fa182c40c60",
        "bogus",
    ] {
        let mut changed = envelope.clone();
        changed["__ferrule_file"]["float_bits"]["/commands/0/region/left/offset"] =
            Value::String(bad.into());
        assert!(matches!(
            pdf_layout_file::decode_str(&changed.to_string()),
            Err(FileCodecError::InvalidFloatMetadata { .. })
        ));
    }
    let mut changed = envelope.clone();
    changed["document"]["commands"][0]["region"]["left"]["offset"] = serde_json::json!(4.0);
    assert!(matches!(
        pdf_layout_file::decode_str(&changed.to_string()),
        Err(FileCodecError::InvalidFloatMetadata { .. })
    ));
    let mut changed = envelope.clone();
    changed["__ferrule_file"]["kind"] = Value::String("project".into());
    assert!(matches!(
        pdf_layout_file::decode_str(&changed.to_string()),
        Err(FileCodecError::WrongKind { .. })
    ));
    let mut changed = envelope.clone();
    changed["__ferrule_file"]["version"] = serde_json::json!(999);
    assert!(matches!(
        pdf_layout_file::decode_str(&changed.to_string()),
        Err(FileCodecError::UnsupportedVersion { version: 999 })
    ));
    assert!(matches!(
        pdf_layout_file::decode_bytes(&[0xff]),
        Err(FileCodecError::InvalidUtf8(_))
    ));
    for float in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut invalid = PdfRegion::full();
        invalid.left.offset = float;
        assert!(
            PdfLayout::new(
                "PDF",
                PdfPageSelection::First,
                vec![capture("text", invalid)]
            )
            .is_err(),
            "layout constructor rejects nonfinite before encoding"
        );
    }
    let oversized = format!(
        "{{\"__ferrule_file\":{{\"kind\":\"pdf_layout\",\"version\":2,\"float_bits\":{{}}}},\"document\":{{}},\"padding\":\"{}\"}}",
        "x".repeat(pdf_layout_file::MAX_DOCUMENT_BYTES)
    );
    assert!(matches!(
        pdf_layout_file::decode_str(&oversized),
        Err(FileCodecError::TooLarge { .. })
    ));
    let duplicate = encoded.replacen("\"version\": 2", "\"version\": 2, \"version\": 2", 1);
    assert!(matches!(
        pdf_layout_file::decode_str(&duplicate),
        Err(FileCodecError::InvalidEnvelope { .. })
    ));
}
