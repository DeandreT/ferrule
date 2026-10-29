#[path = "support/qualified_root_fixture.rs"]
mod fixture;
use fixture::{Directory, Shape, project, read};
use ir::{Instance, Value};

#[test]
fn preflight_owns_schema_buffers_without_creating_destination_directory() {
    let directory = Directory::new();
    let destination = directory.0.join("absent/directory/roundtrip.mfd");
    for shape in [
        Shape::Unqualified,
        Shape::Qualified,
        Shape::Mixed,
        Shape::Fanout,
    ] {
        let report = mfd::preflight_export(&project(shape, true, false), &destination).unwrap();
        assert!(report.is_native_compatible());
        assert!(report.warnings.is_empty());
        assert!(!directory.0.join("absent").exists());
        assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 0);
    }
}

#[test]
fn public_export_import_roundtrip_preserves_data_and_required_source_ownership() {
    let directory = Directory::new();
    for (index, shape) in [
        Shape::Unqualified,
        Shape::Qualified,
        Shape::Mixed,
        Shape::Fanout,
    ]
    .into_iter()
    .enumerate()
    {
        let original = project(shape, true, false);
        let path = directory.0.join(format!("mapping-{index}.mfd"));
        mfd::export_with_profile(&original, &path, mfd::ExportProfile::NativeMfd).unwrap();
        let outcome =
            mfd::import_with_profile(&path, &Default::default(), mfd::ImportProfile::Executable)
                .unwrap();
        assert!(outcome.report.executable);
        let imported = outcome.imported.project;
        assert_eq!(imported.source, original.source);
        assert_eq!(imported.target, original.target);
        assert_eq!(imported.source_options, original.source_options);
        assert_eq!(imported.target_options, original.target_options);
        assert_eq!(imported.source_path, original.source_path);
        assert_eq!(imported.target_path, original.target_path);
        assert!(imported.target_options.xml_schema_hints.is_none());
        for xml in [
            shape.xml(Some("Extended"), true),
            shape.xml(None, true),
            shape.xml(Some("Basic"), true),
        ] {
            assert_eq!(
                engine::run(&imported, &read(&imported, &xml)).unwrap(),
                engine::run(&original, &read(&original, &xml)).unwrap()
            );
        }
        let missing = shape.xml(Some("Extended"), false);
        let result = engine::run(&imported, &read(&imported, &missing));
        if matches!(shape, Shape::Fanout) {
            assert_eq!(
                result.unwrap().field("Extra"),
                Some(&Instance::Scalar(Value::String("code-value".into())))
            );
        } else {
            assert!(format!("{:?}", result.unwrap_err()).contains("MissingRequiredField"));
        }
    }
}

#[test]
fn target_required_use_does_not_change_source_read_requirements() {
    let directory = Directory::new();
    for source_required in [false, true] {
        for target_required in [false, true] {
            let original = project(Shape::Unqualified, source_required, target_required);
            let path = directory
                .0
                .join(format!("use-{source_required}-{target_required}.mfd"));
            mfd::export(&original, &path).unwrap();
            let imported = mfd::import(&path).unwrap().project;
            let selected = Shape::Unqualified.xml(Some("Extended"), false);
            assert_eq!(
                engine::run(&imported, &read(&imported, &selected)).is_err(),
                source_required
            );
            let unmarked = Shape::Unqualified.xml(None, false);
            let output = engine::run(&imported, &read(&imported, &unmarked)).unwrap();
            assert_eq!(output.field("Extra"), Some(&Instance::Scalar(Value::Null)));
        }
    }
}

#[test]
fn detected_invalid_root_profile_refuses_both_import_profiles() {
    let directory = Directory::new();
    let path = directory.0.join("mapping.mfd");
    mfd::export(&project(Shape::Unqualified, true, false), &path).unwrap();
    let original = std::fs::read_to_string(&path).unwrap();
    assert!(original.contains("AddSchemaLocation=\"0\""));
    for invalid in [
        original.replace(
            "displayselectionmode=\"all\"",
            "displayselectionmode=\"other\"",
        ),
        original.replace("name=\"equal\"", "name=\"unequal\""),
        original.replace("datatype=\"QName\"", "datatype=\"string\""),
        original.replace("AddSchemaLocation=\"0\"", "AddSchemaLocation=\"invalid\""),
    ] {
        assert_ne!(invalid, original);
        std::fs::write(&path, invalid).unwrap();
        for profile in [
            mfd::ImportProfile::BestEffort,
            mfd::ImportProfile::Executable,
        ] {
            assert!(
                matches!(mfd::import_with_profile(&path, &Default::default(), profile), Err(mfd::MfdError::UnsupportedImport(reason)) if reason.starts_with("closed XML root profile "))
            );
        }
    }
}

#[test]
fn reference_default_schema_location_imports_the_hint_and_reexports_the_default() {
    let directory = Directory::new();
    for (index, shape) in [Shape::Unqualified, Shape::Qualified]
        .into_iter()
        .enumerate()
    {
        let original = project(shape, true, false);
        let path = directory.0.join(format!("hint-{index}.mfd"));
        mfd::export_with_profile(&original, &path, mfd::ExportProfile::NativeMfd).unwrap();
        let exported = std::fs::read_to_string(&path).unwrap();
        for setting in ["", " AddSchemaLocation=\"1\""] {
            std::fs::write(&path, exported.replace(" AddSchemaLocation=\"0\"", setting)).unwrap();
            let imported = mfd::import_with_profile(
                &path,
                &Default::default(),
                mfd::ImportProfile::Executable,
            )
            .unwrap()
            .imported
            .project;
            let hint = |location: &str| match &imported.target.xml_namespace {
                Some(ir::XmlNamespace::Qualified(uri)) => ir::XmlSchemaHints {
                    no_namespace_location: None,
                    locations: vec![ir::XmlSchemaLocation {
                        namespace: uri.as_str().into(),
                        location: location.into(),
                    }],
                    derivation: Some(ir::XmlSchemaHintDerivation {
                        schema_reference: location.into(),
                    }),
                },
                _ => ir::XmlSchemaHints {
                    no_namespace_location: Some(location.into()),
                    locations: Vec::new(),
                    derivation: Some(ir::XmlSchemaHintDerivation {
                        schema_reference: location.into(),
                    }),
                },
            };
            assert_eq!(
                imported.target_options.xml_schema_hints,
                Some(hint(&format!("hint-{index}-target.xsd")))
            );
            assert_eq!(imported.source, original.source);
            assert_eq!(imported.target, original.target);
            let again = directory.0.join(format!("again-{index}.mfd"));
            mfd::export_with_profile(&imported, &again, mfd::ExportProfile::NativeMfd).unwrap();
            assert!(
                !std::fs::read_to_string(&again)
                    .unwrap()
                    .contains("AddSchemaLocation")
            );
            let reimported = mfd::import(&again).unwrap().project;
            assert_eq!(
                reimported.target_options.xml_schema_hints,
                Some(hint(&format!("again-{index}-target.xsd")))
            );
        }
    }
}

#[test]
fn unsupported_export_preserves_all_existing_files() {
    let directory = Directory::new();
    let path = directory.0.join("mapping.mfd");
    mfd::export(&project(Shape::Unqualified, true, false), &path).unwrap();
    let before = std::fs::read_dir(&directory.0)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            (
                path.clone(),
                (
                    std::fs::read(&path).unwrap(),
                    std::fs::metadata(&path).unwrap().modified().unwrap(),
                ),
            )
        })
        .collect::<Vec<_>>();
    for change in 0..4 {
        let mut invalid = project(Shape::Unqualified, true, false);
        match change {
            0 => {
                invalid
                    .graph
                    .nodes
                    .insert(99, mapping::Node::Const { value: Value::Null });
            }
            1 => invalid.source_options.xml_allow_inactive_root_type_members = false,
            2 => invalid.target_options.xml_document = true,
            _ => invalid.source_path = Some("input.json".into()),
        }
        assert!(mfd::preflight_export(&invalid, &path).is_err());
        assert!(mfd::export(&invalid, &path).is_err());
        for (path, (bytes, modified)) in &before {
            assert_eq!(&std::fs::read(path).unwrap(), bytes);
            assert_eq!(
                std::fs::metadata(path).unwrap().modified().unwrap(),
                *modified
            );
        }
        assert_eq!(
            std::fs::read_dir(&directory.0).unwrap().count(),
            before.len()
        );
    }
}

#[test]
fn missing_base_display_mode_retains_existing_zero_port_refusal() {
    let directory = Directory::new();
    let path = directory.0.join("mapping.mfd");
    mfd::export(&project(Shape::Unqualified, true, false), &path).unwrap();
    let original = std::fs::read_to_string(&path).unwrap();
    assert_eq!(original.matches(" displayselectionmode=\"all\"").count(), 2);
    let changed = original.replace(" displayselectionmode=\"all\"", "");
    roxmltree::Document::parse(&changed).unwrap();
    std::fs::write(&path, changed).unwrap();
    for profile in [
        mfd::ImportProfile::BestEffort,
        mfd::ImportProfile::Executable,
    ] {
        assert!(matches!(
            mfd::import_with_profile(&path, &Default::default(), profile),
            Err(mfd::MfdError::UnsupportedImport(reason))
                if reason.starts_with("connected XML document-root direction was recovered for a zero-port first-entry projection; root-view ownership and condition/construction remain unsupported; no project is published: ")
                    && reason.contains("unproved XML document-root view")
        ));
    }
}

#[test]
fn plural_root_type_condition_has_typed_refusal_without_partial_fallback() {
    let directory = Directory::new();
    let path = directory.0.join("mapping.mfd");
    mfd::export(&project(Shape::Unqualified, true, false), &path).unwrap();
    let original = std::fs::read_to_string(&path).unwrap();
    let changed = original
        .replace("<condition>", "<conditions>")
        .replace("</condition>", "</conditions>");
    assert_ne!(changed, original);
    roxmltree::Document::parse(&changed).unwrap();
    std::fs::write(&path, changed).unwrap();
    for profile in [
        mfd::ImportProfile::BestEffort,
        mfd::ImportProfile::Executable,
    ] {
        assert!(matches!(
            mfd::import_with_profile(&path, &Default::default(), profile),
            Err(mfd::MfdError::UnsupportedImport(reason))
                if reason == "closed XML root profile shape: unrepresented or namespaced structural child"
        ));
    }
}
