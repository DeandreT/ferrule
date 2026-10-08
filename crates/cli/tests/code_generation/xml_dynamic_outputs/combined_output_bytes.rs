//! Opt-in physical primary document-list bytes; no production or filesystem publication claim.
use super::super::*;
use serde_json::{Value as Json, json};
use std::time::{Duration, Instant};

#[path = "combined_output_bytes_support.rs"]
mod support;
use support::*;

const M: usize = 64 * 1024 * 1024;
const H: usize = 14; // Independent literal <O><T> + </T></O>, not measured writer output.
static SERIAL: Mutex<()> = Mutex::new(());

fn scalar(name: &str, ty: ScalarType) -> SchemaNode {
    let mut value = SchemaNode::scalar(name, ty);
    value.xml_namespace = Some(ir::XmlNamespace::Unqualified);
    value
}
fn group(name: &str, children: Vec<SchemaNode>) -> SchemaNode {
    let mut value = SchemaNode::group(name, children);
    value.xml_namespace = Some(ir::XmlNamespace::Unqualified);
    value
}
fn project() -> Project {
    let mut rows = group(
        "R",
        vec![
            scalar("P", ScalarType::String),
            scalar("N", ScalarType::Int),
        ],
    );
    rows.repeating = true;
    Project {
        source: group("I", vec![scalar("B", ScalarType::String), rows]),
        target: group("O", vec![scalar("T", ScalarType::String)]),
        source_path: None,
        target_path: None,
        source_options: mapping::FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        target_options: mapping::FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["P".into()],
                        frame: Some(vec!["R".into()]),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["B".into()],
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![1, 1],
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    4,
                    Node::SourceField {
                        path: vec!["N".into()],
                        frame: Some(vec!["R".into()]),
                    },
                ),
                (
                    5,
                    Node::Call {
                        function: "substring".into(),
                        args: vec![2, 3, 4],
                    },
                ),
            ]),
        },
        root: Scope {
            target_field: "O".into(),
            iteration: ScopeIteration::DynamicDocuments {
                source: vec!["R".into()],
                output_path: 0,
            },
            bindings: vec![Binding {
                target_field: "T".into(),
                node: 5,
            }],
            ..Default::default()
        },
    }
}
fn small_instance(text: &str) -> Instance {
    Instance::Group(vec![("T".into(), Instance::Scalar(Value::String(text.into())))].into())
}
fn input(blob: &str, text_lengths: [usize; 5]) -> String {
    let mut value = format!("<I><B>{blob}</B>");
    for length in text_lengths {
        value.push_str(&format!("<R><P>same.xml</P><N>{length}</N></R>"));
    }
    value.push_str("</I>");
    value
}
fn prepare(directory: &Path, language: &str) -> TestResult<PathBuf> {
    let project = project();
    let validation = engine::validate(&project);
    let lowered = codegen::lower(&project);
    std::fs::write(
        directory.join("original-project-validation-lowering.txt"),
        format!("{project:#?}\n{validation:#?}\n{lowered:#?}\n"),
    )?;
    assert!(validation.is_empty());
    let mut program = lowered?;
    let before = format!("{program:#?}\n");
    std::fs::write(
        directory.join("original-lowered-before-policy.txt"),
        &before,
    )?;
    assert_eq!(
        program.xml_output_mode()?,
        Some(codegen::XmlOutputMode::DynamicPrimaryDocuments)
    );
    let policy = program
        .xml_boundary
        .as_mut()
        .ok_or("explicit XML policy missing")?;
    assert!(policy.output.declaration && policy.output.indent);
    assert!(policy.output.default_namespace.is_none() && policy.output.schema_hints.is_none());
    policy.output.declaration = false;
    policy.output.indent = false;
    let valid = codegen::validate_program(&program);
    std::fs::write(
        directory.join("original-lowered-after-policy-validation.txt"),
        format!("{program:#?}\n{valid:#?}\n"),
    )?;
    valid?;
    assert!(program.extra_sources.is_empty() && program.extra_targets.is_empty());
    assert!(program.failure_rules.is_empty());
    let options = format_xml::XmlWriteOptions {
        declaration: false,
        indent: false,
        ..Default::default()
    };
    let writer =
        format_xml::to_string_with_options(&project.target, &small_instance("abc"), &options);
    std::fs::write(
        directory.join("original-independent-small-writer-result.txt"),
        format!("{writer:#?}\n"),
    )?;
    let writer = writer?;
    std::fs::write(
        directory.join("original-independent-small-writer.xml"),
        &writer,
    )?;
    assert_eq!(writer, "<O><T>abc</T></O>");
    assert_eq!(H, "<O><T></T></O>".len());
    const { assert!(H < 64 && 64 - H > 0) };
    let small = input("abcd", [1, 2, 3, 4, 5]);
    let parsed = format_xml::from_str(&small, &project.source);
    std::fs::write(
        directory.join("original-small-parse.txt"),
        format!("{parsed:#?}\n"),
    )?;
    let mapped = engine::run_outputs(&project, &parsed?);
    std::fs::write(
        directory.join("original-small-engine-result.txt"),
        format!("{mapped:#?}\n"),
    )?;
    let mapped = mapped?;
    let expected = ["a", "ab", "abc", "abcd", "abcda"]
        .into_iter()
        .map(|text| {
            ir::DocumentMember::new("same.xml", small_instance(text))
                .ok_or("manual path must be nonempty")
        })
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(mapped.primary, Instance::DocumentSet(expected));
    assert!(mapped.extras.is_empty());
    std::fs::write(directory.join("small.xml"), small)?;
    let encoded = mapping::project_file::encode_pretty(&project);
    std::fs::write(
        directory.join("original-project-codec.txt"),
        format!("{encoded:#?}\n"),
    )?;
    std::fs::write(directory.join("project.json"), encoded?)?;
    for (name, schema) in [("source", &project.source), ("target", &project.target)] {
        let encoded =
            codegen::serialize_embedded_schema(schema, codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES);
        std::fs::write(
            directory.join(format!("original-{name}-schema-result.txt")),
            format!("{encoded:#?}\n"),
        )?;
        std::fs::write(directory.join(format!("{name}-schema.txt")), encoded?)?;
    }
    let artifacts = if language == "rust" {
        let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../codegen-runtime")
            .canonicalize()?;
        let emitted = codegen_rust::emit(
            &program,
            &codegen_rust::Options {
                package_name: "ferrule-generated-mapping".into(),
                runtime_dependency: codegen_rust::RuntimeDependency::Path(
                    runtime.to_string_lossy().into_owned(),
                ),
            },
        );
        std::fs::write(
            directory.join("original-public-emission.txt"),
            format!("{emitted:#?}\n"),
        )?;
        emitted?
    } else {
        let emitted = codegen_csharp::emit(&program);
        std::fs::write(
            directory.join("original-public-emission.txt"),
            format!("{emitted:#?}\n"),
        )?;
        emitted?
    };
    let generated = directory.join("generated");
    std::fs::create_dir(&generated)?;
    for file in artifacts.files() {
        let path = generated.join(file.path.as_str());
        std::fs::create_dir_all(path.parent().ok_or("artifact parent")?)?;
        std::fs::write(path, &file.contents)?;
    }
    Ok(generated)
}
fn verify_observation(folder: &Path, case: &str, api: usize) -> TestResult<()> {
    let original = std::fs::read(folder.join("original-result.json"))?;
    let row: Json = serde_json::from_slice(&original)?;
    assert_eq!(
        (row["case"].as_str(), row["api"].as_u64()),
        (Some(case), Some(api as u64))
    );
    if case == "plus-one" {
        assert_eq!(row["result"], "Err");
        assert_eq!(row["returned_documents"], Json::Null);
        assert_eq!(row["kind"], "Output");
        assert_eq!(
            row["member"],
            json!({"target":"Primary","index":4,"path":"same.xml"})
        );
        assert_eq!(
            (row["bytes"].clone(), row["limit"].clone()),
            (Json::Null, Json::Null)
        );
        assert_eq!(row["resource"], "xml_output_set_utf8_bytes");
        assert_eq!(
            (row["observed"].as_u64(), row["resource_limit"].as_u64()),
            (Some(268435457), Some(268435456))
        );
        assert_eq!(row["original_boundary_identity"], true);
        assert_eq!(row["typed_original_cause"], true);
    } else {
        assert_eq!(row["result"], "Ok");
        assert_eq!(
            row["paths"],
            json!(["same.xml", "same.xml", "same.xml", "same.xml", "same.xml"])
        );
        let sizes = if case == "small" {
            vec![15, 16, 17, 18, 19]
        } else {
            vec![M, M, M, M - 64, 64]
        };
        assert_eq!(row["sizes"], json!(sizes));
        assert_eq!(row["all_literal_documents_match"], true);
    }
    Ok(())
}
fn exercise(language: &str, large: bool) -> TestResult<()> {
    let _serial = SERIAL
        .lock()
        .map_err(|_| io::Error::other("combined output test lock poisoned"))?;
    if large {
        assert_eq!(
            std::env::var_os("FERRULE_CODEGEN_LARGE_XML_OUTPUTS").as_deref(),
            Some(OsStr::new("1"))
        );
        let target = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
            .ok_or("large tests need the shared HOST target")?;
        assert!(
            Path::new(&target).is_absolute(),
            "large runs use one explicitly bound absolute shared HOST target"
        );
    }
    let directory = Originals::new(language, large)?;
    let deadline = Instant::now() + Duration::from_secs(if large { 1800 } else { 600 });
    let before_sources = source_identities()?;
    json_file(
        &directory.path.join("complete-source-before.json"),
        &before_sources,
    )?;
    let result = caught(&directory.path, || {
        let mut command = Command::new(gnu_time()?);
        command.arg("--version");
        let output = recorded(
            &mut command,
            &directory.path,
            "time-identity",
            deadline,
            30,
            false,
        )?;
        require_success(&output);
        if large {
            headroom(&directory.path, deadline, "before-build")?;
        }
        let generated = prepare(&directory.path, language)?;
        let before_library = library_identities(&generated)?;
        json_file(
            &directory.path.join("complete-generated-before.json"),
            &before_library,
        )?;
        let body = caught(&directory.path, || {
            let host = build(&directory.path, language, &generated, deadline)?;
            let host_before = host_identities(&host, language)?;
            json_file(
                &directory.path.join("complete-host-before.json"),
                &host_before,
            )?;
            let mut cases = vec!["small"];
            if large {
                let blob = "x".repeat(32 * 1024 * 1024);
                for (case, last) in [("exact", 64), ("plus-one", 65)] {
                    let xml = input(&blob, [M - H, M - H, M - H, M - 64 - H, last - H]);
                    std::fs::write(directory.path.join(format!("{case}.xml")), &xml)?;
                    json_file(
                        &directory.path.join(format!("{case}-input-original.json")),
                        &identity(&directory.path.join(format!("{case}.xml")))?,
                    )?;
                    assert!(xml.len() < 33 * 1024 * 1024);
                }
                cases.extend(["exact", "plus-one"]);
            }
            let corpus_before = corpus_identities(&directory.path)?;
            json_file(
                &directory.path.join("complete-corpus-before.json"),
                &corpus_before,
            )?;
            let calls = caught(&directory.path, || {
                for case in cases {
                    for api in 0..4 {
                        if large {
                            headroom(
                                &directory.path,
                                deadline,
                                &format!("{case}-{api}-before-host"),
                            )?;
                        }
                        let folder = directory.path.join(format!("observations-{case}-{api}"));
                        std::fs::create_dir(&folder)?;
                        let mut command = host_command(&host, language, &directory.path);
                        command.arg(&directory.path).arg(case).arg(api.to_string());
                        let output = recorded(
                            &mut command,
                            &directory.path,
                            &format!("host-{case}-{api}"),
                            deadline,
                            300,
                            true,
                        )?;
                        require_success(&output);
                        verify_observation(&folder, case, api)?;
                    }
                }
                Ok(())
            });
            std::fs::write(
                directory.path.join("original-call-loop-result.txt"),
                format!("{calls:#?}\n"),
            )?;
            let host_after = host_identities(&host, language)?;
            json_file(
                &directory.path.join("complete-host-after.json"),
                &host_after,
            )?;
            let corpus_after = corpus_identities(&directory.path)?;
            json_file(
                &directory.path.join("complete-corpus-after.json"),
                &corpus_after,
            )?;
            assert_eq!(
                host_after, host_before,
                "actual host binary/closure unchanged even on ordinary panic"
            );
            assert_eq!(
                corpus_after, corpus_before,
                "full input/project/schema corpus unchanged even on ordinary panic"
            );
            calls
        });
        std::fs::write(
            directory
                .path
                .join("original-build-and-call-body-result.txt"),
            format!("{body:#?}\n"),
        )?;
        let after_library = library_identities(&generated)?;
        json_file(
            &directory.path.join("complete-generated-after.json"),
            &after_library,
        )?;
        assert_eq!(
            after_library, before_library,
            "all generated library bodies unchanged even on ordinary panic"
        );
        body
    });
    std::fs::write(
        directory.path.join("original-generation-body-result.txt"),
        format!("{result:#?}\n"),
    )?;
    let after_sources = source_identities()?;
    json_file(
        &directory.path.join("complete-source-after.json"),
        &after_sources,
    )?;
    assert_eq!(
        after_sources, before_sources,
        "all reached source/normal test and CLI identities unchanged even on ordinary panic"
    );
    result
}

#[test]
fn generated_rust_combined_xml_output_small_literal_documents() -> TestResult<()> {
    exercise("rust", false)
}
#[test]
fn generated_csharp_combined_xml_output_small_literal_documents() -> TestResult<()> {
    exercise("csharp", false)
}
#[test]
#[ignore = "opt-in serial physical 256MiB/+1 XML output qualification; set FERRULE_CODEGEN_LARGE_XML_OUTPUTS=1"]
fn generated_rust_combined_xml_output_exact_and_plus_one_final_member() -> TestResult<()> {
    exercise("rust", true)
}
#[test]
#[ignore = "opt-in serial physical 256MiB/+1 XML output qualification; set FERRULE_CODEGEN_LARGE_XML_OUTPUTS=1"]
fn generated_csharp_combined_xml_output_exact_and_plus_one_final_member() -> TestResult<()> {
    exercise("csharp", true)
}
