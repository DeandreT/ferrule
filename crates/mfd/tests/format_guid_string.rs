use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use ir::{Instance, Value};
use mapping::Node;

#[test]
fn standard_guid_formatter_imports_executes_and_round_trips()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/format_guid_string");
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let evidence = std::env::temp_dir().join(format!(
        "ferrule-guid269-mfd-{}-{stamp}",
        std::process::id()
    ));
    std::fs::create_dir(&evidence)?;
    eprintln!("GUID269_MFD_ORIGINALS={}", evidence.display());
    for file in ["mapping.mfd", "source.xsd", "target.xsd"] {
        std::fs::copy(fixture.join(file), evidence.join(file))?;
    }
    let imported = mfd::import(&evidence.join("mapping.mfd"))?;
    let validation = engine::validate(&imported.project);
    std::fs::write(
        evidence.join("IMPORT-VALIDATION-ORIGINAL.txt"),
        format!(
            "{:?}\n{:#?}\n{:#?}\n{validation:#?}\n",
            imported.mapping_path, imported.warnings, imported.project
        ),
    )?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(validation.is_empty(), "{validation:?}");
    let mut calls = imported
        .project
        .graph
        .nodes
        .values()
        .filter_map(|node| match node {
            Node::Call { function, args } if function == "format_guid_string" => Some(args),
            _ => None,
        });
    let arguments = calls.next().expect("the standard formatter is retained");
    assert!(calls.next().is_none());
    assert_eq!(arguments.len(), 1);
    assert!(
        matches!(imported.project.graph.nodes.get(&arguments[0]), Some(Node::SourceField { path, frame: None }) if path == &["Hex"])
    );
    let source = Instance::Group(
        vec![(
            "Hex".into(),
            Instance::Scalar(Value::String("0123456789abcdef0123456789abcdef".into())),
        )]
        .into(),
    );
    let wanted = Instance::Group(
        vec![(
            "Formatted".into(),
            Instance::Scalar(Value::String("01234567-89ab-cdef-0123-456789abcdef".into())),
        )]
        .into(),
    );
    let actual = engine::run(&imported.project, &source);
    std::fs::write(
        evidence.join("NATIVE-COMPLETE-ORIGINAL.txt"),
        format!("{source:#?}\n{actual:#?}\n{wanted:#?}\n"),
    )?;
    assert_eq!(actual?, wanted);
    let exported = evidence.join("roundtrip.mfd");
    let export_warnings = mfd::export(&imported.project, &exported)?;
    let exported_text = std::fs::read_to_string(&exported)?;
    let reimported = mfd::import(&exported)?;
    let revalidation = engine::validate(&reimported.project);
    let rerun = engine::run(&reimported.project, &source);
    std::fs::write(
        evidence.join("ROUNDTRIP-COMPLETE-ORIGINAL.txt"),
        format!(
            "{export_warnings:#?}\n{:?}\n{:#?}\n{:#?}\n{revalidation:#?}\n{rerun:#?}\n{wanted:#?}\n",
            reimported.mapping_path, reimported.warnings, reimported.project
        ),
    )?;
    assert!(export_warnings.is_empty(), "{export_warnings:?}");
    assert!(exported_text.contains("name=\"format-guid-string\" library=\"lang\""));
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(revalidation.is_empty(), "{revalidation:?}");
    assert_eq!(rerun?, wanted);
    Ok(())
}

#[test]
fn canonical_ir_names_in_vendor_libraries_remain_unresolved()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/format_guid_string");
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let evidence = std::env::temp_dir().join(format!(
        "ferrule-guid269-namespace-{}-{stamp}",
        std::process::id()
    ));
    std::fs::create_dir(&evidence)?;
    eprintln!("GUID269_NAMESPACE_ORIGINALS={}", evidence.display());
    let mapping = std::fs::read_to_string(fixture.join("mapping.mfd"))?;
    let mut results = Vec::new();
    for library in ["core", "lang"] {
        let package = evidence.join(library);
        std::fs::create_dir(&package)?;
        for file in ["source.xsd", "target.xsd"] {
            std::fs::copy(fixture.join(file), package.join(file))?;
        }
        let mapping = mapping.replace(
            "name=\"format-guid-string\" library=\"lang\"",
            &format!("name=\"format_guid_string\" library=\"{library}\""),
        );
        let path = package.join("mapping.mfd");
        std::fs::write(&path, mapping)?;
        let imported = mfd::import(&path)?;
        let functions = imported
            .project
            .graph
            .nodes
            .values()
            .filter_map(|node| match node {
                Node::Call { function, .. } => Some(function.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let validation = engine::validate(&imported.project);
        let lowered = codegen::lower(&imported.project);
        let strict = mfd::import_with_profile(
            &path,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        );
        let strict_original = strict.as_ref().map(|outcome| {
            (
                &outcome.imported.mapping_path,
                &outcome.imported.warnings,
                &outcome.imported.project,
                &outcome.report,
            )
        });
        std::fs::write(
            package.join("COMPLETE-IMPORT-ADMISSION-ORIGINAL.txt"),
            format!(
                "{:#?}\n{:#?}\n{functions:#?}\n{validation:#?}\n{lowered:#?}\n{strict_original:#?}\n",
                imported.warnings, imported.project
            ),
        )?;
        results.push((library, functions, validation, lowered, strict.is_err()));
    }
    std::fs::write(
        evidence.join("ALL-COMPLETE-NAMESPACE-RESULTS.txt"),
        format!("{results:#?}\n"),
    )?;
    for (library, functions, validation, lowered, strict_refused) in results {
        assert_eq!(
            functions,
            [format!("unsupported:{library}:5:format_guid_string")],
            "vendor-library names must not acquire canonical builtin identity"
        );
        assert!(
            validation
                .iter()
                .any(|error| error.to_string().contains("unknown function"))
        );
        assert!(lowered.is_err());
        assert!(strict_refused);
    }
    Ok(())
}
