use mfd::{MfdPackage, PackageError, export_package, import_package};

fn books() -> MfdPackage {
    MfdPackage::new(
        "books.mfd",
        [
            ("books.mfd", include_bytes!("fixtures/books.mfd").as_slice()),
            (
                "books-source.xsd",
                include_bytes!("fixtures/books-source.xsd").as_slice(),
            ),
            (
                "books-target.xsd",
                include_bytes!("fixtures/books-target.xsd").as_slice(),
            ),
        ],
    )
    .unwrap()
}

fn scalar_project(format: &str) -> mapping::Project {
    use ir::{ScalarType, SchemaNode};
    mapping::Project {
        source: SchemaNode::group(
            "Input",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        source_path: Some(format!("input.{format}")),
        target_path: Some(format!("output.{format}")),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        user_functions: Default::default(),
        failure_rules: Vec::new(),
        graph: mapping::Graph {
            nodes: std::collections::BTreeMap::from([(
                0,
                mapping::Node::SourceField {
                    path: vec!["Value".into()],
                    frame: None,
                },
            )]),
        },
        root: mapping::Scope {
            bindings: vec![mapping::Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            ..Default::default()
        },
    }
}

#[test]
fn xml_and_json_packages_preserve_scalar_semantics_with_nested_design_paths() {
    for format in ["xml", "json"] {
        let project = scalar_project(format);
        let exported =
            export_package(&project, "maps/design.mfd", mfd::ExportProfile::NativeMfd).unwrap();
        assert!(exported.report.is_native_compatible());
        let imported = import_package(&exported.package).unwrap();
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        let input = ir::Instance::Group(
            vec![(
                "Value".into(),
                ir::Instance::Scalar(ir::Value::String("hello λ".into())),
            )]
            .into(),
        );
        assert_eq!(
            engine::run(&imported.project, &input).unwrap(),
            engine::run(&project, &input).unwrap()
        );
    }
}

#[test]
fn transitive_schema_dependencies_are_supplied_not_reopened() {
    for format in ["xml", "json"] {
        let project = scalar_project(format);
        let exported =
            export_package(&project, "design.mfd", mfd::ExportProfile::NativeMfd).unwrap();
        let schema_name = if format == "xml" {
            "design-source.xsd"
        } else {
            "design-source.schema.json"
        };
        let dependency = if format == "xml" {
            "types.xsd"
        } else {
            "types.schema.json"
        };
        let original = exported.package.get(schema_name).unwrap();
        let wrapper = if format == "xml" {
            format!(
                "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"><xs:include schemaLocation=\"{dependency}\"/></xs:schema>"
            )
        } else {
            format!("{{\"$ref\":\"{dependency}\"}}")
        };
        let mut entries = exported
            .package
            .entries()
            .map(|(path, bytes)| (path.to_owned(), bytes.to_vec()))
            .collect::<Vec<_>>();
        entries
            .iter_mut()
            .find(|(path, _)| path == schema_name)
            .unwrap()
            .1 = wrapper.into_bytes();
        entries.push((dependency.into(), original.to_vec()));
        let complete = MfdPackage::new("design.mfd", entries.iter().map(|(p, b)| (p, b))).unwrap();
        assert!(import_package(&complete).unwrap().warnings.is_empty());
        entries.pop();
        let incomplete =
            MfdPackage::new("design.mfd", entries.iter().map(|(p, b)| (p, b))).unwrap();
        assert!(matches!(
            import_package(&incomplete),
            Err(mfd::MfdError::Package(PackageError::MissingEntry))
        ));
    }
}

#[test]
fn schema_reference_traversal_is_refused_before_normalization() {
    for format in ["xml", "json"] {
        for reference in [
            "../types.xsd",
            "/etc/passwd",
            "https://example.invalid/types.xsd",
            "C:/types.xsd",
            "sub/../types.xsd",
        ] {
            let exported = export_package(
                &scalar_project(format),
                "design.mfd",
                mfd::ExportProfile::NativeMfd,
            )
            .unwrap();
            let schema_name = if format == "xml" {
                "design-source.xsd"
            } else {
                "design-source.schema.json"
            };
            let wrapper = if format == "xml" {
                format!(
                    "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"><xs:include schemaLocation=\"{reference}\"/></xs:schema>"
                )
            } else {
                format!("{{\"$ref\":\"{reference}\"}}")
            };
            let mut entries = exported
                .package
                .entries()
                .map(|(path, bytes)| (path.to_owned(), bytes.to_vec()))
                .collect::<Vec<_>>();
            entries
                .iter_mut()
                .find(|(path, _)| path == schema_name)
                .unwrap()
                .1 = wrapper.into_bytes();
            let package =
                MfdPackage::new("design.mfd", entries.iter().map(|(p, b)| (p, b))).unwrap();
            assert!(
                matches!(
                    import_package(&package),
                    Err(mfd::MfdError::Package(PackageError::InvalidPath))
                ),
                "{format}: {reference}"
            );
        }
    }
}

fn protobuf_source_package() -> MfdPackage {
    MfdPackage::new(
        "protobuf-source.mfd",
        [
            (
                "protobuf-source.mfd",
                include_bytes!("fixtures/protobuf-source.mfd").as_slice(),
            ),
            (
                "protobuf-target.proto",
                include_bytes!("fixtures/protobuf-target.proto").as_slice(),
            ),
            (
                "protobuf-source-target.xsd",
                include_bytes!("fixtures/protobuf-source-target.xsd").as_slice(),
            ),
        ],
    )
    .unwrap()
}

#[test]
fn protobuf_packages_preserve_embedded_schema() {
    let package = protobuf_source_package();
    let imported = import_package(&package).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let exported = export_package(
        &imported.project,
        "roundtrip.mfd",
        mfd::ExportProfile::FerruleExtensions,
    )
    .unwrap();
    let restored = import_package(&exported.package).unwrap();
    assert!(restored.warnings.is_empty(), "{:?}", restored.warnings);
    assert_eq!(restored.project.source, imported.project.source);
}

#[test]
fn missing_json_pointer_targets_refuse_local_and_external_refs() {
    for reference in ["types.json#/$defs/Missing", "#/$defs/Missing"] {
        let exported = export_package(
            &scalar_project("json"),
            "design.mfd",
            mfd::ExportProfile::NativeMfd,
        )
        .unwrap();
        let mut entries = exported
            .package
            .entries()
            .map(|(path, bytes)| (path.to_owned(), bytes.to_vec()))
            .collect::<Vec<_>>();
        let source = entries
            .iter_mut()
            .find(|(name, _)| name == "design-source.schema.json")
            .unwrap();
        source.1 = serde_json::to_vec(&serde_json::json!({"title":"Input","type":"object","properties":{"Value":{"$ref":reference}},"required":["Value"]})).unwrap();
        entries.push((
            "types.json".into(),
            br#"{"$defs":{"Present":{"type":"string"}}}"#.to_vec(),
        ));
        let package = MfdPackage::new(
            "design.mfd",
            entries.iter().map(|(name, bytes)| (name, bytes)),
        )
        .unwrap();
        assert!(
            matches!(
                import_package(&package),
                Err(mfd::MfdError::Package(PackageError::InvalidResource))
            ),
            "{reference}"
        );
    }
}

#[test]
fn valid_json_pointer_targets_include_forward_and_recursive_definitions() {
    let exported = export_package(
        &scalar_project("json"),
        "design.mfd",
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap();
    let mut entries = exported
        .package
        .entries()
        .map(|(path, bytes)| (path.to_owned(), bytes.to_vec()))
        .collect::<Vec<_>>();
    entries.iter_mut().find(|(name, _)| name == "design-source.schema.json").unwrap().1 = br##"{"title":"Input","type":"object","properties":{"Value":{"$ref":"types.json#/$defs/Present"}},"required":["Value"]}"##.to_vec();
    entries.push((
        "types.json".into(),
        br##"{"$defs":{"Present":{"type":"string"},"Self":{"$ref":"#/$defs/Self"}}}"##.to_vec(),
    ));
    let package = MfdPackage::new(
        "design.mfd",
        entries.iter().map(|(name, bytes)| (name, bytes)),
    )
    .unwrap();
    assert!(import_package(&package).unwrap().warnings.is_empty());
}

#[test]
fn legacy_filesystem_missing_json_pointer_remains_permissive() {
    use std::io::Write;
    struct File(std::path::PathBuf);
    impl Drop for File {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let path = std::env::temp_dir().join(format!(
        "ferrule_package_legacy_ref_{}.json",
        std::process::id()
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    let guard = File(path);
    file.write_all(br##"{"type":"object","properties":{"Value":{"$ref":"#/$defs/Missing"}}}"##)
        .unwrap();
    drop(file);
    let schema = format_json::json_schema::import(&guard.0).unwrap();
    assert!(matches!(
        schema.child("Value").map(|value| &value.kind),
        Some(ir::SchemaKind::Scalar {
            ty: ir::ScalarType::String
        })
    ));
}

#[test]
fn protobuf_consumes_only_reachable_package_sources() {
    let original = protobuf_source_package();
    let mut entries = original
        .entries()
        .map(|(path, bytes)| (path.to_owned(), bytes.to_vec()))
        .collect::<Vec<_>>();
    entries.push(("unrelated-invalid.proto".into(), vec![0xff]));
    for index in 0..3 {
        entries.push((
            format!("unrelated-{index}.proto"),
            vec![b' '; 3 * 1024 * 1024],
        ));
    }
    let package = MfdPackage::new(
        original.design_path(),
        entries.iter().map(|(name, bytes)| (name, bytes)),
    )
    .unwrap();
    let expected = import_package(&original).unwrap();
    let actual = import_package(&package).unwrap();
    assert!(actual.warnings.is_empty(), "{:?}", actual.warnings);
    assert_eq!(actual.project.source, expected.project.source);
}

#[test]
fn protobuf_reader_is_lazy_and_preserves_reachable_limits() {
    use std::sync::Arc;
    let mut reads = Vec::new();
    let bundle = format_protobuf::SchemaBundle::from_reader("root.proto", |path| {
        reads.push(path.to_owned());
        let bytes: &[u8] = match path {
            "root.proto" => br#"syntax = "proto2"; import "types.inc"; message Item { optional Name name = 1; }"#,
            "types.inc" => br#"syntax = "proto2"; message Name { optional string value = 1; }"#,
            _ => panic!("unreachable resource read"),
        };
        Ok(Arc::from(bytes))
    }).unwrap();
    assert_eq!(reads, ["root.proto", "types.inc"]);
    assert_eq!(bundle.imports().len(), 1);
    let too_large = format_protobuf::SchemaBundle::from_reader("root.proto", |_| {
        Ok(Arc::from(vec![b' '; format_protobuf::MAX_SCHEMA_BYTES + 1]))
    });
    assert!(too_large.is_err());
    let huge = " ".repeat(3 * 1024 * 1024);
    assert!(
        format_protobuf::SchemaBundle::from_sources(
            "root.proto",
            "syntax = \"proto2\"; message Root {}",
            [
                ("unused1.proto", huge.as_str()),
                ("unused2.proto", huge.as_str()),
                ("unused3.proto", huge.as_str())
            ]
        )
        .is_ok()
    );
}

#[test]
fn protobuf_reachable_graph_count_and_aggregate_limits_are_enforced() {
    use std::sync::Arc;
    let mut reads = 0;
    let count_limit = format_protobuf::SchemaBundle::from_reader("root.proto", |path| {
        reads += 1;
        let source = if path == "root.proto" {
            let imports = (0..format_protobuf::MAX_SCHEMA_FILES)
                .map(|index| format!("import \"f{index}.proto\"; "))
                .collect::<String>();
            format!("syntax = \"proto2\"; {imports} message Root {{}}")
        } else {
            format!(
                "syntax = \"proto2\"; message {} {{}}",
                path.trim_end_matches(".proto")
            )
        };
        Ok(Arc::from(source.into_bytes()))
    });
    assert!(count_limit.is_err());
    assert_eq!(reads, format_protobuf::MAX_SCHEMA_FILES);
    let byte_limit = format_protobuf::SchemaBundle::from_reader("f0.proto", |path| {
        let index = path
            .trim_start_matches('f')
            .trim_end_matches(".proto")
            .parse::<usize>()
            .unwrap();
        let import = if index < 9 {
            format!("import \"f{}.proto\";", index + 1)
        } else {
            String::new()
        };
        Ok(Arc::from(
            format!(
                "syntax = \"proto2\"; {import} message M{index} {{}} {}",
                " ".repeat(900 * 1024)
            )
            .into_bytes(),
        ))
    });
    assert!(byte_limit.is_err());
}

#[test]
fn protobuf_export_refuses_noncanonical_retained_imports() {
    let mut project = import_package(&protobuf_source_package()).unwrap().project;
    let protobuf = project.source_options.protobuf.as_mut().unwrap();
    protobuf.schema = format!(
        "syntax = \"proto2\"; import \"sub/../types.proto\"; {}",
        include_str!("fixtures/protobuf-target.proto").replace("syntax = \"proto2\";", "")
    );
    protobuf.schema_path = Some("root.proto".into());
    protobuf.imports.push(mapping::ProtobufSchemaFile {
        path: "types.proto".into(),
        source: "syntax = \"proto2\"; message Unused {}".into(),
    });
    assert!(mfd::preflight_export(&project, std::path::Path::new("legacy.mfd")).is_ok());
    assert!(
        export_package(
            &project,
            "output.mfd",
            mfd::ExportProfile::FerruleExtensions
        )
        .is_err()
    );
    protobuf_import_with_canonical_dependency_succeeds();
}

fn protobuf_import_with_canonical_dependency_succeeds() {
    let mut project = import_package(&protobuf_source_package()).unwrap().project;
    let protobuf = project.source_options.protobuf.as_mut().unwrap();
    protobuf.schema = format!(
        "syntax = \"proto2\"; import \"types.inc\"; {}",
        include_str!("fixtures/protobuf-target.proto").replace("syntax = \"proto2\";", "")
    );
    protobuf.schema_path = Some("root.proto".into());
    protobuf.imports.push(mapping::ProtobufSchemaFile {
        path: "types.inc".into(),
        source: "syntax = \"proto2\"; message Unused {}".into(),
    });
    let exported = export_package(
        &project,
        "output.mfd",
        mfd::ExportProfile::FerruleExtensions,
    )
    .unwrap();
    assert!(
        import_package(&exported.package)
            .unwrap()
            .warnings
            .is_empty()
    );
}

fn xbrl_target_project() -> mapping::Project {
    let scalar = export_package(
        &scalar_project("xml"),
        "design.mfd",
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap();
    let mfd = br#"<mapping version="26"><component name="map"><structure><children>
      <component name="Input" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Input"><entry name="Value" outkey="10"/></entry></entry></entry></root><document schema="input.xsd" inputinstance="input.xml" instanceroot="{}Input"/></data></component>
      <component name="Facts" library="xbrl" kind="27"><properties XSLTDefaultOutput="1"/><data><root><header><namespaces><namespace/><namespace uid="urn:package"/></namespaces></header><entry name="FileInstance"><entry name="document"><entry name="xbrl"><entry name="Report"><entry name="Amount" ns="1" inpkey="20"/></entry></entry></entry></entry></root><xbrl schema="taxonomy.xsd" sps="layout.sps" outputinstance="facts.xbrl"/></data></component>
      </children></structure><connections><edge from="10" to="20"/></connections></component></mapping>"#;
    let presentation = br#"<structure><schemasources><namespaces><nspair prefix="ex" uri="urn:package"/></namespaces></schemasources><template subtype="xbrl-concept-aspect" match="ex:Amount"><children><calltemplate subtype="named" match="monetaryItemType"/></children></template></structure>"#;
    let source = scalar.package.get("design-source.xsd").unwrap();
    let package = MfdPackage::new(
        "input.mfd",
        [
            ("input.mfd", mfd.as_slice()),
            ("input.xsd", source.as_ref()),
            ("layout.sps", presentation.as_slice()),
        ],
    )
    .unwrap();
    let imported = import_package(&package).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    imported.project
}

#[test]
fn xbrl_export_refuses_aliases_and_preserves_canonical_presentation_roundtrip() {
    let original = xbrl_target_project();
    let exported = export_package(
        &original,
        "output.mfd",
        mfd::ExportProfile::FerruleExtensions,
    )
    .unwrap();
    let restored = import_package(&exported.package).unwrap();
    assert_eq!(
        restored.project.target_options.xbrl,
        original.target_options.xbrl
    );
    let input = ir::Instance::Group(
        vec![(
            "Value".into(),
            ir::Instance::Scalar(ir::Value::String("12.50".into())),
        )]
        .into(),
    );
    assert_eq!(
        engine::run(&restored.project, &input).unwrap(),
        engine::run(&original, &input).unwrap()
    );
    for reference in ["./layout.sps", "sub\\layout.sps", "sub/../layout.sps"] {
        let mut project = original.clone();
        let boundary = project.target_options.xbrl.take().unwrap();
        project.target_options.xbrl = Some(
            mapping::XbrlBoundaryOptions::external_target(boundary.taxonomy(), Some(reference))
                .unwrap()
                .with_namespace_bindings(boundary.namespace_bindings().to_vec())
                .unwrap()
                .with_fact_bindings(boundary.fact_bindings().to_vec())
                .unwrap(),
        );
        assert!(
            matches!(
                export_package(
                    &project,
                    "output.mfd",
                    mfd::ExportProfile::FerruleExtensions
                ),
                Err(mfd::MfdError::Package(PackageError::InvalidPath))
            ),
            "{reference}"
        );
    }
}

#[test]
fn supplied_protobuf_bundle_is_closed_and_canonical() {
    let source =
        "syntax = \"proto2\"; import \"types.proto\"; message Item { optional Name name = 1; }";
    let types = "syntax = \"proto2\"; message Name { optional string value = 1; }";
    let bundle =
        format_protobuf::SchemaBundle::from_sources("root.proto", source, [("types.proto", types)])
            .unwrap();
    assert_eq!(bundle.imports().len(), 1);
    assert!(format_protobuf::SchemaBundle::from_sources("root.proto", source, []).is_err());
    assert!(
        format_protobuf::SchemaBundle::from_sources(
            "../root.proto",
            source,
            [("types.proto", types)]
        )
        .is_err()
    );
    assert!(
        format_protobuf::SchemaBundle::from_sources(
            "root.proto",
            &source.replace("types.proto", "sub/../types.proto"),
            [("types.proto", types)]
        )
        .is_err()
    );
}

#[test]
fn dtd_schema_packages_do_not_require_xsd_metadata_or_files() {
    let exported = export_package(
        &scalar_project("xml"),
        "design.mfd",
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap();
    let mut entries = exported
        .package
        .entries()
        .filter(|(path, _)| *path != "design-source.xsd")
        .map(|(path, bytes)| (path.to_owned(), bytes.to_vec()))
        .collect::<Vec<_>>();
    let design = entries
        .iter_mut()
        .find(|(path, _)| path == "design.mfd")
        .unwrap();
    design.1 = String::from_utf8(std::mem::take(&mut design.1))
        .unwrap()
        .replace("design-source.xsd", "source.dtd")
        .into_bytes();
    entries.push((
        "source.dtd".into(),
        b"<!ELEMENT Input (Value)><!ELEMENT Value (#PCDATA)>".to_vec(),
    ));
    let package = MfdPackage::new("design.mfd", entries.iter().map(|(p, b)| (p, b))).unwrap();
    let imported = import_package(&package).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.source, scalar_project("xml").source);
}

#[test]
fn database_boundaries_refuse_instead_of_returning_an_unimportable_package() {
    for source in [true, false] {
        let mut project = scalar_project("xml");
        if source {
            project.source_path = Some("input.sqlite".into());
        } else {
            project.target_path = Some("output.sqlite".into());
        }
        assert!(
            export_package(
                &project,
                "design.mfd",
                mfd::ExportProfile::FerruleExtensions
            )
            .is_err()
        );
    }
}

#[test]
fn package_round_trip_preserves_execution() {
    let original = import_package(&books()).unwrap();
    assert!(original.warnings.is_empty(), "{:?}", original.warnings);
    let exported = export_package(
        &original.project,
        "copy.mfd",
        mfd::ExportProfile::FerruleExtensions,
    )
    .unwrap();
    assert!(
        exported.report.warnings.is_empty(),
        "{:?}",
        exported.report.warnings
    );
    let restored = import_package(&exported.package).unwrap();
    assert!(restored.warnings.is_empty(), "{:?}", restored.warnings);
    let input =
        format_xml::from_str(include_str!("fixtures/books.xml"), &original.project.source).unwrap();
    let expected = engine::run(&original.project, &input).unwrap();
    let actual = engine::run(&restored.project, &input).unwrap();
    assert_eq!(
        format_xml::to_string(&restored.project.target, &actual).unwrap(),
        format_xml::to_string(&original.project.target, &expected).unwrap()
    );
}

#[test]
fn refuses_unsafe_and_aliased_paths() {
    for path in [
        "",
        "/design.mfd",
        "../design.mfd",
        "a/../design.mfd",
        "a//b",
        "./a",
        "a\\b",
        "C:/a",
        "https://a/b",
        "a\0b",
        "a.",
        "a ",
    ] {
        assert_eq!(
            MfdPackage::new(path, [(path, b"x")]).unwrap_err(),
            PackageError::InvalidPath,
            "{path:?}"
        );
    }
    assert_eq!(
        MfdPackage::new("a.mfd", [("a.mfd", b"x"), ("A.mfd", b"y")]).unwrap_err(),
        PackageError::DuplicatePath
    );
    assert_eq!(
        MfdPackage::new("a.mfd", [("other.mfd", b"x")]).unwrap_err(),
        PackageError::MissingEntry
    );
}

#[test]
fn missing_schema_is_a_stable_refusal_not_entry_tree_fallback() {
    let package = MfdPackage::new(
        "books.mfd",
        [("books.mfd", include_bytes!("fixtures/books.mfd").as_slice())],
    )
    .unwrap();
    assert!(matches!(
        import_package(&package),
        Err(mfd::MfdError::Package(PackageError::MissingEntry))
    ));
}

#[test]
fn enforces_file_count_and_byte_limits() {
    let oversized = vec![0; mfd::MAX_PACKAGE_FILE_BYTES + 1];
    assert_eq!(
        MfdPackage::new("a.mfd", [("a.mfd", &oversized)]).unwrap_err(),
        PackageError::LimitExceeded
    );
    let files = (0..=mfd::MAX_PACKAGE_ENTRIES).map(|i| (format!("{i}.mfd"), b""));
    assert_eq!(
        MfdPackage::new("0.mfd", files).unwrap_err(),
        PackageError::LimitExceeded
    );
    let contents = vec![0; mfd::MAX_PACKAGE_FILE_BYTES];
    let files = (0..5).map(|i| (format!("{i}.mfd"), &contents));
    assert_eq!(
        MfdPackage::new("0.mfd", files).unwrap_err(),
        PackageError::LimitExceeded
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn package_operations_under_filesystem_and_network_denial() {
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "denied_filesystem_child",
            "--test-threads=1",
            "--nocapture",
        ])
        .env("FERRULE_MEMORY_PACKAGE_DENIED_CHILD", "1")
        .status()
        .unwrap();
    assert!(status.success());
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn denied_filesystem_child() {
    if std::env::var_os("FERRULE_MEMORY_PACKAGE_DENIED_CHILD").is_none() {
        return;
    }
    use seccompiler::{SeccompAction, SeccompFilter, TargetArch};
    let denied = [
        libc::SYS_open,
        libc::SYS_openat,
        libc::SYS_openat2,
        libc::SYS_creat,
        libc::SYS_stat,
        libc::SYS_lstat,
        libc::SYS_newfstatat,
        libc::SYS_statx,
        libc::SYS_readlink,
        libc::SYS_readlinkat,
        libc::SYS_access,
        libc::SYS_faccessat,
        libc::SYS_getdents,
        libc::SYS_getdents64,
        libc::SYS_mkdir,
        libc::SYS_mkdirat,
        libc::SYS_unlink,
        libc::SYS_unlinkat,
        libc::SYS_rename,
        libc::SYS_renameat,
        libc::SYS_socket,
        libc::SYS_connect,
    ];
    let filter = SeccompFilter::new(
        denied
            .into_iter()
            .map(|number| (number, Vec::new()))
            .collect(),
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM as u32),
        TargetArch::x86_64,
    )
    .unwrap();
    let program: seccompiler::BpfProgram = filter.try_into().unwrap();
    rustix::thread::set_no_new_privs(true).unwrap();
    seccompiler::apply_filter(&program).unwrap();
    assert_eq!(
        std::fs::read("/etc/passwd").unwrap_err().kind(),
        std::io::ErrorKind::PermissionDenied
    );
    assert!(std::net::TcpStream::connect("127.0.0.1:9").is_err());
    package_round_trip_preserves_execution();
    missing_schema_is_a_stable_refusal_not_entry_tree_fallback();
    xml_and_json_packages_preserve_scalar_semantics_with_nested_design_paths();
    transitive_schema_dependencies_are_supplied_not_reopened();
    schema_reference_traversal_is_refused_before_normalization();
    protobuf_packages_preserve_embedded_schema();
    supplied_protobuf_bundle_is_closed_and_canonical();
    dtd_schema_packages_do_not_require_xsd_metadata_or_files();
    database_boundaries_refuse_instead_of_returning_an_unimportable_package();
    missing_json_pointer_targets_refuse_local_and_external_refs();
    valid_json_pointer_targets_include_forward_and_recursive_definitions();
    protobuf_consumes_only_reachable_package_sources();
    protobuf_reader_is_lazy_and_preserves_reachable_limits();
    protobuf_reachable_graph_count_and_aggregate_limits_are_enforced();
    protobuf_export_refuses_noncanonical_retained_imports();
    xbrl_export_refuses_aliases_and_preserves_canonical_presentation_roundtrip();
}
