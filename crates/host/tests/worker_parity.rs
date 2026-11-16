use ferrule_host::{
    BoundaryId, BoundaryShape, FileCodecError, FunctionId, InspectionError, InspectionLimits,
    NativeLocation, ProjectFileLayout, ResourceKind, StructuralCounts, ValidationEndpoint,
    ValidationError, ValidationLimits, ValidationOwner, inspect, load, validate,
};
use ir::{SchemaNode, Value};
use mapping::{Binding, ExecutionContract, Node, Scope, project_file};
use serde_json::Value as Json;

const INVENTORY: &[u8] = include_bytes!("fixtures/orchard-inventory.json");
const LEDGER: &[u8] = include_bytes!("fixtures/versioned-ledger.json");
const MAXIMUM_BYTES: usize = 4 * 1024 * 1024;

fn pointer(path: &str) -> NativeLocation {
    NativeLocation::JsonPointer(path.into())
}

#[test]
fn load_preserves_exact_values_and_reports_the_admitted_file_layout() {
    let bare = load(INVENTORY, INVENTORY.len()).unwrap();
    assert_eq!(bare.layout(), ProjectFileLayout::Bare);
    let versioned = load(LEDGER, LEDGER.len()).unwrap();
    assert_eq!(versioned.layout(), ProjectFileLayout::Envelope);
    assert!(matches!(
        versioned.project().graph.nodes[&9],
        Node::Const {
            value: Value::Int(9_007_199_254_741_029)
        }
    ));
    assert!(matches!(
        versioned.project().graph.nodes[&10],
        Node::Const { value: Value::Float(value) } if value.to_bits() == 1 << 63
    ));
    let mut v3 = versioned.into_project();
    v3.execution_contract = Some(ExecutionContract::ClosedPrimarySourceV1);
    let v3 = project_file::encode_pretty(&v3).unwrap();
    let file: Json = serde_json::from_str(&v3).unwrap();
    assert_eq!(file["__ferrule_file"]["version"], 3);
    let loaded = load(v3.as_bytes(), MAXIMUM_BYTES).unwrap();
    assert_eq!(loaded.layout(), ProjectFileLayout::Envelope);
    assert_eq!(
        loaded.project().execution_contract,
        Some(ExecutionContract::ClosedPrimarySourceV1)
    );

    // Legacy admission ignores fields the native model does not interpret.
    let legacy = format!(
        "{{\"ignored\":1e99999,{}",
        std::str::from_utf8(INVENTORY)
            .unwrap()
            .strip_prefix('{')
            .unwrap()
    );
    let loaded = load(legacy.as_bytes(), MAXIMUM_BYTES).unwrap();
    assert_eq!(loaded.layout(), ProjectFileLayout::Bare);
}

#[test]
fn load_checks_bytes_before_decoding_and_preserves_codec_errors() {
    assert!(matches!(
        load(INVENTORY, INVENTORY.len() - 1),
        Err(FileCodecError::TooLarge { bytes, max })
            if bytes == INVENTORY.len() && max == INVENTORY.len() - 1
    ));
    assert!(matches!(
        load(&[0xff, 0xff], 1),
        Err(FileCodecError::TooLarge { bytes: 2, max: 1 })
    ));
    assert!(matches!(
        load(&[0xff], 1),
        Err(FileCodecError::InvalidUtf8(_))
    ));
    assert!(matches!(
        load(b"{", 1),
        Err(FileCodecError::Deserialization(_))
    ));

    let mut envelope: Json = serde_json::from_slice(LEDGER).unwrap();
    envelope["__ferrule_file"]["version"] = 99.into();
    assert!(matches!(
        load(envelope.to_string().as_bytes(), MAXIMUM_BYTES),
        Err(FileCodecError::UnsupportedVersion { version: 99 })
    ));
    envelope["__ferrule_file"]["version"] = 2.into();
    envelope["document"]["graph"]["nodes"]["10"]["value"] = 2.5.into();
    assert!(matches!(
        load(envelope.to_string().as_bytes(), MAXIMUM_BYTES),
        Err(FileCodecError::InvalidFloatMetadata { .. })
    ));
}

#[test]
fn inspection_preserves_order_counts_and_unresolved_resource_locations() {
    use BoundaryId::{NamedSource, NamedTarget, Source, Target};
    use ResourceKind::*;

    let loaded = load(INVENTORY, MAXIMUM_BYTES).unwrap();
    let report = inspect(
        loaded.project(),
        loaded.layout(),
        InspectionLimits::default(),
    )
    .unwrap();
    assert_eq!(
        report,
        inspect(
            loaded.project(),
            loaded.layout(),
            InspectionLimits::default()
        )
        .unwrap()
    );
    assert_eq!(
        report.counts,
        StructuralCounts {
            schema_nodes: 9,
            graph_nodes: 2,
            scope_nodes: 3,
            user_functions: 0,
            failure_rules: 1,
            total_items: 36,
        }
    );
    assert!(!report.truncated);
    let boundaries: Vec<_> = report
        .boundaries
        .iter()
        .map(|boundary| {
            (
                boundary.id,
                boundary.name.text.as_str(),
                boundary.root_name.text.as_str(),
                boundary.shape,
                boundary.repeating,
                boundary.native_location.clone(),
            )
        })
        .collect();
    assert_eq!(
        boundaries,
        [
            (
                Source,
                "Orchard",
                "Orchard",
                BoundaryShape::Object,
                false,
                pointer("/source")
            ),
            (
                NamedSource(0),
                "Rates",
                "Rate",
                BoundaryShape::ScalarUnion,
                false,
                pointer("/extra_sources/0")
            ),
            (
                Target,
                "Report",
                "Report",
                BoundaryShape::Object,
                false,
                pointer("/target")
            ),
            (
                NamedTarget(0),
                "Archive",
                "ArchiveEntry",
                BoundaryShape::Object,
                true,
                pointer("/extra_targets/0")
            ),
        ]
    );
    let resources: Vec<_> = report
        .unresolved_resources
        .iter()
        .map(|resource| {
            (
                resource.boundary_id,
                resource.kind,
                resource.native_location.clone(),
            )
        })
        .collect();
    assert_eq!(
        resources,
        [
            (Source, DesignTimeInput, pointer("/source_path")),
            (
                Source,
                ExternalSource,
                pointer("/source_options/external_source")
            ),
            (
                Source,
                SchemaReference,
                pointer("/source_options/json_schema_unresolved_reference")
            ),
            (
                NamedSource(0),
                DesignTimeInput,
                pointer("/extra_sources/0/path")
            ),
            (
                NamedSource(0),
                DynamicInput,
                pointer("/extra_sources/0/dynamic_path")
            ),
            (
                NamedSource(0),
                EdiConfiguration,
                pointer("/extra_sources/0/options/edi_config_reference")
            ),
            (Target, DesignTimeOutput, pointer("/target_path")),
            (
                Target,
                XmlSchemaHint,
                pointer("/target_options/xml_schema_hints")
            ),
            (
                NamedTarget(0),
                DesignTimeOutput,
                pointer("/extra_targets/0/path")
            ),
            (
                NamedTarget(0),
                DynamicOutput,
                pointer("/extra_targets/0/root/output_path")
            ),
        ]
    );
    let original: Json = serde_json::from_slice(INVENTORY).unwrap();
    for (_, _, location) in resources {
        let NativeLocation::JsonPointer(path) = location else {
            panic!("{location:?}")
        };
        assert!(original.pointer(&path).is_some(), "{path}");
    }
    assert!(!format!("{report:?}").contains("fixture-private"));
}

#[test]
fn item_budget_is_inclusive_and_preflights_validation() {
    let loaded = load(INVENTORY, MAXIMUM_BYTES).unwrap();
    let mut inspection = InspectionLimits {
        maximum_items: 36,
        ..InspectionLimits::default()
    };
    assert!(inspect(loaded.project(), loaded.layout(), inspection).is_ok());
    inspection.maximum_items -= 1;
    assert_eq!(
        inspect(loaded.project(), loaded.layout(), inspection),
        Err(InspectionError::ItemLimitExceeded)
    );
    assert_eq!(
        validate(
            loaded.project(),
            loaded.layout(),
            ValidationLimits {
                inspection,
                ..ValidationLimits::default()
            }
        ),
        Err(ValidationError::Inspection(
            InspectionError::ItemLimitExceeded
        ))
    );
}

#[test]
fn schema_and_scope_depth_have_independent_limits() {
    let mut project = load(LEDGER, MAXIMUM_BYTES).unwrap().into_project();
    let layout = ProjectFileLayout::Bare;
    let limits = InspectionLimits::default();
    for _ in 0..32 {
        project.source = SchemaNode::group("row", vec![project.source]);
    }
    assert!(inspect(&project, layout, limits).is_ok());
    project.source = SchemaNode::group("too-deep", vec![project.source]);
    assert_eq!(
        inspect(&project, layout, limits),
        Err(InspectionError::SchemaDepthExceeded)
    );

    project.source = SchemaNode::group("Ledger", vec![]);
    for _ in 0..32 {
        project.root = Scope {
            children: vec![project.root],
            ..Scope::default()
        };
    }
    assert!(inspect(&project, layout, limits).is_ok());
    project.root = Scope {
        children: vec![project.root],
        ..Scope::default()
    };
    assert_eq!(
        inspect(&project, layout, limits),
        Err(InspectionError::ScopeDepthExceeded)
    );
}

#[test]
fn inspection_truncates_utf8_and_falls_back_for_long_pointers() {
    let mut loaded = load(INVENTORY, MAXIMUM_BYTES).unwrap().into_project();
    loaded.source.name = "é".repeat(40);
    let report = inspect(
        &loaded,
        ProjectFileLayout::Bare,
        InspectionLimits {
            maximum_text_bytes: 35,
            ..InspectionLimits::default()
        },
    )
    .unwrap();
    assert!(report.truncated);
    assert!(report.boundaries[0].root_name.truncated);
    assert_eq!(report.boundaries[0].root_name.text, "é".repeat(17));
    assert_eq!(report.boundaries[0].native_location, pointer("/source"));
    assert_eq!(
        report.unresolved_resources[2].kind,
        ResourceKind::SchemaReference
    );
    assert_eq!(
        report.unresolved_resources[2].native_location,
        NativeLocation::Document
    );
}

#[test]
fn validation_matches_order_and_preserves_large_and_versioned_owners() {
    let loaded = load(LEDGER, MAXIMUM_BYTES).unwrap();
    let unlimited = validate(
        loaded.project(),
        loaded.layout(),
        ValidationLimits::default(),
    )
    .unwrap();
    assert!(!unlimited.is_valid);
    assert!(!unlimited.truncated);
    assert_eq!(unlimited.total_issue_count, 2);
    let first_owner = ValidationOwner::GraphNode {
        function: Some(FunctionId::new(9_007_199_254_741_027)),
        node: 17,
    };
    let second_owner = ValidationOwner::UserFunction(FunctionId::new(9_007_199_254_741_031));
    assert_eq!(unlimited.diagnostics[0].owner, Some(first_owner));
    assert_eq!(
        unlimited.diagnostics[0].native_location,
        NativeLocation::GraphNode {
            function: Some(FunctionId::new(9_007_199_254_741_027)),
            node: 17,
        }
    );
    assert_eq!(unlimited.diagnostics[1].owner, Some(second_owner));
    let path = "/document/user_functions/9007199254741031";
    assert_eq!(unlimited.diagnostics[1].native_location, pointer(path));
    let original: Json = serde_json::from_slice(LEDGER).unwrap();
    assert_eq!(original.pointer(path).unwrap()["name"], "Summary");
    let direct = engine::validate(loaded.project());
    for (host, engine) in unlimited.diagnostics.iter().zip(direct) {
        assert_eq!(host.owner, engine.owner);
        assert_eq!(host.location.text, engine.location);
        assert_eq!(host.message.text, engine.message);
    }

    let limits = ValidationLimits {
        maximum_diagnostics: 1,
        maximum_issues: 2,
        ..ValidationLimits::default()
    };
    let limited = validate(loaded.project(), loaded.layout(), limits).unwrap();
    assert!(!limited.is_valid);
    assert!(limited.truncated);
    assert_eq!(limited.total_issue_count, 2);
    assert_eq!(limited.diagnostics, unlimited.diagnostics[..1]);
    assert_eq!(
        validate(
            loaded.project(),
            loaded.layout(),
            ValidationLimits {
                maximum_issues: 1,
                ..limits
            }
        ),
        Err(ValidationError::IssueLimitExceeded)
    );
}

#[test]
fn validation_reports_success_when_the_project_has_no_issues() {
    let mut project = load(LEDGER, MAXIMUM_BYTES).unwrap().into_project();
    project.user_functions.clear();
    let report = validate(
        &project,
        ProjectFileLayout::Envelope,
        ValidationLimits::default(),
    )
    .unwrap();
    assert!(report.is_valid);
    assert_eq!(report.total_issue_count, 0);
    assert!(report.diagnostics.is_empty());
    assert!(!report.truncated);
}

#[test]
fn validation_preserves_owner_when_long_pointer_falls_back() {
    let mut project = load(LEDGER, MAXIMUM_BYTES).unwrap().into_project();
    project.user_functions.clear();
    let mut schema = SchemaNode::group("detail", vec![]);
    let mut scope = Scope {
        target_field: "detail".into(),
        bindings: vec![Binding {
            target_field: "missing".into(),
            node: 9,
        }],
        ..Scope::default()
    };
    for index in 0..8 {
        let name = format!("level{index}");
        schema = SchemaNode::group(&name, vec![schema]);
        scope = Scope {
            target_field: name,
            children: vec![scope],
            ..Scope::default()
        };
    }
    project.target = SchemaNode::group("Summary", vec![schema]);
    project.root = Scope {
        children: vec![scope],
        ..Scope::default()
    };
    let report = validate(
        &project,
        ProjectFileLayout::Envelope,
        ValidationLimits::default(),
    )
    .unwrap();
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| matches!(diagnostic.owner, Some(ValidationOwner::Scope(_))))
        .unwrap();
    let NativeLocation::JsonPointer(path) = &diagnostic.native_location else {
        panic!("{diagnostic:?}")
    };
    assert!(path.starts_with("/document/root/children/0"));
    assert!(path.len() > 64);
    let limits = ValidationLimits {
        inspection: InspectionLimits {
            maximum_text_bytes: 64,
            ..InspectionLimits::default()
        },
        ..ValidationLimits::default()
    };
    let limited = validate(&project, ProjectFileLayout::Envelope, limits).unwrap();
    let fallback = limited
        .diagnostics
        .iter()
        .find(|issue| issue.owner == diagnostic.owner)
        .unwrap();
    assert_eq!(fallback.native_location, NativeLocation::Document);
}

#[test]
fn validation_checks_owner_identity_before_truncating_diagnostics() {
    let mut project = load(INVENTORY, MAXIMUM_BYTES).unwrap().into_project();
    project.extra_sources[0].name = "é".repeat(32);
    // Duplicate named endpoints are invalid and keep their declaration index.
    let duplicate = project.extra_sources[0].clone();
    project.extra_sources.push(duplicate);
    let mut limits = ValidationLimits {
        inspection: InspectionLimits {
            maximum_text_bytes: 64,
            ..InspectionLimits::default()
        },
        ..ValidationLimits::default()
    };
    let report = validate(&project, ProjectFileLayout::Bare, limits).unwrap();
    let admitted = report.diagnostics.iter().any(|issue| {
        issue.owner
            == Some(ValidationOwner::Endpoint(ValidationEndpoint::NamedSource {
                index: 1,
                name: "é".repeat(32),
            }))
    });
    assert!(admitted);
    limits.maximum_diagnostics = 0;
    project.extra_sources[1].name.push('x');
    project.extra_sources[0].name.push('x');
    assert_eq!(
        validate(&project, ProjectFileLayout::Bare, limits),
        Err(ValidationError::OwnerIdentityTooLong)
    );
}
