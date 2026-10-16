#![cfg(any(
    target_os = "android",
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "visionos",
    target_os = "watchos",
    target_os = "redox"
))]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use cli::{BundleCatalog, BundleCatalogKind, BundleOptions, bundle_project};
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, NamedSource, NamedTarget, Node, Project, Scope};
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};

struct Temp(PathBuf);
impl Temp {
    fn new() -> anyhow::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let parent = std::env::var_os("FERRULE_BUNDLE_EVIDENCE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let root = parent.join(format!(
            "ferrule-bundle-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root)?;
        Ok(Self(root))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        if std::env::var_os("FERRULE_BUNDLE_EVIDENCE_ROOT").is_none() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
fn write(root: &Path, path: &str, bytes: impl AsRef<[u8]>) -> anyhow::Result<()> {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, bytes)?;
    Ok(())
}
fn project() -> Project {
    let string = |name| SchemaNode::scalar(name, ScalarType::String);
    let json_options = || FormatOptions {
        json_document: true,
        ..Default::default()
    };
    Project {
        source: SchemaNode::group("Input", vec![string("Value")]),
        target: SchemaNode::group(
            "Output",
            vec![
                string("Value"),
                SchemaNode::scalar("Precise", ScalarType::Float),
                string("Label"),
            ],
        ),
        source_path: Some("../data/input.json".into()),
        target_path: Some("../out/result.json".into()),
        source_options: json_options(),
        target_options: json_options(),
        extra_sources: vec![NamedSource {
            name: "catalog".into(),
            path: "../data/catalog.json".into(),
            schema: SchemaNode::group("Catalog", vec![string("Label")]),
            options: json_options(),
            dynamic_path: None,
        }],
        extra_targets: vec![NamedTarget {
            name: "audit".into(),
            path: Some("../out/audit.json".into()),
            schema: SchemaNode::group("Audit", vec![string("Label")]),
            options: json_options(),
            root: Scope {
                bindings: vec![Binding {
                    target_field: "Label".into(),
                    node: 2,
                }],
                ..Default::default()
            },
        }],
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: None,
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::Float(1.0),
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["catalog".into(), "Label".into()],
                        frame: None,
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "Value".into(),
                    node: 0,
                },
                Binding {
                    target_field: "Precise".into(),
                    node: 1,
                },
                Binding {
                    target_field: "Label".into(),
                    node: 2,
                },
            ],
            ..Default::default()
        },
        failure_rules: Vec::new(),
        user_functions: Default::default(),
    }
}
fn source(root: &Path) -> anyhow::Result<()> {
    std::fs::create_dir(root)?;
    let p = project();
    assert!(engine::validate(&p).is_empty());
    write(
        root,
        "maps/mapper.json",
        mapping::project_file::encode_pretty(&p)?,
    )?;
    write(root, "data/input.json", "{\"Value\":\"Café\"}\n")?;
    write(root, "data/catalog.json", b"{\"Label\":\"lookup\"}\n")?;
    write(
        root,
        "resources/json/shared/source.schema.json",
        include_bytes!("fixtures/offline_bundle/source.schema.json"),
    )?;
    write(
        root,
        "private-credential.txt",
        b"EXCLUDED SYNTHETIC SENTINEL",
    )?;
    Ok(())
}
fn options(include_inputs: bool) -> BundleOptions {
    BundleOptions {
        project: "maps/mapper.json".into(),
        resources: vec!["resources/json/shared/source.schema.json".into()],
        catalogs: vec![BundleCatalog {
            kind: BundleCatalogKind::JsonSchema,
            root: "resources/json".into(),
        }],
        include_inputs,
    }
}
fn tree(root: &Path) -> anyhow::Result<BTreeMap<String, Vec<u8>>> {
    fn walk(
        root: &Path,
        path: &Path,
        result: &mut BTreeMap<String, Vec<u8>>,
    ) -> anyhow::Result<()> {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                walk(root, &entry.path(), result)?;
            } else {
                result.insert(
                    entry
                        .path()
                        .strip_prefix(root)?
                        .to_str()
                        .unwrap()
                        .replace('\\', "/"),
                    std::fs::read(entry.path())?,
                );
            }
        }
        Ok(())
    }
    let mut result = BTreeMap::new();
    walk(root, root, &mut result)?;
    Ok(result)
}
fn inventory(root: &Path) -> anyhow::Result<Json> {
    Ok(serde_json::from_slice(&std::fs::read(
        root.join("ferrule-bundle.json"),
    )?)?)
}

#[test]
fn default_excludes_instances_and_preserves_complete_project_and_float_tags() -> anyhow::Result<()>
{
    let temp = Temp::new()?;
    let original = temp.0.join("source");
    source(&original)?;
    // Omission must also succeed when the configured input files are absent.
    std::fs::remove_file(original.join("data/input.json"))?;
    std::fs::remove_file(original.join("data/catalog.json"))?;
    let out = temp.0.join("bundle");
    let result = bundle_project(&original, &out, &options(false));
    eprintln!("default bundle original: {result:#?}");
    let outcome = result?;
    let actual_tree = tree(&out)?;
    eprintln!("complete default tree original: {actual_tree:#?}");
    assert_eq!(
        actual_tree.keys().map(String::as_str).collect::<Vec<_>>(),
        vec![
            "ferrule-bundle.json",
            "ferrule-package.json",
            "project.json",
            "resources/json/shared/source.schema.json"
        ]
    );
    assert_eq!(outcome.input_files, 0);
    assert_eq!(outcome.files_written, 4);
    let loaded = mapping::project_file::decode_bytes(&actual_tree["project.json"])?;
    let mut expected = project();
    expected.source_path = Some("data/input.json".into());
    expected.target_path = Some("out/result.json".into());
    expected.extra_sources[0].path = "data/catalog.json".into();
    expected.extra_targets[0].path = Some("out/audit.json".into());
    assert_eq!(
        serde_json::to_value(&loaded)?,
        serde_json::to_value(expected)?
    );
    assert!(
        matches!(loaded.graph.nodes.get(&1), Some(Node::Const { value: Value::Float(v) }) if v.to_bits() == 1.0_f64.to_bits())
    );
    let manifest = mfd::PackageManifest::load(&out.join("ferrule-package.json"))?;
    assert_eq!(
        manifest.json_schema_catalog_roots(),
        &[std::fs::canonicalize(out.join("resources/json"))?]
    );
    let report = inventory(&out)?;
    assert_eq!(report["schemaVersion"], 1);
    assert_eq!(report["kind"], "ferrule.project-bundle");
    assert_eq!(report["originalProject"]["path"], "maps/mapper.json");
    assert!(!serde_json::to_string(&report)?.contains(temp.0.to_str().unwrap()));
    assert_eq!(
        report["pathRebases"],
        json!([
        {"owner":"source_path","from":"../data/input.json","to":"data/input.json"},
        {"owner":"target_path","from":"../out/result.json","to":"out/result.json"},
        {"owner":"extra_sources[0].path","from":"../data/catalog.json","to":"data/catalog.json"},
        {"owner":"extra_targets[0].path","from":"../out/audit.json","to":"out/audit.json"}])
    );
    for entry in report["files"].as_array().unwrap() {
        let bytes = &actual_tree[entry["path"].as_str().unwrap()];
        assert_eq!(entry["byteLen"], bytes.len());
        assert_eq!(entry["sha256"], format!("{:x}", Sha256::digest(bytes)));
    }
    Ok(())
}

#[test]
fn deterministic_bundles_relocate_and_run_after_removing_all_originals() -> anyhow::Result<()> {
    let temp = Temp::new()?;
    let original = temp.0.join("source");
    source(&original)?;
    let a = temp.0.join("a");
    let b = temp.0.join("b");
    bundle_project(&original, &a, &options(true))?;
    bundle_project(&original, &b, &options(true))?;
    let a_tree = tree(&a)?;
    let b_tree = tree(&b)?;
    assert_eq!(a_tree, b_tree);
    std::fs::remove_dir_all(original)?;
    let relocated = temp.0.join("relocated");
    std::fs::rename(a, &relocated)?;
    let run = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["run", "--project"])
        .arg(relocated.join("project.json"))
        .output()?;
    eprintln!("complete relocated CLI run original: {run:#?}");
    assert!(run.status.success());
    let primary: Json = serde_json::from_slice(&std::fs::read(relocated.join("out/result.json"))?)?;
    let named: Json = serde_json::from_slice(&std::fs::read(relocated.join("out/audit.json"))?)?;
    eprintln!("complete primary and named originals: {primary:#?} {named:#?}");
    assert_eq!(
        primary,
        json!({"Value":"Café","Precise":1.0,"Label":"lookup"})
    );
    assert_eq!(named, json!({"Label":"lookup"}));
    assert_eq!(inventory(&relocated)?["includeInputs"], true);
    Ok(())
}

#[test]
fn relocated_declared_design_imports_with_the_generated_catalog_manifest() -> anyhow::Result<()> {
    let temp = Temp::new()?;
    let original = temp.0.join("source");
    source(&original)?;
    write(
        &original,
        "maps/design.mfd",
        include_bytes!("fixtures/offline_bundle/design.mfd"),
    )?;
    write(&original, "maps/input.json", b"{\"Code\":42}\n")?;
    let mut request = options(true);
    request
        .resources
        .extend(["maps/design.mfd".into(), "maps/input.json".into()]);
    // Catalog declaration order differs from artifact path sort order.
    write(&original, "resources/a-string/shared/source.schema.json",
        br#"{"title":"Envelope","type":"object","properties":{"Code":{"type":"string"}},"required":["Code"],"additionalProperties":false}"#)?;
    request
        .resources
        .push("resources/a-string/shared/source.schema.json".into());
    request.catalogs.push(BundleCatalog {
        kind: BundleCatalogKind::JsonSchema,
        root: "resources/a-string".into(),
    });
    let out = temp.0.join("bundle");
    bundle_project(&original, &out, &request)?;
    std::fs::remove_dir_all(original)?;
    let moved = temp.0.join("moved");
    std::fs::rename(out, &moved)?;
    let manifest = mfd::PackageManifest::load(&moved.join("ferrule-package.json"))?;
    assert_eq!(
        manifest.json_schema_catalog_roots(),
        &[
            std::fs::canonicalize(moved.join("resources/json"))?,
            std::fs::canonicalize(moved.join("resources/a-string"))?
        ]
    );
    let imported = moved.join("imported.json");
    let result = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["import-mfd", "--mfd"])
        .arg(moved.join("maps/design.mfd"))
        .args(["--package-manifest"])
        .arg(moved.join("ferrule-package.json"))
        .args(["--require-executable", "--out"])
        .arg(&imported)
        .output()?;
    eprintln!("complete relocated import original: {result:#?}");
    assert!(result.status.success());
    let p = mapping::project_file::decode_bytes(&std::fs::read(&imported)?)?;
    assert!(matches!(
        p.source.child("Code").unwrap().kind,
        ir::SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));
    let result = cli::run_project_with_paths(&imported, None, None);
    eprintln!("complete imported run original: {result:#?}");
    result?;
    let bytes = std::fs::read(moved.join("maps/output.xml"))?;
    let text = std::str::from_utf8(&bytes)?;
    let xml = roxmltree::Document::parse(text)?;
    assert_eq!(xml.root_element().tag_name().name(), "Output");
    assert_eq!(
        xml.root_element()
            .children()
            .filter(|n| n.is_element())
            .map(|n| (n.tag_name().name(), n.text()))
            .collect::<Vec<_>>(),
        vec![("Code", Some("42"))]
    );
    Ok(())
}

#[test]
fn invalid_resource_names_duplicates_and_missing_files_never_publish() -> anyhow::Result<()> {
    let temp = Temp::new()?;
    let original = temp.0.join("source");
    source(&original)?;
    let invalid = [
        "/etc/passwd",
        "../escape",
        "C:/secret",
        "x\\y",
        "a//b",
        "a/./b",
        "CON.json",
        "COM¹.txt",
        "lpt².data",
        "x:y",
        "trail. ",
        "missing.json",
    ];
    for (i, path) in invalid.into_iter().enumerate() {
        let out = temp.0.join(format!("invalid-{i}"));
        let mut request = options(false);
        request.resources.push(path.into());
        let result = bundle_project(&original, &out, &request);
        eprintln!("resource refusal {path:?}: {result:#?}");
        assert!(result.is_err());
        assert!(!out.exists());
    }
    let mut request = options(false);
    request.resources.push(request.resources[0].clone());
    assert!(bundle_project(&original, &temp.0.join("duplicate"), &request).is_err());
    assert!(!temp.0.join("duplicate").exists());
    let remaining = std::fs::read_dir(&temp.0)?
        .map(|e| e.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(remaining, vec!["source"]);
    Ok(())
}

#[test]
fn case_unicode_normalization_and_directory_collisions_refuse() -> anyhow::Result<()> {
    let temp = Temp::new()?;
    for (index, names) in [
        vec!["Alias.txt", "alias.txt"],
        vec!["Upper/a.txt", "upper/b.txt"],
        vec!["é.txt", "e\u{301}.txt"],
        vec!["project.JSON"],
        vec!["project.json/child"],
    ]
    .into_iter()
    .enumerate()
    {
        let original = temp.0.join(format!("source-{index}"));
        source(&original)?;
        let mut request = options(true);
        for name in &names {
            write(&original, name, b"declared")?;
            request.resources.push((*name).into());
        }
        let out = temp.0.join(format!("case-{index}"));
        let result = bundle_project(&original, &out, &request);
        eprintln!("alias refusal {names:?}: {result:#?}");
        assert!(result.is_err());
        assert!(!out.exists());
    }
    Ok(())
}

#[test]
fn all_descendant_symlinks_and_nonregular_resources_refuse() -> anyhow::Result<()> {
    use std::os::unix::fs::symlink;
    let temp = Temp::new()?;
    let original = temp.0.join("source");
    source(&original)?;
    symlink("data/input.json", original.join("contained-link.json"))?;
    symlink(temp.0.join("outside"), original.join("dangling-link"))?;
    write(&temp.0, "outside-secret", b"EXCLUDED OUTSIDE ORIGINAL")?;
    symlink(temp.0.join("outside-secret"), original.join("outside-link"))?;
    symlink("resources/json", original.join("catalog-link"))?;
    std::fs::create_dir(original.join("directory-resource"))?;
    for resource in [
        "contained-link.json",
        "dangling-link",
        "outside-link",
        "catalog-link/shared/source.schema.json",
        "directory-resource",
    ] {
        let mut request = options(false);
        request.resources.push(resource.into());
        let out = temp.0.join("refused");
        let result = bundle_project(&original, &out, &request);
        eprintln!("symlink/nonregular refusal {resource}: {result:#?}");
        assert!(result.is_err());
        assert!(!out.exists());
    }
    let mut request = options(false);
    request.project = "contained-link.json".into();
    assert!(bundle_project(&original, &temp.0.join("project-link"), &request).is_err());
    assert_eq!(
        std::fs::read(temp.0.join("outside-secret"))?,
        b"EXCLUDED OUTSIDE ORIGINAL"
    );
    Ok(())
}

#[test]
fn cancelled_symlink_components_never_substitute_lexical_input_bytes() -> anyhow::Result<()> {
    use std::os::unix::fs::symlink;
    let temp = Temp::new()?;
    let original = temp.0.join("source");
    source(&original)?;
    write(&original, "data/input.json", br#"{"Value":"LEXICAL"}"#)?;
    write(
        &original,
        "alternate/data/input.json",
        br#"{"Value":"TRAVERSED"}"#,
    )?;
    std::fs::create_dir(original.join("alternate/nested"))?;
    symlink("alternate/nested", original.join("link"))?;
    let mut p = project();
    p.source_path = Some("../link/../data/input.json".into());
    let document = original.join("maps/mapper.json");
    write(
        &original,
        "maps/mapper.json",
        mapping::project_file::encode_pretty(&p)?,
    )?;
    // The ordinary runner follows the original traversal, not its lexical
    // normalization. Both candidate files exist and contain different values.
    cli::run_project_with_paths(&document, None, None)?;
    let output: Json =
        serde_json::from_slice(&std::fs::read(original.join("out/result.json"))?)?;
    assert_eq!(output["Value"], "TRAVERSED");
    let lexical: Json =
        serde_json::from_slice(&std::fs::read(original.join("data/input.json"))?)?;
    assert_eq!(lexical["Value"], "LEXICAL");

    for owner in ["primary", "named", "output", "named-output"] {
        let mut p = project();
        match owner {
            "primary" => p.source_path = Some("../link/../data/input.json".into()),
            "named" => p.extra_sources[0].path = "../link/../data/catalog.json".into(),
            "output" => p.target_path = Some("../link/../out/result.json".into()),
            "named-output" => {
                p.extra_targets[0].path = Some("../link/../out/audit.json".into());
            }
            _ => unreachable!(),
        }
        write(
            &original,
            "maps/mapper.json",
            mapping::project_file::encode_pretty(&p)?,
        )?;
        for include_inputs in [false, true] {
            let out = temp.0.join(format!("{owner}-{include_inputs}"));
            let error = bundle_project(&original, &out, &options(include_inputs)).unwrap_err();
            assert!(error.to_string().contains("cancels a normal component"));
            assert!(!out.exists());
        }
    }
    assert_eq!(std::fs::read_dir(&temp.0)?.count(), 1);
    Ok(())
}

#[test]
fn existing_win32_device_aliases_refuse_without_publication() -> anyhow::Result<()> {
    let temp = Temp::new()?;
    let original = temp.0.join("source");
    source(&original)?;
    for name in [
        "CONIN$",
        "conout$.txt",
        "NUL .txt",
        "CON  .json",
        "COM1 .dat",
        "lpt9 .json",
        "COM¹ .txt",
    ] {
        // These are real ordinary files on the producer host, so a missing-file
        // error cannot accidentally satisfy the alias refusal regression.
        let path = format!("resources/{name}");
        write(&original, &path, b"SYNTHETIC DEVICE ALIAS")?;
        let mut request = options(false);
        request.resources.push(path);
        let out = temp.0.join("refused");
        let error = bundle_project(&original, &out, &request).unwrap_err();
        assert!(error.to_string().contains("not a portable file name"));
        assert!(!out.exists());
        let mut p = project();
        p.source_path = Some(format!("../resources/{name}"));
        write(
            &original,
            "maps/mapper.json",
            mapping::project_file::encode_pretty(&p)?,
        )?;
        let error = bundle_project(&original, &out, &options(false)).unwrap_err();
        assert!(error.to_string().contains("not a portable file name"));
        assert!(!out.exists());
        write(
            &original,
            "maps/mapper.json",
            mapping::project_file::encode_pretty(&project())?,
        )?;
    }
    assert_eq!(std::fs::read_dir(&temp.0)?.count(), 1);
    Ok(())
}

#[test]
fn default_data_exclusion_root_escapes_and_limits_refuse_without_publication() -> anyhow::Result<()>
{
    let temp = Temp::new()?;
    let original = temp.0.join("source");
    source(&original)?;
    let mut request = options(false);
    request.resources.push("data/input.json".into());
    assert!(bundle_project(&original, &temp.0.join("input-refused"), &request).is_err());
    let mut p = project();
    p.source_path = Some("../../outside.json".into());
    write(
        &original,
        "maps/mapper.json",
        mapping::project_file::encode_pretty(&p)?,
    )?;
    assert!(bundle_project(&original, &temp.0.join("escape-refused"), &options(true)).is_err());
    let mut p = project();
    p.source_path = Some("/absolute.json".into());
    write(
        &original,
        "maps/mapper.json",
        mapping::project_file::encode_pretty(&p)?,
    )?;
    assert!(bundle_project(&original, &temp.0.join("absolute-refused"), &options(false)).is_err());
    write(
        &original,
        "maps/mapper.json",
        mapping::project_file::encode_pretty(&project())?,
    )?;
    let mut request = options(false);
    request.resources = vec!["never-read".into(); 4094];
    assert!(bundle_project(&original, &temp.0.join("count-refused"), &request).is_err());
    let oversized = std::fs::File::create(original.join("oversized.bin"))?;
    oversized.set_len(64 * 1024 * 1024 + 1)?;
    let mut request = options(false);
    request.resources.push("oversized.bin".into());
    assert!(bundle_project(&original, &temp.0.join("size-refused"), &request).is_err());
    for entry in std::fs::read_dir(&temp.0)? {
        assert_eq!(entry?.file_name(), "source");
    }
    Ok(())
}

#[test]
fn preexisting_destinations_and_dangling_links_remain_exact() -> anyhow::Result<()> {
    use std::os::unix::fs::symlink;
    let temp = Temp::new()?;
    let original = temp.0.join("source");
    source(&original)?;
    let destination = temp.0.join("existing");
    std::fs::create_dir(&destination)?;
    std::fs::write(destination.join("sentinel"), b"UNCHANGED\0\xff")?;
    let before = tree(&destination)?;
    assert!(bundle_project(&original, &destination, &options(true)).is_err());
    assert_eq!(tree(&destination)?, before);
    let file = temp.0.join("existing-file");
    std::fs::write(&file, b"UNCHANGED REGULAR DESTINATION")?;
    assert!(bundle_project(&original, &file, &options(true)).is_err());
    assert_eq!(std::fs::read(file)?, b"UNCHANGED REGULAR DESTINATION");
    let linked = temp.0.join("dangling");
    symlink("not-created", &linked)?;
    assert!(bundle_project(&original, &linked, &options(true)).is_err());
    assert_eq!(std::fs::read_link(linked)?, Path::new("not-created"));
    Ok(())
}

#[test]
fn offline_profile_refuses_dynamic_and_http_sources() -> anyhow::Result<()> {
    let temp = Temp::new()?;
    let original = temp.0.join("source");
    source(&original)?;
    for mode in ["dynamic", "http"] {
        let mut p = project();
        if mode == "dynamic" {
            p.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath {
                node: 0,
                iteration: Vec::new(),
            });
        } else {
            p.source_options.http_get = Some(serde_json::from_str("{}")?);
        }
        write(
            &original,
            "maps/mapper.json",
            mapping::project_file::encode_pretty(&p)?,
        )?;
        let out = temp.0.join(mode);
        let result = bundle_project(&original, &out, &options(true));
        eprintln!("complete {mode} profile refusal: {result:#?}");
        let error = result.unwrap_err().to_string();
        assert!(error.contains("static local sources"), "{error}");
        assert!(!out.exists());
    }
    Ok(())
}

#[test]
fn actual_sqlite_inputs_require_snapshots_even_with_shared_paths() -> anyhow::Result<()> {
    let temp = Temp::new()?;
    let original = temp.0.join("source");
    source(&original)?;
    let database = original.join("data/live.sqlite");
    format_db::write(
        &database,
        &project().source,
        &[ir::Instance::Group(vec![(
            "Value".into(),
            ir::Instance::Scalar(Value::String("SYNTHETIC DATABASE".into())),
        )])],
    )?;
    format_db::write(
        &database,
        &project().extra_sources[0].schema,
        &[ir::Instance::Group(vec![(
            "Label".into(),
            ir::Instance::Scalar(Value::String("SYNTHETIC DATABASE LOOKUP".into())),
        )])],
    )?;
    let before = std::fs::read(&database)?;
    assert!(before.starts_with(b"SQLite format 3\0"));
    for owner in ["primary", "named", "shared"] {
        let mut p = project();
        if owner == "primary" {
            p.source_path = Some("../data/live.sqlite".into());
            p.source_options = FormatOptions::default();
        } else {
            p.extra_sources[0].path = "../data/live.sqlite".into();
            p.extra_sources[0].options = FormatOptions::default();
            if owner == "shared" {
                // A JSON primary sharing the path must not hide the named
                // source's SQLite adapter when the inputs are deduplicated.
                p.source_path = Some("../data/live.sqlite".into());
            }
        }
        write(
            &original,
            "maps/mapper.json",
            mapping::project_file::encode_pretty(&p)?,
        )?;
        let out = temp.0.join(format!("refused-{owner}"));
        let error = bundle_project(&original, &out, &options(true)).unwrap_err();
        assert!(error.to_string().contains("snapshot"), "{error:#}");
        assert!(!out.exists());
        assert_eq!(std::fs::read(&database)?, before);
    }
    assert_eq!(std::fs::read_dir(&temp.0)?.count(), 1);
    Ok(())
}

#[test]
fn json_document_inputs_with_sqlite_suffixes_bundle_and_run() -> anyhow::Result<()> {
    let temp = Temp::new()?;
    let original = temp.0.join("source");
    source(&original)?;
    let mut p = project();
    p.source_path = Some("../data/input.sqlite".into());
    p.extra_sources[0].path = "../data/catalog.db".into();
    let primary = br#"{"Value":"JSON NOT SQLITE"}"#;
    let named = br#"{"Label":"JSON LOOKUP"}"#;
    write(&original, "data/input.sqlite", primary)?;
    write(&original, "data/catalog.db", named)?;
    write(
        &original,
        "maps/mapper.json",
        mapping::project_file::encode_pretty(&p)?,
    )?;
    let out = temp.0.join("bundle");
    let outcome = bundle_project(&original, &out, &options(true))?;
    assert_eq!(outcome.input_files, 2);
    assert_eq!(std::fs::read(out.join("data/input.sqlite"))?, primary);
    assert_eq!(std::fs::read(out.join("data/catalog.db"))?, named);
    std::fs::remove_dir_all(&original)?;
    cli::run_project_with_paths(&out.join("project.json"), None, None)?;
    let output: Json = serde_json::from_slice(&std::fs::read(out.join("out/result.json"))?)?;
    assert_eq!(output["Value"], "JSON NOT SQLITE");
    assert_eq!(output["Label"], "JSON LOOKUP");
    Ok(())
}

#[test]
fn omitted_input_paths_cannot_alias_the_new_bundle_metadata() -> anyhow::Result<()> {
    let temp = Temp::new()?;
    let original = temp.0.join("source");
    source(&original)?;
    for name in [
        "project.json",
        "ferrule-package.json",
        "ferrule-bundle.json",
    ] {
        let mut p = project();
        p.source_path = Some(format!("../{name}"));
        write(
            &original,
            "maps/mapper.json",
            mapping::project_file::encode_pretty(&p)?,
        )?;
        let out = temp.0.join("refused");
        let result = bundle_project(&original, &out, &options(false));
        eprintln!("complete metadata input alias refusal: {result:#?}");
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("collides with bundle metadata")
        );
        assert!(!out.exists());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn deep_paths_have_a_separate_finite_metadata_budget() -> anyhow::Result<()> {
    let temp = Temp::new()?;
    let original = temp.0.join("source");
    source(&original)?;
    let mut request = options(false);
    for i in 0..48 {
        let path = format!(
            "deep-{i:02}/{}/item.json",
            vec!["a".repeat(54); 60].join("/")
        );
        write(&original, &path, b"{}")?;
        request.resources.push(path);
    }
    let out = temp.0.join("metadata-refused");
    let result = bundle_project(&original, &out, &request);
    eprintln!("complete metadata budget refusal: {result:#?}");
    assert!(result.unwrap_err().to_string().contains("metadata budget"));
    assert!(!out.exists());
    Ok(())
}

#[test]
fn cli_exposes_bundle_and_structured_refusals() -> anyhow::Result<()> {
    let help = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["bundle-project", "--help"])
        .output()?;
    assert!(help.status.success());
    assert!(String::from_utf8(help.stdout)?.contains("--include-inputs"));
    let temp = Temp::new()?;
    let original = temp.0.join("source");
    source(&original)?;
    let out = temp.0.join("cli-bundle");
    let result = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .arg("bundle-project")
        .arg("--root")
        .arg(&original)
        .args(["--project", "maps/mapper.json", "--include-inputs", "--out"])
        .arg(&out)
        .output()?;
    eprintln!("complete CLI export original: {result:#?}");
    assert!(result.status.success());
    assert_eq!(inventory(&out)?["includeInputs"], true);
    let refusal = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["--diagnostics", "json", "bundle-project", "--root"])
        .arg(&original)
        .args(["--project", "../escape", "--out"])
        .arg(temp.0.join("absent"))
        .output()?;
    eprintln!("complete CLI refusal original: {refusal:#?}");
    assert!(!refusal.status.success());
    let errors = String::from_utf8(refusal.stderr)?;
    let diagnostics = errors
        .lines()
        .map(serde_json::from_str::<Json>)
        .collect::<Result<Vec<_>, _>>()?;
    assert!(
        diagnostics
            .iter()
            .any(|d| d["command"] == "bundle-project" && d["severity"] == "error")
    );
    assert!(!temp.0.join("absent").exists());
    Ok(())
}
