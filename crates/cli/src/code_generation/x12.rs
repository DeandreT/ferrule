use std::path::Path;

use anyhow::bail;
use codegen::{X12BoundaryOptions, X12BoundaryPolicy, X12EnvelopeProfile};
use ir::SchemaNode;
use mapping::{EdiBoundaryKind, FormatOptions, Project};

pub(super) fn policy_with_envelope_profiles(
    project: &Project,
    source_envelope: Option<X12EnvelopeProfile>,
    target_envelope: Option<X12EnvelopeProfile>,
) -> anyhow::Result<X12BoundaryPolicy> {
    Ok(X12BoundaryPolicy {
        source: selected_boundary(
            &project.source,
            project.source_path.as_deref(),
            &project.source_options,
            "source",
            source_envelope,
        )?,
        target: selected_boundary(
            &project.target,
            project.target_path.as_deref(),
            &project.target_options,
            "target",
            target_envelope,
        )?,
    })
}

fn selected_boundary(
    schema: &SchemaNode,
    path: Option<&str>,
    options: &FormatOptions,
    side: &str,
    envelope: Option<X12EnvelopeProfile>,
) -> anyhow::Result<Option<X12BoundaryOptions>> {
    let mut boundary = boundary(schema, path, options, side)?;
    if let Some(envelope) = envelope {
        let Some(selected) = boundary.as_mut() else {
            bail!("generated raw X12 {side} envelope selection requires an X12 endpoint");
        };
        selected.envelope_profile = envelope;
    }
    Ok(boundary)
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
    fn retained_00401_options_reach_the_selected_boundary_without_affecting_json() {
        let options = FormatOptions {
            edi_kind: Some(EdiBoundaryKind::X12),
            x12_interchange_version: Some("00401".into()),
            lenient_segments: true,
            edi_implied_decimals: vec![
                mapping::EdiImpliedDecimal::new(vec!["Value".into()], 2).unwrap(),
            ],
            edi_lexical_formats: vec![
                mapping::EdiLexicalFormat::new(
                    vec!["Value".into()],
                    mapping::EdiLexicalKind::Decimal { max_chars: 8 },
                )
                .unwrap(),
            ],
            edi_autocomplete: Some(mapping::EdiAutocomplete::X12(mapping::X12Autocomplete {
                request_acknowledgement: true,
                transaction_set: Some("940".into()),
            })),
            x12_separators: Some(mapping::X12Separators {
                element: '*',
                component: ':',
                segment: '~',
                repetition: Some('^'),
                release: None,
            }),
            ..Default::default()
        };
        let root = std::env::temp_dir().join(format!(
            "ferrule-x12-cli-profile-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("OPTIONS.original.txt"), format!("{options:#?}")).unwrap();
        std::fs::write(root.join("SCHEMA.original.txt"), format!("{:#?}", schema())).unwrap();
        let result = boundary(&schema(), Some("input.txt"), &options, "source");
        std::fs::write(root.join("OUTCOME.original.txt"), format!("{result:#?}")).unwrap();
        let captured = result.unwrap().unwrap();
        assert!(captured.lenient_segments);
        assert_eq!(captured.implied_decimals, options.edi_implied_decimals);
        assert_eq!(captured.lexical_formats, options.edi_lexical_formats);
        assert_eq!(captured.separators, options.x12_separators);
        assert!(captured.autocomplete.unwrap().request_acknowledgement);
        let json = FormatOptions {
            json_document: true,
            lenient_segments: true,
            ..Default::default()
        };
        std::fs::write(root.join("JSON-OPTIONS.original.txt"), format!("{json:#?}")).unwrap();
        let refused = boundary(&schema(), Some("input.json"), &json, "target");
        std::fs::write(
            root.join("JSON-OUTCOME.original.txt"),
            format!("{refused:#?}"),
        )
        .unwrap();
        assert!(refused.is_err());
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

    #[test]
    fn envelope_selection_is_explicit_and_never_selects_a_json_endpoint() {
        let root = std::env::temp_dir().join(format!(
            "ferrule-x12-cli-envelope-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let json = FormatOptions {
            json_document: true,
            ..Default::default()
        };
        std::fs::write(root.join("SCHEMA.original.txt"), format!("{:#?}", schema())).unwrap();
        std::fs::write(root.join("JSON-OPTIONS.original.txt"), format!("{json:#?}")).unwrap();
        for side in ["source", "target"] {
            for selection in [
                X12EnvelopeProfile::SingleTransaction,
                X12EnvelopeProfile::GroupedTransactions,
            ] {
                let json_result = selected_boundary(
                    &schema(),
                    Some("document.json"),
                    &json,
                    side,
                    Some(selection),
                );
                let x12_result = selected_boundary(
                    &schema(),
                    Some("document.x12"),
                    &FormatOptions::default(),
                    side,
                    Some(selection),
                );
                std::fs::write(
                    root.join(format!("{side}-{selection:?}-OUTCOMES.original.txt")),
                    format!("JSON={json_result:#?}\nX12={x12_result:#?}"),
                )
                .unwrap();
                assert_eq!(
                    json_result.unwrap_err().to_string(),
                    format!("generated raw X12 {side} envelope selection requires an X12 endpoint")
                );
                assert_eq!(x12_result.unwrap().unwrap().envelope_profile, selection);
            }
        }
        let result = selected_boundary(
            &schema(),
            Some("document.x12"),
            &FormatOptions::default(),
            "source",
            None,
        );
        std::fs::write(
            root.join("DEFAULT-OUTCOME.original.txt"),
            format!("{result:#?}"),
        )
        .unwrap();
        assert_eq!(
            result.unwrap().unwrap().envelope_profile,
            X12EnvelopeProfile::SingleTransaction
        );
    }

    #[test]
    fn grouped_facade_backend_refusal_precedes_project_and_output_loading() {
        let root = std::env::temp_dir().join(format!(
            "ferrule-grouped-x12-backend-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let output = root.join("absent-parent/unpublished");
        let target = crate::GenerateTarget::Rust {
            runtime_path: root.join("missing-runtime"),
        };
        std::fs::write(
            root.join("INPUT.original.txt"),
            format!("target={target:#?}\nsource=GroupedTransactions\ntarget_envelope=None\n"),
        )
        .unwrap();
        let result = crate::generate_project_with_x12_envelope_profiles(
            &root.join("missing-project.json"),
            &output,
            target,
            Some(X12EnvelopeProfile::GroupedTransactions),
            None,
        );
        std::fs::write(root.join("OUTCOME.original.txt"), format!("{result:#?}")).unwrap();
        assert_eq!(
            result.unwrap_err().to_string(),
            "generated raw X12 adapters currently require the C# backend"
        );
        assert!(!output.parent().unwrap().exists());
    }

    #[test]
    fn explicit_json_profile_refusal_preserves_existing_destination_before_lowering() {
        let project = include_str!(
            "../../tests/code_generation/static_document_adapters/fixtures/projects/base.json"
        );
        let root = std::env::temp_dir().join(format!(
            "ferrule-grouped-x12-json-refusal-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let input = root.join("project.json");
        let output = root.join("existing");
        std::fs::write(&input, project).unwrap();
        std::fs::create_dir(&output).unwrap();
        std::fs::write(output.join("sentinel.txt"), "authored unchanged sentinel\n").unwrap();
        for (source, side) in [(true, "source"), (false, "target")] {
            for selection in [
                X12EnvelopeProfile::SingleTransaction,
                X12EnvelopeProfile::GroupedTransactions,
            ] {
                let result = crate::generate_project_with_x12_envelope_profiles(
                    &input,
                    &output,
                    crate::GenerateTarget::CSharp,
                    source.then_some(selection),
                    (!source).then_some(selection),
                );
                std::fs::write(
                    root.join(format!("{side}-{selection:?}-OUTCOME.original.txt")),
                    format!("{result:#?}"),
                )
                .unwrap();
                let entries = std::fs::read_dir(&output)
                    .unwrap()
                    .map(|entry| entry.unwrap().file_name())
                    .collect::<Vec<_>>();
                let sentinel = std::fs::read_to_string(output.join("sentinel.txt")).unwrap();
                std::fs::write(
                    root.join(format!("{side}-{selection:?}-DESTINATION.original.txt")),
                    format!("entries={entries:?}\nsentinel={sentinel:?}"),
                )
                .unwrap();
                assert_eq!(
                    result.unwrap_err().to_string(),
                    format!("generated raw X12 {side} envelope selection requires an X12 endpoint")
                );
                assert_eq!(entries, vec![std::ffi::OsString::from("sentinel.txt")]);
                assert_eq!(sentinel, "authored unchanged sentinel\n");
            }
        }
    }
}
