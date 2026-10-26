use std::path::Path;

use cli::{PayloadDocument, PayloadRunOptions, TargetSelection};
use ir::{ScalarType, SchemaNode};
use mapping::{
    EdiAutocomplete, EdiBoundaryKind, EdifactAutocomplete, FormatOptions, NamedSource, NamedTarget,
    Node, Scope, ScopeConstruction, X12Autocomplete,
};

#[path = "support/bounded_documents.rs"]
mod fixtures;

fn autocomplete(kind: EdiBoundaryKind) -> EdiAutocomplete {
    match kind {
        EdiBoundaryKind::X12 => EdiAutocomplete::X12(X12Autocomplete {
            request_acknowledgement: false,
            transaction_set: Some("850".into()),
        }),
        EdiBoundaryKind::Edifact => EdiAutocomplete::Edifact(EdifactAutocomplete {
            syntax_level: None,
            syntax_version: None,
            controlling_agency: None,
            message_type: None,
        }),
        _ => unreachable!("only the two admitted dialects are fixtures"),
    }
}

fn opposite(kind: EdiBoundaryKind) -> EdiBoundaryKind {
    match kind {
        EdiBoundaryKind::X12 => EdiBoundaryKind::Edifact,
        EdiBoundaryKind::Edifact => EdiBoundaryKind::X12,
        _ => unreachable!("only the two admitted dialects are fixtures"),
    }
}

fn options(kind: EdiBoundaryKind, completion: Option<EdiAutocomplete>) -> FormatOptions {
    FormatOptions {
        edi_kind: Some(kind),
        edi_autocomplete: completion,
        ..Default::default()
    }
}

fn fixture(kind: EdiBoundaryKind) -> (SchemaNode, String) {
    let (segments, wire): (&[(&str, usize)], String) = match kind {
        EdiBoundaryKind::X12 => (
            &[
                ("ISA", 16),
                ("GS", 8),
                ("ST", 2),
                ("SE", 2),
                ("GE", 2),
                ("IEA", 2),
            ],
            format!(
                "ISA*00*{:10}*00*{:10}*ZZ*{:<15}*ZZ*{:<15}*261026*1200*U*00401*000000001*0*T*:~\nGS*PO*SOURCE*DEST*20261026*120000*1*X*004010~\nST*850*0001~\nSE*2*0001~\nGE*1*1~\nIEA*1*000000001~\n",
                "", "", "SOURCE", "DEST"
            ),
        ),
        EdiBoundaryKind::Edifact => (
            &[("UNH", 1), ("BGM", 1), ("UNT", 2)],
            "UNH+1'\nBGM+42'\nUNT+3+1'\n".into(),
        ),
        _ => unreachable!("only the two admitted dialects are fixtures"),
    };
    let schema = SchemaNode::group(
        "Message",
        segments
            .iter()
            .map(|(tag, fields)| {
                SchemaNode::group(
                    *tag,
                    (1..=*fields)
                        .map(|field| SchemaNode::scalar(format!("E{field:02}"), ScalarType::String))
                        .collect(),
                )
            })
            .collect(),
    );
    (schema, wire)
}

fn assert_pair_refusal(error: &anyhow::Error) {
    assert!(
        format!("{error:#}").contains("`edi_autocomplete` to match `edi_kind`"),
        "{error:#}"
    );
    assert!(
        error.downcast_ref::<format_edi::EdiFormatError>().is_none(),
        "EDI parsing must not begin: {error:#}"
    );
    assert!(
        error.downcast_ref::<std::str::Utf8Error>().is_none(),
        "UTF-8 parsing must not begin: {error:#}"
    );
}

#[test]
fn bounded_edi_admits_matching_and_absent_completion_for_both_dialects() -> anyhow::Result<()> {
    for kind in [EdiBoundaryKind::X12, EdiBoundaryKind::Edifact] {
        let (schema, wire) = fixture(kind);
        for completion in [None, Some(autocomplete(kind))] {
            // Native EDIFACT completion retains the four empty message-header
            // components; absent completion does not create that composite.
            let expected = if kind == EdiBoundaryKind::Edifact && completion.is_some() {
                wire.replacen("UNH+1'", "UNH+1+:::'", 1)
            } else {
                wire.clone()
            };
            let project = fixtures::copy_project(schema.clone(), options(kind, completion));
            for selection in [None, Some(TargetSelection::Primary)] {
                let output = fixtures::run(&project, wire.as_bytes(), &[], selection)?;
                assert_eq!(output.artifacts.len(), 1);
                assert_eq!(output.artifacts[0].bytes, expected.as_bytes());
            }
        }
    }
    Ok(())
}

#[test]
fn mismatched_primary_sources_refuse_before_bad_bytes_and_allow_repaired_retry()
-> anyhow::Result<()> {
    for kind in [EdiBoundaryKind::X12, EdiBoundaryKind::Edifact] {
        let (schema, wire) = fixture(kind);
        let mut project = fixtures::copy_project(schema, options(kind, None));
        let expected = fixtures::run(&project, wire.as_bytes(), &[], None)?;
        project.source_options.edi_autocomplete = Some(autocomplete(opposite(kind)));
        for selection in [None, Some(TargetSelection::Primary)] {
            let error = fixtures::run(&project, b"\xffnot EDI", &[], selection).unwrap_err();
            assert_eq!(error.to_string(), "reading primary input payload");
            assert_pair_refusal(&error);
        }
        project.source_options.edi_autocomplete = Some(autocomplete(kind));
        assert_eq!(
            fixtures::run(&project, wire.as_bytes(), &[], None)?,
            expected
        );
        project.source_options.edi_autocomplete = None;
        assert_eq!(
            fixtures::run(&project, wire.as_bytes(), &[], None)?,
            expected
        );
    }
    Ok(())
}

#[test]
fn primary_and_named_target_pair_checks_follow_selected_output_scope() -> anyhow::Result<()> {
    for kind in [EdiBoundaryKind::X12, EdiBoundaryKind::Edifact] {
        let (schema, wire) = fixture(kind);
        let mut project = fixtures::copy_project(schema.clone(), options(kind, None));
        project.extra_targets.push(NamedTarget {
            name: "named".into(),
            path: Some("named.json5".into()),
            schema,
            options: options(kind, None),
            root: Scope {
                construction: ScopeConstruction::CopyCurrentSource,
                ..Default::default()
            },
        });
        let expected = fixtures::run(&project, wire.as_bytes(), &[], None)?;
        for bad_primary in [true, false] {
            if bad_primary {
                project.target_options.edi_autocomplete = Some(autocomplete(opposite(kind)));
            } else {
                project.extra_targets[0].options.edi_autocomplete =
                    Some(autocomplete(opposite(kind)));
            }
            let good_selection = if bad_primary {
                TargetSelection::Named("named")
            } else {
                TargetSelection::Primary
            };
            let bad_selection = if bad_primary {
                TargetSelection::Primary
            } else {
                TargetSelection::Named("named")
            };
            assert_eq!(
                fixtures::run(&project, wire.as_bytes(), &[], Some(good_selection))?.artifacts[0]
                    .bytes,
                wire.as_bytes()
            );
            for selection in [None, Some(bad_selection)] {
                assert_pair_refusal(
                    &fixtures::run(&project, wire.as_bytes(), &[], selection).unwrap_err(),
                );
            }
            project.target_options.edi_autocomplete = None;
            project.extra_targets[0].options.edi_autocomplete = None;
            assert_eq!(
                fixtures::run(&project, wire.as_bytes(), &[], None)?,
                expected
            );
        }
    }
    Ok(())
}

#[test]
fn named_source_admission_ignores_unselected_inputs_but_refuses_required_bad_pairs()
-> anyhow::Result<()> {
    for kind in [EdiBoundaryKind::X12, EdiBoundaryKind::Edifact] {
        let (schema, wire) = fixture(kind);
        let mut project = fixtures::project(fixtures::json(), fixtures::json(), false);
        project.extra_sources.push(NamedSource {
            name: "edi".into(),
            path: "saved.json5".into(),
            schema,
            options: options(kind, Some(autocomplete(opposite(kind)))),
            dynamic_path: None,
        });
        let tag = if kind == EdiBoundaryKind::X12 {
            "ISA"
        } else {
            "UNH"
        };
        project.graph.nodes.insert(
            1,
            Node::SourceField {
                frame: None,
                path: vec!["edi".into(), tag.into(), "E01".into()],
            },
        );
        project.extra_targets.push(NamedTarget {
            name: "named".into(),
            path: Some("named.json5".into()),
            schema: fixtures::schema(),
            options: fixtures::json(),
            root: fixtures::scope(None, 1),
        });
        assert!(fixtures::run(&project, b"{}", &[], Some(TargetSelection::Primary)).is_ok());
        assert!(
            fixtures::run(
                &project,
                b"{}",
                &[("edi", b"\xffbad")],
                Some(TargetSelection::Primary)
            )
            .is_ok()
        );
        for selection in [None, Some(TargetSelection::Named("named"))] {
            let error =
                fixtures::run(&project, b"{}", &[("edi", b"\xffbad")], selection).unwrap_err();
            assert_eq!(error.to_string(), "reading payload source `edi`");
            assert_pair_refusal(&error);
        }
        project.extra_sources[0].options.edi_autocomplete = Some(autocomplete(kind));
        let repaired = fixtures::run(&project, b"{}", &[("edi", wire.as_bytes())], None)?;
        project.extra_sources[0].options.edi_autocomplete = None;
        assert_eq!(
            fixtures::run(&project, b"{}", &[("edi", wire.as_bytes())], None)?,
            repaired
        );
    }
    Ok(())
}

#[test]
fn default_and_bounded_tabular_keep_mismatched_autocomplete_compatibility() -> anyhow::Result<()> {
    for kind in [EdiBoundaryKind::X12, EdiBoundaryKind::Edifact] {
        let (schema, wire) = fixture(kind);
        let mut project = fixtures::copy_project(schema, options(kind, None));
        let input = PayloadDocument::new(Path::new("uploaded.edi"), wire.as_bytes())?;
        let run = |project: &mapping::Project, bounded_tabular: bool| {
            let options = PayloadRunOptions::new(input).with_output_path(Path::new("output.edi"));
            let options = if bounded_tabular {
                options.with_bounded_tabular()
            } else {
                options
            };
            cli::run_project_value_payloads(project, Path::new("project.json"), &options)
        };
        for bounded_tabular in [false, true] {
            let expected = run(&project, bounded_tabular)?;
            project.source_options.edi_autocomplete = Some(autocomplete(opposite(kind)));
            project.target_options.edi_autocomplete = Some(autocomplete(opposite(kind)));
            assert_eq!(run(&project, bounded_tabular)?, expected);
            project.source_options.edi_autocomplete = None;
            project.target_options.edi_autocomplete = None;
        }
    }
    Ok(())
}
