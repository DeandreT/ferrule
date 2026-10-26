use std::path::Path;

pub fn assert_retained(directory: &Path, kind: mapping::Json5StringComponentKind) {
    let design = directory.join("mapping.mfd");
    let original = std::fs::read_to_string(&design).unwrap();
    for value in ["1", "0", "invalid"] {
        let marked = original.replacen(
            "<json schema=",
            &format!("<json json5=\"{value}\" schema="),
            1,
        );
        assert_ne!(marked, original);
        std::fs::write(&design, marked).unwrap();
        let imported = mfd::import(&design).unwrap();
        let restored = mapping::project_file::decode_str(
            &mapping::project_file::encode_pretty(&imported.project).unwrap(),
        )
        .unwrap();
        let [retained] = restored
            .source_options
            .mfd_json5_string_dependencies
            .as_slice()
        else {
            panic!("string component dependency was lost")
        };
        assert_eq!(retained.component, "payload");
        assert_eq!(retained.kind, kind);
        assert_eq!(
            retained.input_instance.as_deref(),
            match kind {
                mapping::Json5StringComponentKind::Parser => Some("missing-design-time.json"),
                mapping::Json5StringComponentKind::Serializer => None,
            }
        );
        assert_eq!(
            retained.dependency,
            mapping::Json5InterchangeDependency::ComponentSetting {
                value: value.into()
            }
        );
        assert!(!restored.source_options.json5);
        assert!(
            engine::validate(&restored)
                .iter()
                .any(|issue| issue.message.contains("native JSON5 interchange"))
        );
        assert!(codegen::lower(&restored).is_err());
        assert!(matches!(
            mfd::import_with_profile(
                &design,
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable
            ),
            Err(mfd::MfdError::IncompatibleImport(_))
        ));
        let output = directory.join("blocked-string.mfd");
        std::fs::write(&output, "keep original").unwrap();
        assert!(mfd::export(&restored, &output).is_err());
        assert_eq!(std::fs::read_to_string(&output).unwrap(), "keep original");
    }
    std::fs::write(design, original).unwrap();
}
