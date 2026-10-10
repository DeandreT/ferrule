use super::*;

fn project() -> Project {
    serde_json::from_str(include_str!(
        "../../../tests/code_generation/static_document_adapters/fixtures/projects/base.json"
    ))
    .unwrap()
}

fn evidence(topic: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ferrule-static-documents-{topic}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn static_identity_is_explicit_and_never_derived_from_schema() {
    let root = evidence("identity");
    let mut outcomes = Vec::new();
    for (path, options, x12) in [
        (
            Some("input.txt"),
            FormatOptions {
                edi_kind: Some(EdiBoundaryKind::X12),
                ..Default::default()
            },
            true,
        ),
        (
            None,
            FormatOptions {
                edi_kind: Some(EdiBoundaryKind::X12),
                ..Default::default()
            },
            true,
        ),
        (
            Some("input.txt"),
            FormatOptions {
                json_document: true,
                ..Default::default()
            },
            false,
        ),
        (Some("input.X12"), FormatOptions::default(), true),
        (Some("input.json"), FormatOptions::default(), false),
    ] {
        let result = boundary(path, &options, "source");
        outcomes.push(format!("{path:?} {options:#?}: {result:#?}"));
        std::fs::write(root.join("OUTCOMES.original.txt"), outcomes.join("\n")).unwrap();
        assert_eq!(
            matches!(result.unwrap(), DocumentBoundaryOptions::X12(_)),
            x12
        );
    }
    for path in [
        None,
        Some("input"),
        Some("input.txt"),
        Some("input.json5"),
        Some("input.xml"),
        Some("input.csv"),
    ] {
        let result = boundary(path, &FormatOptions::default(), "source");
        outcomes.push(format!("{path:?}: {result:#?}"));
        std::fs::write(root.join("OUTCOMES.original.txt"), outcomes.join("\n")).unwrap();
        assert_eq!(
            result.unwrap_err().to_string(),
            "static document source requires an explicit X12 or strict JSON format identity"
        );
    }
    let conflicting = FormatOptions {
        json_document: true,
        edi_kind: Some(EdiBoundaryKind::X12),
        ..Default::default()
    };
    let result = boundary(Some("input.x12"), &conflicting, "source");
    std::fs::write(root.join("CONFLICT.original.txt"), format!("{result:#?}")).unwrap();
    assert!(matches!(
        result
            .unwrap_err()
            .downcast_ref::<codegen::X12BoundaryPolicyError>(),
        Some(codegen::X12BoundaryPolicyError::FormatOptions)
    ));
}

#[test]
fn static_policy_retains_all_declared_own_boundaries_in_order() {
    let candidate = project();
    let result = policy(&candidate);
    let root = evidence("policy");
    std::fs::write(
        root.join("PROJECT.original.json"),
        serde_json::to_vec_pretty(&candidate).unwrap(),
    )
    .unwrap();
    std::fs::write(root.join("OUTCOME.original.txt"), format!("{result:#?}")).unwrap();
    let policy = result.unwrap();
    assert!(matches!(policy.source, DocumentBoundaryOptions::Json));
    assert!(matches!(policy.target, DocumentBoundaryOptions::Json));
    assert_eq!(
        policy
            .extra_sources
            .iter()
            .map(|source| source.name.as_str())
            .collect::<Vec<_>>(),
        ["Supplement", "Reference", "Unused"]
    );
    assert!(matches!(
        policy.extra_sources[0].options,
        DocumentBoundaryOptions::Json
    ));
    assert!(matches!(
        policy.extra_sources[1].options,
        DocumentBoundaryOptions::X12(_)
    ));
    assert!(matches!(
        policy.extra_sources[2].options,
        DocumentBoundaryOptions::Json
    ));
    assert_eq!(
        policy
            .extra_targets
            .iter()
            .map(|target| target.name.as_str())
            .collect::<Vec<_>>(),
        ["Advice", "Audit"]
    );
    let DocumentBoundaryOptions::X12(ref options) = policy.extra_targets[0].options else {
        panic!("own X12 target was not retained")
    };
    assert_eq!(options.separators.as_ref().unwrap().element, '|');
    assert!(matches!(
        policy.extra_targets[1].options,
        DocumentBoundaryOptions::Json
    ));
}

#[test]
fn static_backend_refuses_before_project_load_or_destination_creation() {
    let root = evidence("backend");
    let output = root.join("absent-parent/output");
    let result = crate::generate_project_with_static_document_adapters(
        &root.join("absent-project.json"),
        &output,
        crate::GenerateTarget::Rust {
            runtime_path: root.join("absent-runtime"),
        },
    );
    std::fs::write(root.join("OUTCOME.original.txt"), format!("{result:#?}")).unwrap();
    assert_eq!(
        result.unwrap_err().to_string(),
        "generated static document adapters currently require the C# backend"
    );
    assert!(!output.parent().unwrap().exists());
}

#[test]
fn static_unselected_metadata_refusal_precedes_lower_and_preserves_destination() {
    let root = evidence("publication");
    let mut candidate = project();
    candidate.extra_sources[2].options.json_document = false;
    candidate.extra_sources[2].path = "unused.unknown".into();
    candidate.graph = Default::default();
    let input = root.join("project.json");
    std::fs::write(&input, serde_json::to_vec_pretty(&candidate).unwrap()).unwrap();
    let absent = root.join("absent-parent/output");
    let existing = root.join("existing");
    std::fs::create_dir(&existing).unwrap();
    let sentinel = existing.join("sentinel.bin");
    std::fs::write(&sentinel, b"authored unchanged sentinel\0").unwrap();
    let before = std::fs::metadata(&sentinel).unwrap().modified().unwrap();
    for (index, output) in [&absent, &existing].into_iter().enumerate() {
        let result = crate::generate_project_with_static_document_adapters(
            &input,
            output,
            crate::GenerateTarget::CSharp,
        );
        std::fs::write(
            root.join(format!("OUTCOME-{index}.original.txt")),
            format!("{result:#?}"),
        )
        .unwrap();
        assert_eq!(
            result.unwrap_err().to_string(),
            "static document named source \"Unused\" requires an explicit X12 or strict JSON format identity"
        );
    }
    assert!(!absent.parent().unwrap().exists());
    assert_eq!(
        std::fs::read(&sentinel).unwrap(),
        b"authored unchanged sentinel\0"
    );
    assert_eq!(
        std::fs::metadata(&sentinel).unwrap().modified().unwrap(),
        before
    );
    assert_eq!(std::fs::read_dir(&existing).unwrap().count(), 1);
}
