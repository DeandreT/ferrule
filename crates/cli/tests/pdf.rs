use std::collections::BTreeMap;
use std::path::Path;

use ir::{ScalarType, SchemaNode};
use mapping::{
    Binding, FixedFieldWidth, FixedWidthLayout, FlexCommand, FlexLineEnding, FlexTextLayout,
    FormatOptions, Graph, HttpGetOptions, Node, PdfCapture, PdfCommand, PdfLayout,
    PdfPageSelection, PdfRegion, Project, ProtobufOptions, Scope,
};

fn layout() -> PdfLayout {
    PdfLayout::new(
        "Document",
        PdfPageSelection::First,
        vec![PdfCommand::Capture(PdfCapture {
            name: "Value".into(),
            region: PdfRegion::full(),
        })],
    )
    .unwrap()
}

fn configured_project() -> Project {
    let layout = layout();
    Project {
        source: layout.schema(),
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        source_path: Some("missing-stored-input.pdf".into()),
        target_path: None,
        source_options: FormatOptions {
            pdf: Some(layout),
            ..FormatOptions::default()
        },
        target_options: FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::SourceField {
                    path: vec!["Value".into()],
                    frame: None,
                },
            )]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            ..Scope::default()
        },
    }
}

fn fixture_project() -> Project {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    serde_json::from_slice(&std::fs::read(fixture_dir.join("project.json")).unwrap()).unwrap()
}

fn paths(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let tag = format!("pdf_{name}_{}", std::process::id());
    let temporary = std::env::temp_dir();
    (
        temporary.join(format!("ferrule_cli_{tag}.json")),
        temporary.join(format!("ferrule_cli_{tag}.pdf")),
    )
}

fn pdf_bytes(text: &str) -> Vec<u8> {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)");
    let content = format!("BT /F1 12 Tf 72 720 Td ({escaped}) Tj ET\n");
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>\n".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>\n".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>\n".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\n".to_vec(),
        format!(
            "<< /Length {} >>\nstream\n{}endstream\n",
            content.len(),
            content
        )
        .into_bytes(),
    ];
    let mut bytes = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(object);
        bytes.extend_from_slice(b"endobj\n");
    }
    let xref = bytes.len();
    bytes.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    bytes.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    bytes
}

#[test]
fn configured_pdf_input_overrides_extension_and_stored_path() {
    let tag = format!("pdf_override_{}", std::process::id());
    let temporary = std::env::temp_dir();
    let project_path = temporary.join(format!("ferrule_cli_{tag}.json"));
    let input_path = temporary.join(format!("ferrule_cli_{tag}.data"));
    let output_path = temporary.join(format!("ferrule_cli_{tag}.xml"));
    std::fs::write(
        &project_path,
        serde_json::to_vec(&configured_project()).unwrap(),
    )
    .unwrap();
    std::fs::write(&input_path, pdf_bytes("Ferrule PDF input")).unwrap();

    let outcome =
        cli::run_project_with_paths(&project_path, Some(&input_path), Some(&output_path)).unwrap();
    assert_eq!(outcome.input_path, input_path);
    assert_eq!(outcome.records_written, 1);
    let output = std::fs::read_to_string(&output_path).unwrap();
    assert!(
        output.contains("<Value>Ferrule PDF input</Value>"),
        "{output}"
    );

    for path in [project_path, input_path, output_path] {
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn pdf_options_reject_other_format_options_before_reading() {
    let (project_path, input_path) = paths("conflict");
    let conflicts = vec![
        (
            "CSV",
            FormatOptions {
                delimiter: Some(';'),
                ..FormatOptions::default()
            },
        ),
        (
            "EDI",
            FormatOptions {
                lenient_segments: true,
                ..FormatOptions::default()
            },
        ),
        (
            "fixed-width",
            FormatOptions {
                fixed_width: Some(
                    FixedWidthLayout::new(vec![FixedFieldWidth::new(1).unwrap()], ' ', true, false)
                        .unwrap(),
                ),
                ..FormatOptions::default()
            },
        ),
        (
            "FlexText",
            FormatOptions {
                flextext: Some(
                    FlexTextLayout::new(
                        "Document",
                        FlexCommand::store("Value", ScalarType::String, None),
                        FlexLineEnding::Lf,
                        false,
                    )
                    .unwrap(),
                ),
                ..FormatOptions::default()
            },
        ),
        (
            "HTTP",
            FormatOptions {
                http_get: Some(HttpGetOptions::default()),
                ..FormatOptions::default()
            },
        ),
        (
            "JSON Lines",
            FormatOptions {
                json_lines: true,
                ..FormatOptions::default()
            },
        ),
        (
            "Protocol Buffers",
            FormatOptions {
                protobuf: Some(ProtobufOptions {
                    schema: "message Document { optional string Value = 1; }".into(),
                    root_message: "Document".into(),
                }),
                ..FormatOptions::default()
            },
        ),
        (
            "XLSX",
            FormatOptions {
                xlsx_sheet: Some("Sheet1".into()),
                ..FormatOptions::default()
            },
        ),
    ];

    for (format, mut options) in conflicts {
        options.pdf = Some(layout());
        let mut project = configured_project();
        project.source_options = options;
        std::fs::write(&project_path, serde_json::to_vec(&project).unwrap()).unwrap();

        let error = cli::run_project(
            &project_path,
            &input_path,
            &std::env::temp_dir().join("ferrule_cli_pdf_unused.csv"),
        )
        .unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("`pdf` cannot be combined"),
            "{format}: {message}"
        );
    }

    std::fs::remove_file(project_path).unwrap();
}

#[test]
fn pdf_input_without_embedded_options_is_rejected_explicitly() {
    let (project_path, input_path) = paths("missing_layout");
    std::fs::write(
        &project_path,
        serde_json::to_vec(&fixture_project()).unwrap(),
    )
    .unwrap();

    let error = cli::run_project(
        &project_path,
        &input_path,
        &std::env::temp_dir().join("ferrule_cli_pdf_unused.csv"),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("PDF input requires embedded `pdf` extraction options")
    );

    std::fs::remove_file(project_path).unwrap();
}

#[test]
fn pdf_output_is_rejected_before_creating_the_file() {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let (project_path, output_path) = paths("target");
    std::fs::write(
        &project_path,
        serde_json::to_vec(&fixture_project()).unwrap(),
    )
    .unwrap();

    let error =
        cli::run_project(&project_path, &fixture_dir.join("input.csv"), &output_path).unwrap_err();
    assert!(error.to_string().contains("PDF output is not supported"));
    assert!(!output_path.exists());

    std::fs::remove_file(project_path).unwrap();
}

#[test]
fn configured_pdf_target_is_rejected_before_creating_the_file() {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let (project_path, output_path) = paths("configured_target");
    let output_path = output_path.with_extension("data");
    let mut project = fixture_project();
    project.target_options.pdf = Some(layout());
    std::fs::write(&project_path, serde_json::to_vec(&project).unwrap()).unwrap();

    let error =
        cli::run_project(&project_path, &fixture_dir.join("input.csv"), &output_path).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("PDF extraction is valid only for mapping sources")
    );
    assert!(!output_path.exists());

    std::fs::remove_file(project_path).unwrap();
}
