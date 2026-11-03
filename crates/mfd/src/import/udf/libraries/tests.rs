use super::*;

#[test]
fn transitive_paths_keep_owner_root_and_do_not_inherit_main_scope() {
    let root = Path::new("/approved");
    let owner = root.join("nested/owner.mfd");
    assert_eq!(
        dependency_path(root, &owner, "../leaf.mfd").unwrap(),
        root.join("leaf.mfd")
    );
    for reference in [
        "../../escape.mfd",
        "/elsewhere.mfd",
        "C:/library.mfd",
        "https://host/library.mfd",
    ] {
        assert!(dependency_path(root, &owner, reference).is_err());
    }
}

#[test]
fn unsupported_import_formats_and_namespace_aliases_select_refusal_boundary() {
    for declaration in [
        r#"<settings><libraries><import path="a.dll" saverelative="1"/></libraries></settings>"#,
        r#"<settings><libraries xmlns="urn:unknown"><import path="a.mfd" saverelative="1"/></libraries></settings>"#,
        r#"<settings><libraries><import path="a.mfd" saverelative="1" alias="other"/></libraries></settings>"#,
    ] {
        let text = format!("<mapping>{declaration}</mapping>");
        let document = roxmltree::Document::parse(&text).unwrap();
        assert!(has_mapping_imports(document.root_element()));
        assert!(imports(document.root_element()).is_err());
    }
}

#[test]
fn closure_file_and_delivery_quotas_precede_read_or_cached_delivery() {
    let directory =
        std::env::temp_dir().join(format!("ferrule_udf_preallocation_{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    {
        let main = directory.join("main.mfd");
        std::fs::write(&main, "<mapping/>").unwrap();
        std::fs::write(directory.join("library.mfd"), "<mapping/>").unwrap();
        let resources = ResourceResolver::new(&main, Some(&directory)).unwrap();
        let reader = resources.mapping_library("library.mfd", [0; 32]).unwrap();
        assert_eq!(reader.bytes_bounded(reader.path(), 10).unwrap().len(), 10);
        assert!(reader.bytes_bounded(reader.path(), 9).is_err());
        let reader = resources.mapping_library("library.mfd", [0; 32]).unwrap();
        let mut budget = Budget::default();
        for index in 0..MAX_FILES {
            budget
                .files
                .insert(directory.join(format!("unused-{index}.mfd")), [0; 32]);
        }
        let error = read_document(
            reader.path(),
            &reader,
            &main,
            [0; 32],
            "library.mfd",
            &resources,
            &mut budget,
            &mut Vec::new(),
        )
        .err()
        .unwrap();
        assert!(error.contains("before resource read"));
        let mut budget = Budget {
            bytes: MAX_BYTES - 9,
            ..Default::default()
        };
        assert!(
            read_document(
                reader.path(),
                &reader,
                &main,
                [0; 32],
                "library.mfd",
                &resources,
                &mut budget,
                &mut Vec::new()
            )
            .is_err()
        );
    }
    std::fs::remove_dir_all(directory).unwrap();
}
