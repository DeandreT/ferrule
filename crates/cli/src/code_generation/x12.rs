use std::path::Path;

use anyhow::bail;
use codegen::{X12BoundaryOptions, X12BoundaryPolicy};
use ir::SchemaNode;
use mapping::{EdiBoundaryKind, FormatOptions, Project};

pub(super) fn policy(project: &Project) -> anyhow::Result<X12BoundaryPolicy> {
    Ok(X12BoundaryPolicy {
        source: boundary(
            &project.source,
            project.source_path.as_deref(),
            &project.source_options,
            "source",
        )?,
        target: boundary(
            &project.target,
            project.target_path.as_deref(),
            &project.target_options,
            "target",
        )?,
    })
}

fn boundary(
    schema: &SchemaNode,
    path: Option<&str>,
    options: &FormatOptions,
    side: &str,
) -> anyhow::Result<Option<X12BoundaryOptions>> {
    let declared_x12 = options.edi_kind == Some(EdiBoundaryKind::X12);
    if options.edi_kind.is_some() && !declared_x12 {
        bail!("generated raw X12 {side} does not support the declared EDI family");
    }
    let extension = path
        .and_then(|path| Path::new(path).extension())
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    let x12 = if declared_x12 {
        // The retained component family is authoritative. Imported EDI
        // components commonly retain a generic .txt instance identity.
        X12BoundaryOptions::from_format_options(options)?;
        true
    } else if options.json_document {
        false
    } else if let Some(extension) = extension.as_deref() {
        match extension {
            "edi" | "x12" => true,
            "json" => false,
            "json5" | "jsonl" | "ndjson" | "xml" | "csv" | "txt" | "xlsx" | "db" | "sqlite"
            | "sqlite3" | "edifact" | "hl7" | "idoc" | "fin" | "swift" | "pdf" | "xbrl" => {
                bail!("generated raw X12 {side} does not support stored .{extension} identity");
            }
            _ => {
                bail!("generated raw X12 {side} requires an explicit X12 or JSON format identity");
            }
        }
    } else {
        declared_x12 || format_edi::dialect_of(schema).ok() == Some(format_edi::Dialect::X12)
    };
    if x12 {
        X12BoundaryOptions::from_format_options(options)
            .map(Some)
            .map_err(Into::into)
    } else {
        codegen::validate_x12_json_format_options(options)?;
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::ScalarType;

    fn schema() -> SchemaNode {
        SchemaNode::group(
            "Interchange",
            vec![SchemaNode::group(
                "ISA",
                vec![SchemaNode::scalar("ISA01", ScalarType::String)],
            )],
        )
    }

    #[test]
    fn explicit_x12_identity_keeps_generic_imported_file_names() {
        let options = FormatOptions {
            edi_kind: Some(EdiBoundaryKind::X12),
            ..Default::default()
        };
        for path in ["input.txt", "input.dat", "input", "input.json"] {
            assert!(
                boundary(&schema(), Some(path), &options, "source")
                    .unwrap()
                    .is_some()
            );
        }
    }

    #[test]
    fn stored_physical_paths_win_over_schema_shape_without_edi_identity() {
        for path in ["input.edi", "input.X12"] {
            assert!(
                boundary(&schema(), Some(path), &FormatOptions::default(), "source")
                    .unwrap()
                    .is_some()
            );
        }
        assert!(
            boundary(
                &schema(),
                Some("input.json"),
                &FormatOptions::default(),
                "source"
            )
            .unwrap()
            .is_none()
        );
        for path in [
            "input.txt",
            "input.xml",
            "input.json5",
            "input.jsonl",
            "input.dat",
        ] {
            assert!(boundary(&schema(), Some(path), &FormatOptions::default(), "source").is_err());
        }
        assert!(
            boundary(&schema(), None, &FormatOptions::default(), "source")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn selected_json_identity_keeps_strict_semantics_and_rejects_other_metadata() {
        let options = FormatOptions {
            json_document: true,
            ..Default::default()
        };
        assert!(
            boundary(&schema(), Some("input.txt"), &options, "source")
                .unwrap()
                .is_none()
        );
        for options in [
            FormatOptions {
                json_document: true,
                edi_kind: Some(EdiBoundaryKind::X12),
                ..Default::default()
            },
            FormatOptions {
                json_document: true,
                json5: true,
                ..Default::default()
            },
            FormatOptions {
                json_document: true,
                lenient_segments: true,
                ..Default::default()
            },
            FormatOptions {
                json_document: true,
                x12_interchange_version: Some("00401".into()),
                ..Default::default()
            },
        ] {
            let error = boundary(&schema(), Some("input.json"), &options, "source").unwrap_err();
            assert!(matches!(
                error.downcast_ref::<codegen::X12BoundaryPolicyError>(),
                Some(codegen::X12BoundaryPolicyError::FormatOptions)
            ));
        }
    }

    #[test]
    fn unsupported_backend_refuses_before_loading_or_publishing_a_project() {
        let root = std::env::temp_dir().join(format!("ferrule-x12-backend-{}", std::process::id()));
        let output = root.join("unpublished");
        let error = crate::generate_project_with_x12_adapters(
            &root.join("missing-project.json"),
            &output,
            crate::GenerateTarget::Rust {
                runtime_path: root.join("missing-runtime"),
            },
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "generated raw X12 adapters currently require the C# backend"
        );
        assert!(!output.exists());
    }
}
