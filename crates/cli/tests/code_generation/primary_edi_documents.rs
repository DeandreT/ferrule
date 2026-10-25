use super::primary_documents_harness::{self as harness, Cohort};
use super::primary_xml_documents::projects as xml_projects;
use super::*;

#[path = "../../../../conformance/primary-edi-documents/projects.rs"]
mod projects;

fn emit(
    program: &codegen::DocumentProgram,
    language: &str,
    options: &codegen_rust::Options,
) -> TestResult<codegen::ArtifactSet> {
    Ok(if language == "rust" {
        codegen_rust::emit_with_primary_edi_documents(program, options)?
    } else {
        codegen_csharp::emit_with_primary_edi_documents(program)?
    })
}
static RUST_HOST: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    let host = include_str!("fixtures/primary_xml_documents_rust.rs.txt")
        .replace("let mut results = Vec::new();", "check_strict_envelopes()?;\n    let mut results = Vec::new();")
        .replace("PrimaryXmlDocumentError", "PrimaryEdiDocumentError")
        .replace("let mut results = Vec::new();", "let mut budget = codegen_runtime::EdiInputBudget::default();\n    assert!(matches!(budget.charge(64*1024*1024+1), Err(codegen_runtime::EdiBoundaryError::InputTooLarge { maximum: 67108864, .. })));\n    for _ in 0..4 { budget.charge(64*1024*1024)?; }\n    assert!(matches!(budget.charge(1), Err(codegen_runtime::EdiBoundaryError::InputTooLarge { maximum: 268435456, .. })));\n    budget.charge(0)?;\n    let mut results = Vec::new();")
        .replace("execute_primary_xml_document_bytes", "execute_primary_edi_document_bytes")
        .replace("            3 => run!(model3, &execution, &host, maximum),", "            3 => run!(model3, &execution, &host, maximum),\n            17 => run!(model17, &execution, &host, maximum),\n            18 => run!(model18, &execution, &host, maximum),\n            19 => run!(model19, &execution, &host, maximum),")
        .replace("            2 => run!(model2, &execution, &host, maximum),", "            2 => run!(model2, &execution, &host, maximum),\n            15 => run!(model15, &execution, &host, maximum),\n            16 => run!(model16, &execution, &host, maximum),")
        .replace("            1 => run!(model1, &execution, &host, maximum),", "            1 => run!(model1, &execution, &host, maximum),\n            14 => run!(model14, &execution, &host, maximum),")
        .replace("PrimaryEdiDocumentError::Output", "PrimaryEdiDocumentError::Boundary")
        .replace("None => (\"output\", (\"other\", error.to_string())),", "None => match &error { codegen_runtime::EdiBoundaryError::Format(codegen_runtime::EdiFormatError::IdocFieldTooWide { field, .. }) => (\"output\", (\"field_width\", field.clone())), codegen_runtime::EdiBoundaryError::Format(codegen_runtime::EdiFormatError::Validation(_)) => (\"output\", (\"edi\", \"constraint\".into())), codegen_runtime::EdiBoundaryError::Format(codegen_runtime::EdiFormatError::UnsupportedSchema(_)) => (\"output\", (\"edi\", \"schema\".into())), _ => (\"output\", (\"other\", error.to_string())) },")
        .replace("\"xml\"", "\"edi\"")
        .replace(".with_parameters(&parameters)", ".with_parameters(&parameters).with_current_datetime(\"2026-10-25T12:00:00Z\")")
        .replace("            0 => run!(model0, &execution, &host, maximum),", "            0 => run!(model0, &execution, &host, maximum),\n            12 => run!(model12, &execution, &host, maximum),\n            13 => run!(model13, &execution, &host, maximum),")
        .replace("            4 => run!(model4, &execution, &host, maximum),", "            4 => run!(model4, &execution, &host, maximum),\n            10 => run!(model10, &execution, &host, maximum),\n            11 => run!(model11, &execution, &host, maximum),")
        .replace("            other => return Err(format!(\"unknown model {other}\").into()),", "            5 => run!(model5, &execution, &host, maximum),\n            6 => run!(model6, &execution, &host, maximum),\n            7 => run!(model7, &execution, &host, maximum),\n            8 => run!(model8, &execution, &host, maximum),\n            9 => run!(model9, &execution, &host, maximum),\n            other => return Err(format!(\"unknown model {other}\").into()),");
    host + &include_str!("fixtures/edi_strict_rust.rs.txt").replace(
        "__STRICT_CASES__",
        &format!(
            "{:?}",
            include_str!("../../../../conformance/primary-edi-documents/strict-envelopes.json")
        ),
    )
});
static CSHARP_HOST: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    include_str!("fixtures/primary_xml_documents_csharp.cs.txt")
        .replace("var cases = JsonNode.Parse", "CheckStrictEnvelopes();\nvar cases = JsonNode.Parse")
        .replace("var results = new JsonArray();", "var budget = new FerruleEdiInputBudget();\ntry { budget.Charge(64*1024*1024+1); throw new Exception(\"document limit\"); } catch (FerruleEdiException error) when (error.Maximum == 67108864) { }\nfor (int at=0; at<4; at++) budget.Charge(64*1024*1024);\ntry { budget.Charge(1); throw new Exception(\"run limit\"); } catch (FerruleEdiException error) when (error.Maximum == 268435456) { }\nbudget.Charge(0);\nvar results = new JsonArray();")
        .replace(
            "ExecutePrimaryXmlDocumentBytes",
            "ExecutePrimaryEdiDocumentBytes",
        )
        .replace(
            "FerruleXmlOutputSetException",
            "FerruleEdiException or FerruleRuntimeException",
        )
        .replace("FerruleExecutionContext.WithParameters(\"authored.mapping\", new FerruleRuntimeParameters(parameters))", "new FerruleExecutionContext(\"authored.mapping\", \"authored.mapping\", \"2026-10-25T12:00:00Z\", new FerruleRuntimeParameters(parameters))")
        .replace("return MappingCause(error) is", "if (error is FerruleEdiException { Code: FerruleEdiError.Constraint }) return [\"output\", \"edi\", \"constraint\"];\n    return MappingCause(error) is")
        .replace("return MappingCause(error) is", "if (error is FerruleEdiException { Code: FerruleEdiError.FieldWidth } width) return [\"output\", \"field_width\", width.Field];\n    return MappingCause(error) is")
        .replace("return MappingCause(error) is", "if (error is FerruleEdiException { Code: FerruleEdiError.Schema }) return [\"output\", \"edi\", \"schema\"];\n    return MappingCause(error) is")
        .replace("\"xml\"", "\"edi\"")
        .replace("sealed class Recording", &(include_str!("fixtures/edi_strict_csharp.cs.txt").replace("__STRICT_CASES__", &serde_json::to_string(include_str!("../../../../conformance/primary-edi-documents/strict-envelopes.json")).unwrap()) + "\nsealed class Recording"))
});
static COHORT: std::sync::LazyLock<Cohort> = std::sync::LazyLock::new(|| Cohort {
    tag: "primary-edi-documents",
    cases: include_str!("../../../../conformance/primary-edi-documents/cases.json"),
    models: &projects::MODELS,
    document: projects::document,
    emit,
    rust_fixture: &RUST_HOST,
    csharp_fixture: &CSHARP_HOST,
    configured_model: 1,
});
#[test]
fn primary_edi_inventory() -> TestResult<()> {
    harness::check_inventory(&COHORT)
}

#[test]
fn primary_edi_strict_envelope_cases() -> TestResult<()> {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../conformance/primary-edi-documents/strict-envelopes.json"
    ))?;
    for case in cases.as_array().unwrap() {
        let wire = case["wire"].as_str().unwrap();
        match format_edi::edifact::validate_envelope(wire) {
            Ok(()) => assert_eq!(case["valid"], true, "{wire}"),
            Err(format_edi::EdiFormatError::EdifactEnvelope { segment, .. }) => {
                assert_eq!(case["valid"], false, "{wire}");
                assert_eq!(case["segment"].as_u64(), Some(segment as u64), "{wire}");
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
#[test]
fn primary_edi_interpreter() -> TestResult<()> {
    harness::check_interpreter(&COHORT)
}
#[test]
fn primary_edi_emission() -> TestResult<()> {
    harness::check_emission(&COHORT)
}

#[test]
fn primary_edi_manifests_and_descriptor_limits() -> TestResult<()> {
    let document = projects::document("edifact_direct");
    let program = codegen::lower_document(&document)?;
    let rust = harness::rust_options(&COHORT, 0)?;
    let csharp = codegen_csharp::Options {
        manifest: codegen::ManifestSelection::Included,
        ..Default::default()
    };
    for artifacts in [
        codegen_rust::Generator::new(&rust)
            .with_manifest(codegen::ManifestSelection::Included)
            .emit_with_primary_edi_documents(&program)?,
        codegen_csharp::Generator::new(&csharp).emit_with_primary_edi_documents(&program)?,
    ] {
        let manifest = artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "ferrule-manifest.json")
            .unwrap();
        assert_eq!(
            codegen::CompatibilityManifest::from_json(std::str::from_utf8(&manifest.contents)?)?
                .profile,
            codegen::GenerationProfile::PrimaryEdiDocument
        );
    }
    let mut project = document.project().clone();
    project.target_options.edi_value_constraints.push(
        mapping::EdiValueConstraint::new(
            vec!["DTL".into(), "Text".into()],
            0,
            2_000_000,
            vec!["a".repeat(1024 * 1024)],
        )
        .unwrap(),
    );
    let program = codegen::lower_document(&mapping::DocumentProject::new(
        project,
        document.primary_source().clone(),
    ))?;
    assert!(matches!(
        codegen::validate_primary_edi_documents(&program),
        Err(codegen::PrimaryDocumentBoundaryError::EdiDescriptor { .. })
    ));
    Ok(())
}
#[test]
fn primary_edi_cli_generation() -> TestResult<()> {
    harness::check_cli_generation(&COHORT)
}
#[test]
#[ignore = "builds generated Rust and C# hosts with cargo and .NET 10"]
fn primary_edi_generated_hosts() -> TestResult<()> {
    harness::check_generated_hosts(&COHORT)
}

#[test]
fn primary_edi_invalid_plans_never_call_host() -> TestResult<()> {
    for model in projects::MODELS {
        let base = projects::document(model);
        for variant in 0..6 {
            let mut project = base.project().clone();
            match variant {
                0 => {
                    project.source_options.edi_kind = None;
                    project.source_options.idoc = None;
                }
                1 => project.target_options.edi_kind = Some(mapping::EdiBoundaryKind::SwiftMt),
                2 => project.source_options.json_document = true,
                3 => project.source = SchemaNode::group("Bad", vec![]),
                4 => project.target_options.edi_value_constraints.push(
                    mapping::EdiValueConstraint::new(vec!["missing".into()], 0, 3, vec![]).unwrap(),
                ),
                _ => project.target_options.edi_lexical_formats.push(
                    mapping::EdiLexicalFormat::new(
                        vec!["DTL".into(), "Count".into()],
                        mapping::EdiLexicalKind::CompactDate8,
                    )
                    .unwrap(),
                ),
            }
            let document = mapping::DocumentProject::new(project, base.primary_source().clone());
            let temp = TempDir::new("edi-invalid-plan")?;
            let host = harness::Recording {
                inner: format_csv::LocalPrimaryFiles::new(&temp.0)?,
                calls: Default::default(),
            };
            assert!(
                cli::execute_primary_document_with_host(
                    &document,
                    Path::new("mapping.json"),
                    &host,
                    10,
                    None
                )
                .is_err()
            );
            assert!(host.calls.borrow().is_empty(), "{model} variant {variant}");
            assert!(
                codegen::lower_document(&document).is_err() || document.primary_route().is_err()
            );
        }
    }
    Ok(())
}

#[test]
fn primary_edi_saved_extensions_and_layout_preflight() -> TestResult<()> {
    for (model, extension) in [
        ("x12_direct", "x12"),
        ("edifact_direct", "edifact"),
        ("hl7_direct", "hl7"),
        ("tradacoms_direct", "tradacoms"),
        ("idoc_direct", "idoc"),
    ] {
        let base = projects::document(model);
        let mut project = base.project().clone();
        project.source_options.edi_kind = None;
        project.target_options.edi_kind = None;
        project.source_path = Some(format!("identity.{extension}"));
        project.target_path = Some(format!("identity.{extension}"));
        let document = mapping::DocumentProject::new(project, base.primary_source().clone());
        assert_eq!(document.primary_route(), base.primary_route());
        let program = codegen::lower_document(&document)?;
        assert!(codegen_csharp::emit_with_primary_edi_documents(&program).is_ok());
    }
    for variant in 0..6 {
        let base = projects::document(if matches!(variant, 0 | 3 | 4) {
            "x12_direct"
        } else {
            "idoc_direct"
        });
        let mut project = base.project().clone();
        if variant == 5 {
            let fields = project.source_options.idoc.as_ref().unwrap().segments()[0]
                .fields()
                .to_vec();
            project.source_options.idoc = Some(mapping::IdocLayout::new(vec![
                mapping::IdocSegmentLayout::new("row", fields)?,
            ])?);
            let ir::SchemaKind::Group { children, .. } = &mut project.source.kind else {
                unreachable!()
            };
            children[0].name = "row".into();
            assert!(
                format_edi::document::validate_input(
                    &project.source,
                    &project.source_options,
                    mapping::EdiBoundaryKind::Idoc
                )
                .unwrap_err()
                .to_string()
                .contains("native segment grammar")
            );
        } else if variant == 4 {
            project.source_options.x12_interchange_version = Some("00401".into());
            let ir::SchemaKind::Group { children, .. } = &mut project.source.kind else {
                unreachable!()
            };
            let ir::SchemaKind::Group { children, .. } = &mut children[0].kind else {
                unreachable!()
            };
            children[11].fixed = Some("00501".into());
        } else if variant == 3 {
            project.target_options.x12_interchange_version = Some("bad".into());
        } else if variant == 0 {
            project.source_options.x12_separators = Some(mapping::X12Separators {
                element: '*',
                component: '*',
                segment: '~',
                repetition: None,
                release: None,
            });
        } else {
            let layout = project.source_options.idoc.as_ref().unwrap();
            let mut fields = layout.segments()[0].fields().to_vec();
            if variant == 1 {
                fields.swap(0, 1);
            } else {
                fields[0] = mapping::IdocFieldLayout::new(
                    "Key",
                    std::num::NonZeroU32::new(2).unwrap(),
                    std::num::NonZeroU32::new(3).unwrap(),
                )?;
            }
            project.source_options.idoc = Some(mapping::IdocLayout::new(vec![
                mapping::IdocSegmentLayout::new("DTL", fields)?,
            ])?);
        }
        let document = mapping::DocumentProject::new(project, base.primary_source().clone());
        let directory = TempDir::new("edi-layout-preflight")?;
        let host = harness::Recording {
            inner: format_csv::LocalPrimaryFiles::new(&directory.0)?,
            calls: Default::default(),
        };
        assert!(
            cli::execute_primary_document_with_host(
                &document,
                Path::new("mapping.json"),
                &host,
                10,
                None
            )
            .is_err()
        );
        assert!(host.calls.borrow().is_empty());
        assert!(codegen::lower_document(&document).is_err());
    }
    Ok(())
}
