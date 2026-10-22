use ir::{Instance, Value};
use mapping::{Node, ScopeConstruction};
use std::{
    ops::ControlFlow,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

fn design() -> String {
    let fields = |source: bool| {
        let entries = if source {
            vec![
                ("Order", 11),
                ("Part", 12),
                ("Lot", 13),
                ("Amount", 14),
                ("Supplier", 15),
            ]
        } else {
            vec![
                ("Code", 104),
                ("Total", 102),
                ("Number", 105),
                ("Supplier", 103),
            ]
        };
        let root = if source { "Input" } else { "Output" };
        let role = if source { "outkey" } else { "inpkey" };
        let file = if source { 90 } else { 100 };
        let rows = if source { 10 } else { 101 };
        let body = entries
            .iter()
            .map(|(name, key)| format!("<entry name=\"{name}\" {role}=\"{key}\"/>"))
            .collect::<String>();
        let types = entries
            .iter()
            .enumerate()
            .map(|(index, (name, _))| format!("<field{index} name=\"{name}\" type=\"string\"/>"))
            .collect::<String>();
        format!(
            r#"<component name="{root}" library="text" uid="{}" kind="16"><properties DynamicFilenames="1" {}/><data><root><header><namespaces><namespace/><namespace uid="http://www.altova.com/mapforce"/></namespaces></header><entry name="FileInstance" ns="1" inpkey="{file}"><entry name="document" ns="1"><entry name="Rows" {role}="{rows}">{body}</entry></entry></entry></root><text type="csv" encoding="1000" byteorder="1" byteordermark="0"><settings separator="{}" quote="{}" firstrownames="{}" removeempty="true"><names root="{root}" block="Rows">{types}</names></settings></text></data></component>"#,
            if source { 1 } else { 2 },
            if source {
                ""
            } else {
                "XSLTDefaultOutput=\"1\""
            },
            if source { "," } else { "|" },
            if source { "&quot;" } else { "" },
            source
        )
    };
    let function = |uid,
                    name: &str,
                    library: &str,
                    kind,
                    inputs: &[Option<u32>],
                    outputs: &[Option<u32>],
                    data: &str| {
        let pins = |pins: &[Option<u32>]| {
            pins.iter()
                .enumerate()
                .map(|(index, pin)| {
                    pin.map_or_else(
                        || "<datapoint/>".into(),
                        |key| format!("<datapoint pos=\"{index}\" key=\"{key}\"/>"),
                    )
                })
                .collect::<String>()
        };
        format!(
            "<component name=\"{name}\" library=\"{library}\" uid=\"{uid}\" kind=\"{kind}\"><properties/><sources>{}</sources><targets>{}</targets>{data}</component>",
            pins(inputs),
            pins(outputs)
        )
    };
    let mut components = vec![fields(true), fields(false)];
    components.push(function(3,"InputFile","core",6,&[None],&[Some(200)],r#"<data><input datatype="string" previewvalue="preview.csv" usepreviewvalue="1"/><parameter usageKind="input" name="InputFile"/></data>"#));
    for (uid, key, value) in [
        (4, 201, "fixed-"),
        (5, 202, ".txt"),
        (6, 203, "fallback"),
        (7, 204, "|"),
    ] {
        components.push(function(
            uid,
            "constant",
            "core",
            2,
            &[],
            &[Some(key)],
            &format!("<data><constant datatype=\"string\" value=\"{value}\"/><wsdl/></data>"),
        ));
    }
    components.extend([
        function(
            8,
            "group-by",
            "core",
            5,
            &[Some(301), Some(302)],
            &[None, Some(210)],
            "",
        ),
        function(
            9,
            "group-by",
            "core",
            5,
            &[Some(303), Some(304)],
            &[Some(220), None],
            "",
        ),
        function(
            10,
            "group-by",
            "core",
            5,
            &[Some(305), Some(306)],
            &[None, Some(230)],
            "",
        ),
        function(
            11,
            "concat",
            "core",
            5,
            &[Some(307), Some(308)],
            &[Some(240)],
            "",
        ),
        function(12, "sum", "core", 5, &[None, Some(309)], &[Some(250)], ""),
        function(
            13,
            "auto-number",
            "core",
            5,
            &[None, None, None, None],
            &[Some(260)],
            "",
        ),
        function(
            14,
            "substitute-null",
            "db",
            5,
            &[Some(310), Some(311)],
            &[Some(270)],
            "",
        ),
        function(
            15,
            "concat",
            "core",
            5,
            &[Some(312), Some(313)],
            &[Some(280)],
            "",
        ),
        function(
            16,
            "concat",
            "core",
            5,
            &[Some(314), Some(315), Some(316)],
            &[Some(290)],
            "",
        ),
    ]);
    let links = [
        (200, 90),
        (290, 100),
        (10, 301),
        (11, 302),
        (10, 303),
        (240, 304),
        (10, 305),
        (12, 306),
        (12, 307),
        (13, 308),
        (14, 309),
        (15, 310),
        (203, 311),
        (270, 312),
        (204, 313),
        (201, 314),
        (210, 315),
        (202, 316),
        (220, 101),
        (230, 104),
        (250, 102),
        (260, 105),
        (280, 103),
    ];
    let vertices = links
        .iter()
        .map(|(from, to)| {
            format!(
                "<vertex vertexkey=\"{from}\"><edges><edge vertexkey=\"{to}\"/></edges></vertex>"
            )
        })
        .collect::<String>();
    format!(
        "<mapping version=\"26\"><component uid=\"999\" name=\"mapping\"><properties SelectedLanguage=\"builtin\"/><structure><children>{}</children><graph directed=\"1\"><vertices>{vertices}</vertices></graph></structure></component></mapping>",
        components.concat()
    )
}

fn with_design<T>(text: &str, run: impl FnOnce(&Path) -> T) -> T {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let folder = std::env::temp_dir().join(format!(
        "ferrule-grouped-csv-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&folder).unwrap();
    let path = folder.join("mapping.mfd");
    std::fs::write(&path, text).unwrap();
    let result = run(&path);
    if std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").is_none() {
        std::fs::remove_dir_all(folder).unwrap();
    }
    result
}

fn input(rows: &[(&str, &str, &str, &str, Option<&str>)]) -> Instance {
    Instance::Repeated(
        rows.iter()
            .map(|(order, part, lot, amount, supplier)| {
                let mut fields: Vec<(String, Value)> = vec![
                    ("Order".into(), Value::String((*order).into())),
                    ("Part".into(), Value::String((*part).into())),
                    ("Lot".into(), Value::String((*lot).into())),
                    ("Amount".into(), Value::String((*amount).into())),
                ];
                if let Some(supplier) = supplier {
                    fields.push(("Supplier".into(), Value::String((*supplier).into())));
                }
                Instance::Group(
                    fields
                        .into_iter()
                        .map(|(key, value)| (key, Instance::Scalar(value)))
                        .collect::<Vec<_>>()
                        .into(),
                )
            })
            .collect(),
    )
}

#[test]
fn imports_complete_grouped_csv_without_opening_preview_files() {
    with_design(&design(), |path| {
        let imported = mfd::import_grouped_csv_document(path).unwrap();
        assert!(imported.warnings.is_empty());
        assert!(matches!(
            imported.document.global_filename().unwrap(),
            mapping::GlobalFilename::Parameter("InputFile")
        ));
        let project = imported.document.project();
        assert_eq!(project.root.construction, ScopeConstruction::DocumentRows);
        assert_eq!(
            project.root.children[0]
                .bindings
                .iter()
                .map(|binding| binding.target_field.as_str())
                .collect::<Vec<_>>(),
            ["Code", "Total", "Number", "Supplier"]
        );
        assert!(
            !project
                .graph
                .nodes
                .values()
                .any(|node| matches!(node, Node::Position { .. }))
        );
        let source = input(&[
            ("A", "AB", "C", "1", Some("v1")),
            ("B", "z", "L", "5", Some("vB")),
            ("A", "A", "BC", "2", None),
            ("A", "AB", "C", "3", Some("v3")),
        ]);
        let context = engine::ExecutionContext::new(path).with_invocation_counter_limits(
            engine::InvocationCounterLimits {
                max_counters: 1,
                max_text_bytes: 128,
            },
        );
        let mut documents = vec![];
        let result = engine::visit_target_documents(
            project,
            &source,
            vec![],
            &context,
            engine::TargetSelection::Primary,
            |document| {
                let Instance::Repeated(rows) = document.value() else {
                    panic!("not rows")
                };
                let text = format_csv::to_string_with_options(
                    &project.target,
                    rows,
                    &format_csv::CsvWriteOptions {
                        delimiter: Some('|'),
                        quote_disabled: true,
                        raw_delimited_crlf: true,
                        has_headers: false,
                        ..Default::default()
                    },
                )
                .unwrap();
                documents.push((document.path().to_owned(), text));
                ControlFlow::<()>::Continue(())
            },
        )
        .unwrap();
        assert_eq!(result, engine::DocumentVisit::Complete { accepted: 2 });
        assert_eq!(
            documents,
            [
                ("fixed-A.txt".into(), "A|6|1|v3|\r\n".into()),
                ("fixed-B.txt".into(), "z|5|2|vB|\r\n".into())
            ]
        );
        codegen::lower(project).unwrap();
    });
}

#[test]
fn ordinary_import_keeps_its_filename_refusal() {
    with_design(&design(), |path| {
        assert!(
            mfd::import_with_profile(path, &Default::default(), mfd::ImportProfile::Executable)
                .is_err()
        )
    });
}

#[test]
fn malformed_pin_order_is_not_normalized_into_the_new_profile() {
    let text = design().replace("pos=\"1\" key=\"308\"", "pos=\"0\" key=\"308\"");
    with_design(&text, |path| {
        assert!(mfd::import_grouped_csv_document(path).is_err())
    });
}

#[test]
fn unknown_function_metadata_is_refused() {
    let text = design().replace(
        "name=\"auto-number\"",
        "unsupported=\"1\" name=\"auto-number\"",
    );
    with_design(&text, |path| {
        assert!(mfd::import_grouped_csv_document(path).is_err())
    });
}

#[test]
fn unqualified_versions_and_exception_profiles_stay_closed() {
    with_design(
        &design().replace("version=\"26\"", "version=\"25\""),
        |path| assert!(mfd::import_grouped_csv_document(path).is_err()),
    );
    with_design(&design(), |path| {
        let options = mfd::ImportOptions::default().with_item_ordered_exceptions();
        assert!(mfd::import_grouped_csv_document_with_options(path, &options).is_err());
    });
}

#[test]
fn grouped_key_preserves_missing_sequence_and_present_empty_text() {
    let functions = r#"
<component name="constant" library="core" uid="30" kind="2"><sources/><targets><datapoint pos="0" key="4000"/></targets><data><constant datatype="string" value="."/></data></component>
<component name="right-trim" library="lang" uid="31" kind="5"><sources><datapoint pos="0" key="4101"/></sources><targets><datapoint pos="0" key="4001"/></targets></component>
<component name="empty" library="lang" uid="32" kind="5"><sources><datapoint pos="0" key="4102"/></sources><targets><datapoint pos="0" key="4002"/></targets></component>
<component name="substring-before" library="core" uid="33" kind="5"><sources><datapoint pos="0" key="4103"/><datapoint pos="1" key="4104"/></sources><targets><datapoint pos="0" key="4003"/></targets></component>
<component name="if-else" library="core" uid="34" kind="4"><sources><datapoint pos="0" key="4105"/><datapoint pos="1" key="4106"/><datapoint pos="2" key="4107"/></sources><targets><datapoint pos="0" key="4004"/></targets></component>
<component name="substitute-null" library="db" uid="35" kind="5"><sources><datapoint pos="0" key="4108"/><datapoint pos="1" key="4109"/></sources><targets><datapoint pos="0" key="4005"/></targets></component>
"#;
    let edges = [
        (11, 4101),
        (4001, 4102),
        (15, 4103),
        (4000, 4104),
        (4002, 4105),
        (4003, 4106),
        (11, 4107),
        (4004, 4108),
        (15, 4109),
    ];
    let vertices = edges
        .iter()
        .map(|(from, to)| {
            format!(
                "<vertex vertexkey=\"{from}\"><edges><edge vertexkey=\"{to}\"/></edges></vertex>"
            )
        })
        .collect::<String>();
    let text = design()
        .replace("</children>", &format!("{functions}</children>"))
        .replace(
            "vertexkey=\"11\"><edges><edge vertexkey=\"302\"",
            "vertexkey=\"4005\"><edges><edge vertexkey=\"302\"",
        )
        .replace("</vertices>", &format!("{vertices}</vertices>"));
    with_design(&text, |path| {
        let imported = mfd::import_grouped_csv_document(path).unwrap();
        for (order, fallback, expected) in [
            (None, Some("PO.1"), Some("fixed-PO.1.txt")),
            (Some("   "), Some("PO.1"), Some("fixed-PO.txt")),
            (Some(""), Some("PO.1"), Some("fixed-PO.txt")),
            (Some("REF  "), None, Some("fixed-REF  .txt")),
            (None, None, None),
            (Some("   "), None, None),
            (Some("   "), Some("PO"), Some("fixed-.txt")),
        ] {
            let mut fields = vec![
                ("Part".into(), Instance::Scalar(Value::String("X".into()))),
                ("Lot".into(), Instance::Scalar(Value::String("L".into()))),
                ("Amount".into(), Instance::Scalar(Value::String("1".into()))),
            ];
            if let Some(value) = order {
                fields.push((
                    "Order".into(),
                    Instance::Scalar(Value::String(value.into())),
                ));
            }
            if let Some(value) = fallback {
                fields.push((
                    "Supplier".into(),
                    Instance::Scalar(Value::String(value.into())),
                ));
            }
            let source = Instance::Repeated(vec![Instance::Group(fields.into())]);
            let context = engine::ExecutionContext::new(path).with_invocation_counter_limits(
                engine::InvocationCounterLimits {
                    max_counters: 1,
                    max_text_bytes: 128,
                },
            );
            let mut names = vec![];
            let result = engine::visit_target_documents(
                imported.document.project(),
                &source,
                vec![],
                &context,
                engine::TargetSelection::Primary,
                |document| {
                    names.push(document.path().to_owned());
                    ControlFlow::<()>::Continue(())
                },
            )
            .unwrap();
            assert_eq!(
                result,
                engine::DocumentVisit::Complete {
                    accepted: usize::from(expected.is_some())
                }
            );
            assert_eq!(
                names,
                expected.into_iter().map(str::to_owned).collect::<Vec<_>>(),
                "{order:?} / {fallback:?}"
            );
        }
    });
}

#[test]
fn fixed_column_does_not_collapse_independent_source_sequences() {
    let text = design().replace(
        "vertexkey=\"204\"><edges><edge vertexkey=\"313\"",
        "vertexkey=\"14\"><edges><edge vertexkey=\"313\"",
    );
    with_design(&text, |path| {
        assert!(mfd::import_grouped_csv_document(path).is_err())
    });
}

/// Explicit local integration hook. Normal test runs never search for private
/// designs or resources; the caller supplies both paths and the writer policy.
#[test]
#[ignore = "requires explicit FERRULE_GROUPED_CSV_MFD and FERRULE_GROUPED_CSV_OUTPUT"]
fn supplied_grouped_document_generates_both_languages() -> Result<(), Box<dyn std::error::Error>> {
    use sha2::{Digest, Sha256};
    let source = std::path::PathBuf::from(
        std::env::var_os("FERRULE_GROUPED_CSV_MFD").ok_or("missing explicit MFD path")?,
    );
    let output = std::path::PathBuf::from(
        std::env::var_os("FERRULE_GROUPED_CSV_OUTPUT")
            .ok_or("missing explicit output directory")?,
    );
    let before = std::fs::read(&source)?;
    let imported = mfd::import_grouped_csv_document(&source)?;
    assert!(imported.warnings.is_empty());
    std::fs::create_dir(&output)?;
    std::fs::write(
        output.join("document.json"),
        serde_json::to_vec_pretty(&imported.document)?,
    )?;
    std::fs::write(
        output.join("project.json"),
        serde_json::to_vec_pretty(imported.document.project())?,
    )?;
    let program = codegen::lower(imported.document.project())?;
    let mut policy =
        codegen::CsvOutputPolicy::from_format_options(&imported.document.project().target_options)?;
    if std::env::var("FERRULE_GROUPED_CSV_RAW_CRLF").as_deref() == Ok("1") {
        policy.raw_delimited_crlf = true;
    }
    let boundary = codegen::CsvDocumentBoundaryPolicy {
        source: codegen::CsvInputPolicy::from_input_format_options(
            &imported.document.project().source_options,
        )?,
        target: policy,
    };
    let runtime = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
    let rust = codegen_rust::emit_with_csv_document_set_boundary(
        &program,
        &codegen_rust::Options {
            package_name: "original_mapping".into(),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                runtime.to_string_lossy().into_owned(),
            ),
        },
        &boundary,
    )?;
    let csharp = codegen_csharp::emit_with_csv_document_set_boundary(&program, &boundary)?;
    for (language, artifacts) in [("rust", rust), ("csharp", csharp)] {
        for file in artifacts.files() {
            let path = output.join(language).join(file.path.as_str());
            std::fs::create_dir_all(path.parent().ok_or("missing artifact parent")?)?;
            std::fs::write(path, &file.contents)?;
        }
    }
    assert_eq!(std::fs::read(source)?, before);
    std::fs::write(
        output.join("import-report.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "original_sha256":format!("{:x}",Sha256::digest(&before)),"source_unchanged":true,
            "codegen_languages":["rust","csharp"],"original_completion_credit":false,
        }))?,
    )?;
    Ok(())
}
