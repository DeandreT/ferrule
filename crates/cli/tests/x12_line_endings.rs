//! File and bounded-payload dispatch retain an explicitly selected X12 policy.
use mapping::{EdiBoundaryKind, X12SegmentLineEnding as Ending};
#[path = "../../codegen/tests/fixtures/x12_line_endings.rs"]
mod fixture;
#[path = "support/bounded_documents.rs"]
mod payload;

fn project() -> mapping::Project {
    let mut project = fixture::project(codegen::X12EnvelopeProfile::SingleTransaction);
    project.source_options.edi_kind = Some(EdiBoundaryKind::X12);
    project.target_options.edi_kind = Some(EdiBoundaryKind::X12);
    project.target_options.x12_segment_line_ending = Ending::CrLf;
    project.source_path = Some("input.x12".into());
    project.target_path = Some("output.x12".into());
    project
}

#[test]
fn selected_line_endings_reach_file_payload_and_named_targets() -> anyhow::Result<()> {
    let root = test_support::TempDir::new("native-x12-line-endings")?;
    let mut project = project();
    let original = serde_json::to_string(&project)?;
    std::fs::write(
        root.join("project.json"),
        mapping::project_file::encode_pretty(&project)?,
    )?;
    std::fs::write(root.join("input.x12"), fixture::expected("\n"))?;
    cli::run_project(
        &root.join("project.json"),
        &root.join("input.x12"),
        &root.join("output.x12"),
    )?;
    assert_eq!(
        std::fs::read(root.join("output.x12"))?,
        fixture::expected("\r\n").as_bytes()
    );
    let input = fixture::expected("\n");
    let bytes = input.as_bytes().to_vec();
    let before = bytes.clone();
    let output = payload::run(&project, &bytes, &[], None)?;
    assert_eq!(
        output.artifacts[0].bytes,
        fixture::expected("\r\n").as_bytes()
    );
    assert_eq!(serde_json::to_string(&project)?, original);
    project.extra_targets.push(mapping::NamedTarget {
        name: "Copy".into(),
        path: Some("copy.x12".into()),
        schema: project.target.clone(),
        options: project.target_options.clone(),
        root: project.root.clone(),
    });
    let output = payload::run(
        &project,
        &bytes,
        &[],
        Some(cli::TargetSelection::Named("Copy")),
    )?;
    assert_eq!(output.artifacts.len(), 1);
    assert_eq!(
        output.artifacts[0].bytes,
        fixture::expected("\r\n").as_bytes()
    );
    assert_eq!(bytes, before);
    Ok(())
}

#[test]
fn source_other_dialect_ordinary_generation_and_newline_conflicts_fail_closed() -> anyhow::Result<()>
{
    let root = test_support::TempDir::new("x12-policy-refusals")?;
    for mode in 0..4 {
        let mut project = project();
        match mode {
            0 => project.source_options.x12_segment_line_ending = Ending::CrLf,
            1 => project.target_options.edi_kind = Some(EdiBoundaryKind::Edifact),
            _ => {
                project.target_options.x12_separators = Some(mapping::X12Separators {
                    element: '*',
                    component: ':',
                    segment: if mode == 2 { '\n' } else { '\r' },
                    repetition: None,
                    release: None,
                })
            }
        }
        let before = serde_json::to_string(&project)?;
        assert!(payload::run(&project, fixture::expected("\n").as_bytes(), &[], None).is_err());
        assert_eq!(serde_json::to_string(&project)?, before);
    }
    let project = project();
    std::fs::write(
        root.join("project.json"),
        mapping::project_file::encode_pretty(&project)?,
    )?;
    let destination = root.join("generated");
    assert!(
        cli::generate_project(
            &root.join("project.json"),
            &destination,
            cli::GenerateTarget::CSharp
        )
        .is_err()
    );
    assert!(!destination.exists());
    assert!(mfd::export(&project, &root.join("toy.mfd")).is_err());
    assert!(!root.join("toy.mfd").exists());
    assert!(
        format_edi::document::validate(
            &project.target,
            &project.target_options,
            EdiBoundaryKind::X12
        )
        .is_err()
    );
    Ok(())
}
