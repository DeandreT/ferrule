use ir::SchemaNode;
use mapping::{
    FormatOptions, Project, Scope, X12Descriptor, X12DescriptorView, X12SegmentLineEnding,
};
use serde_json::json;

fn project() -> Project {
    Project {
        execution_contract: None,
        source: SchemaNode::group("Input", vec![]),
        target: SchemaNode::group("Output", vec![]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Default::default(),
        root: Scope::default(),
    }
}

const DESCRIPTOR: &str = r#"{"version":"004010","schema":"s","separators":null,"constraints":[],"lenient_segments":false,"implied_decimals":[],"lexical_formats":[],"autocomplete":null}"#;

#[test]
fn closed_values_and_default_canonical_bytes_for_both_descriptor_views() {
    let old = serde_json::to_string(&FormatOptions::default()).unwrap();
    let explicit: FormatOptions =
        serde_json::from_str(r#"{"x12_segment_line_ending":"lf"}"#).unwrap();
    assert_eq!(serde_json::to_string(&explicit).unwrap(), old);
    for (wire, expected) in [
        ("lf", X12SegmentLineEnding::Lf),
        ("crlf", X12SegmentLineEnding::CrLf),
    ] {
        let text = format!(
            "{},\"segment_line_ending\":\"{wire}\"}}",
            &DESCRIPTOR[..DESCRIPTOR.len() - 1]
        );
        let full: X12Descriptor = serde_json::from_str(&text).unwrap();
        let staged: X12DescriptorView = serde_json::from_str(&text).unwrap();
        assert_eq!(full.segment_line_ending, expected);
        assert_eq!(staged.segment_line_ending().unwrap(), expected);
        let canonical = serde_json::to_string(&full).unwrap();
        if expected.is_lf() {
            assert_eq!(canonical, DESCRIPTOR);
        } else {
            assert_eq!(canonical, text);
        }
    }
    assert_eq!(
        serde_json::to_string(&serde_json::from_str::<X12Descriptor>(DESCRIPTOR).unwrap()).unwrap(),
        DESCRIPTOR
    );
    for raw in [
        "null",
        "true",
        "4",
        "[]",
        "{}",
        "{\"lf\":null}",
        "\"CRLF\"",
        "\"none\"",
        "\"unknown\"",
    ] {
        let text = format!(
            "{},\"segment_line_ending\":{raw}}}",
            &DESCRIPTOR[..DESCRIPTOR.len() - 1]
        );
        assert!(
            serde_json::from_str::<X12Descriptor>(&text).is_err(),
            "{text}"
        );
        let view: X12DescriptorView = serde_json::from_str(&text).unwrap();
        assert!(view.segment_line_ending().is_err());
        assert!(
            serde_json::from_str::<FormatOptions>(&format!(
                "{{\"x12_segment_line_ending\":{raw}}}"
            ))
            .is_err()
        );
    }
    for prefix in [
        r#"{"segment_line_ending":"lf","segment_line_ending":"crlf","#,
        r#"{"unknown_policy":true,"#,
    ] {
        let text = format!("{prefix}{}", &DESCRIPTOR[1..]);
        assert!(serde_json::from_str::<X12Descriptor>(&text).is_err());
        assert!(serde_json::from_str::<X12DescriptorView>(&text).is_err());
    }
    assert!(
        serde_json::from_str::<FormatOptions>(
            r#"{"x12_segment_line_ending":"lf","x12_segment_line_ending":"crlf"}"#
        )
        .is_err()
    );
    // Ordinary FormatOptions unknown-field compatibility is deliberately intact.
    assert!(serde_json::from_str::<FormatOptions>(r#"{"future_unrelated":true}"#).is_ok());
    let nested = DESCRIPTOR.replace(
        "\"separators\":null",
        r#""separators":{"element":"*","element":"!","component":":","segment":"~"}"#,
    );
    assert!(serde_json::from_str::<X12Descriptor>(&nested).is_err());
    assert!(
        serde_json::from_str::<X12DescriptorView>(&nested)
            .unwrap()
            .separators()
            .is_err()
    );
}

#[test]
fn capability_is_required_even_for_invalid_source_and_named_placements() {
    let baseline = project();
    let old = mapping::project_file::encode_pretty(&baseline).unwrap();
    assert!(!old.contains("__ferrule_file"));
    for placement in 0..4 {
        let mut p = baseline.clone();
        let options = FormatOptions {
            x12_segment_line_ending: X12SegmentLineEnding::CrLf,
            ..Default::default()
        };
        match placement {
            0 => p.source_options = options,
            1 => p.target_options = options,
            2 => p.extra_sources.push(mapping::NamedSource {
                name: "reference".into(),
                path: "reference.x12".into(),
                schema: p.source.clone(),
                options,
                dynamic_path: None,
            }),
            _ => p.extra_targets.push(mapping::NamedTarget {
                name: "copy".into(),
                path: None,
                schema: p.target.clone(),
                options,
                root: Scope::default(),
            }),
        }
        assert!(p.uses_x12_segment_line_ending());
        assert!(mapping::project_file::decode_str(&serde_json::to_string(&p).unwrap()).is_err());
        let encoded = mapping::project_file::encode_pretty(&p).unwrap();
        let mut envelope: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(envelope["__ferrule_file"]["version"], 3);
        assert_eq!(
            envelope["__ferrule_file"]["capabilities"],
            json!(["x12-segment-line-ending-v1"])
        );
        assert_eq!(
            serde_json::to_value(mapping::project_file::decode_str(&encoded).unwrap()).unwrap(),
            serde_json::to_value(&p).unwrap()
        );
        envelope["__ferrule_file"]["version"] = 2.into();
        envelope["__ferrule_file"]
            .as_object_mut()
            .unwrap()
            .remove("capabilities");
        assert!(mapping::project_file::decode_str(&envelope.to_string()).is_err());
        let mut mismatch: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        mismatch["document"] = serde_json::to_value(&baseline).unwrap();
        assert!(mapping::project_file::decode_str(&mismatch.to_string()).is_err());
        mismatch["__ferrule_file"]["capabilities"] = json!(["future-x12-policy"]);
        assert!(mapping::project_file::decode_str(&mismatch.to_string()).is_err());
        let pipeline = mapping::Pipeline {
            main_mapping_path: None,
            stages: vec![mapping::PipelineStage {
                id: "one".into(),
                mapping_path: None,
                project: p.clone(),
                source: mapping::PipelineInput::Host {
                    name: "input".into(),
                },
                extra_sources: vec![],
            }],
        };
        let text = mapping::pipeline_file::encode_pretty(&pipeline).unwrap();
        assert!(text.contains("x12-segment-line-ending-v1"));
        assert!(mapping::pipeline_file::decode_str(&text).is_ok());
        let document = mapping::DocumentProject::new(p, mapping::PrimarySourcePlan::new(1));
        let text = mapping::document_project_file::encode_pretty(&document).unwrap();
        assert!(text.contains("x12-segment-line-ending-v1"));
        assert!(mapping::document_project_file::decode_str(&text).is_ok());
    }
    assert_eq!(
        mapping::project_file::encode_pretty(&baseline).unwrap(),
        old
    );
}
