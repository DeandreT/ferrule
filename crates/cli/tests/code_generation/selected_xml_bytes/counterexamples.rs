//! Additional physical calls for the independent-review counterexamples.
use super::*;
use std::io::{Read, Write};

const MIB: usize = 1024 * 1024;
const DOCUMENT: usize = 64 * MIB;
const AGGREGATE: usize = 256 * MIB;
const SMALL: &str = "<Document><Check><Reject>false</Reject></Check><Tag>alpha</Tag><Tag></Tag><Tag>omega😀</Tag><Maybe xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:nil=\"true\"/><Maybe>tail</Maybe></Document>";
const GUARD: &str = "<Guard><Text>blocked-by-global-rule</Text></Guard>";
static SERIAL: Mutex<()> = Mutex::new(());

fn small_project() -> Project {
    let mut project = super::project("good");
    let schema = SchemaNode::group(
        "Document",
        vec![
            SchemaNode::group("Check", vec![bool_("Reject")]).repeating(),
            string("Tag").repeating(),
            string("Maybe").nillable().repeating(),
        ],
    );
    project.source = schema.clone();
    project.target = schema.clone();
    project.extra_sources = vec![
        mapping::NamedSource {
            name: "guard".into(),
            path: "never-open-guard.xml".into(),
            schema: SchemaNode::group("Guard", vec![string("Text")]),
            options: options(),
            dynamic_path: None,
        },
        mapping::NamedSource {
            name: "unused".into(),
            path: "never-open-unused.xml".into(),
            schema: SchemaNode::group("Unused", vec![string("Value")]),
            options: options(),
            dynamic_path: None,
        },
    ];
    project.root = Scope {
        target_field: "Document".into(),
        construction: mapping::ScopeConstruction::CopyCurrentSource,
        ..Default::default()
    };
    project.extra_targets = vec![mapping::NamedTarget {
        name: "named".into(),
        path: None,
        schema,
        options: options(),
        root: project.root.clone(),
    }];
    project.graph.nodes = BTreeMap::from([
        (
            1,
            Node::SourceField {
                path: vec!["Reject".into()],
                frame: Some(vec!["Check".into()]),
            },
        ),
        (2, field(&["guard", "Text"])),
    ]);
    project.failure_rules = vec![mapping::FailureRule {
        iteration: mapping::FailureIteration::Source {
            collection: vec!["Check".into()],
        },
        selection: mapping::FailureSelection::WhenTrue { predicate: 1 },
        message: Some(2),
    }];
    project
}

fn aggregate_project() -> Project {
    let mut project = super::project("good");
    project.source = SchemaNode::group("Envelope", vec![string("Pad")]);
    project.target = SchemaNode::group("Output", ["A", "B", "C", "D"].map(string).to_vec());
    project.extra_sources = ["a", "b", "c", "d"]
        .into_iter()
        .map(|name| mapping::NamedSource {
            name: name.into(),
            path: format!("never-open-{name}.xml"),
            schema: SchemaNode::group("Data", vec![string("Token"), string("Pad")]),
            options: options(),
            dynamic_path: None,
        })
        .collect();
    project.graph.nodes = ["a", "b", "c", "d"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| ((index + 1) as u32, field(&[name, "Token"])))
        .collect();
    project.root = Scope {
        target_field: "Output".into(),
        bindings: ["A", "B", "C", "D"]
            .into_iter()
            .enumerate()
            .map(|(index, name)| bind(name, (index + 1) as u32))
            .collect(),
        ..Default::default()
    };
    project.extra_targets = vec![mapping::NamedTarget {
        name: "named".into(),
        path: None,
        schema: project.target.clone(),
        options: options(),
        root: project.root.clone(),
    }];
    project.failure_rules.clear();
    project
}

fn padded(path: &Path, prefix: &str, suffix: &str, length: usize) -> TestResult<()> {
    let mut file = std::io::BufWriter::new(
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?,
    );
    file.write_all(prefix.as_bytes())?;
    let mut remaining = length
        .checked_sub(prefix.len() + suffix.len())
        .ok_or("short padded fixture")?;
    let block = [b'x'; 65536];
    while remaining != 0 {
        let count = remaining.min(block.len());
        file.write_all(&block[..count])?;
        remaining -= count;
    }
    file.write_all(suffix.as_bytes())?;
    file.flush()?;
    assert_eq!(file.get_ref().metadata()?.len(), length as u64);
    Ok(())
}

fn identity(path: &Path) -> TestResult<Json> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    let mut length = 0usize;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        length += read;
        hash.update(&buffer[..read]);
    }
    Ok(json!({"path":path,"bytes":length,"sha256":format!("{:x}",hash.finalize())}))
}

fn corpus(root: &Path, large: bool) -> TestResult<(Vec<Json>, Vec<PathBuf>)> {
    let inputs = root.join("inputs");
    std::fs::create_dir(&inputs)?;
    let mut paths = Vec::new();
    let mut write = |name: &str, bytes: &[u8]| -> TestResult<PathBuf> {
        let path = inputs.join(name);
        std::fs::write(&path, bytes)?;
        paths.push(path.clone());
        Ok(path)
    };
    let mut rows = vec![];
    if !large {
        let pass = write("pass.xml", SMALL.as_bytes())?;
        let fail = write("fail.xml", SMALL.replace("false", "true").as_bytes())?;
        let guard = write("guard.xml", GUARD.as_bytes())?;
        let unused = write("unused.xml", b"<Unused><Value>not-read</Value></Unused>")?;
        let malformed = write("malformed.xml", b"\xff<not-XML")?;
        for target in ["primary", "named", "all"] {
            let named = if target == "all" {
                vec![
                    json!({"name":"guard","path":guard}),
                    json!({"name":"unused","path":unused}),
                ]
            } else {
                vec![json!({"name":"guard","path":guard})]
            };
            rows.push(json!({"case":format!("scalar-{target}"),"target":target,"source":pass,"inputs":named,"expected":"ok"}));
            rows.push(json!({"case":format!("global-{target}"),"target":target,"source":fail,"inputs":named,"expected":"global"}));
        }
        rows.push(json!({"case":"unused-malformed","target":"primary","source":pass,
            "inputs":[{"name":"unused","path":malformed},{"name":"guard","path":guard}],"expected":"unexpected"}));
        rows.push(json!({"case":"global-unused-malformed","target":"named","source":fail,
            "inputs":[{"name":"guard","path":guard},{"name":"unused","path":malformed}],"expected":"unexpected"}));
        rows.push(
            json!({"case":"missing-global-source","target":"primary","source":malformed,
            "inputs":[],"expected":"missing"}),
        );
        rows.push(
            json!({"case":"recovery-after-global","target":"named","source":pass,
            "inputs":[{"name":"guard","path":guard}],"expected":"ok"}),
        );
    } else {
        let source = inputs.join("primary.xml");
        padded(&source, "<Envelope><Pad>", "</Pad></Envelope>", DOCUMENT)?;
        paths.push(source.clone());
        let mut documents = Vec::new();
        for (name, length) in [
            ("a", DOCUMENT),
            ("b", DOCUMENT),
            ("c", DOCUMENT - 64),
            ("d", 64),
            ("d-over", 65),
        ] {
            let path = inputs.join(format!("{name}.xml"));
            padded(
                &path,
                &format!("<Data><Token>{}</Token><Pad>", name.chars().next().unwrap()),
                "</Pad></Data>",
                length,
            )?;
            paths.push(path.clone());
            documents.push(path);
        }
        for target in ["primary", "named"] {
            for over in [false, true] {
                for reverse in [false, true] {
                    let mut named: Vec<_> = ["a","b","c","d"].into_iter().enumerate().map(|(i,name)|
                        json!({"name":name,"path":documents[if over && i==3 {4} else {i}]})).collect();
                    if reverse {
                        named.reverse();
                    }
                    rows.push(json!({"case":format!("aggregate-{target}-{}-{}",if over {"over"} else {"exact"},if reverse {"reverse"} else {"forward"}),
                        "target":target,"source":source,"inputs":named,"expected":if over {"aggregate"} else {"ok"},
                        "total":AGGREGATE+usize::from(over)}));
                }
            }
        }
    }
    Ok((rows, paths))
}

fn native_case(project: &Project, row: &Json) -> anyhow::Result<Vec<Vec<u8>>> {
    let source = std::fs::read(row["source"].as_str().unwrap())?;
    let data = row["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|input| {
            Ok((
                input["name"].as_str().unwrap(),
                PathBuf::from(input["path"].as_str().unwrap()),
                std::fs::read(input["path"].as_str().unwrap())?,
            ))
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    let inputs = data
        .iter()
        .map(|(name, path, bytes)| {
            cli::NamedPayloadInput::new(name, cli::PayloadDocument::new(path, bytes)?)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let source = cli::PayloadDocument::new(Path::new("/counterexamples/input.xml"), &source)?;
    let mut project = project.clone();
    project.extra_targets[0].path = Some("/counterexamples/named.xml".into());
    let mut options = cli::PayloadRunOptions::new(source)
        .with_extra_sources(&inputs)
        .with_output_path(Path::new("/counterexamples/primary.xml"))
        .with_current_datetime("2026-10-26T00:00:00Z")?;
    let target = row["target"].as_str().unwrap();
    if target != "all" {
        options = options.with_target(if target == "primary" {
            engine::TargetSelection::Primary
        } else {
            engine::TargetSelection::Named(target)
        });
    }
    let output = cli::run_project_value_payloads(
        &project,
        Path::new("/counterexamples/mapping.json"),
        &options,
    )?;
    Ok(output.artifacts.into_iter().map(|a| a.bytes).collect())
}

fn assert_small_tree(bytes: &[u8]) -> TestResult<()> {
    let document = roxmltree::Document::parse(std::str::from_utf8(bytes)?)?;
    let tags: Vec<_> = document
        .root_element()
        .children()
        .filter(|n| n.has_tag_name("Tag"))
        .map(|n| n.text().unwrap_or(""))
        .collect();
    assert_eq!(tags, ["alpha", "", "omega😀"]);
    let maybe: Vec<_> = document
        .root_element()
        .children()
        .filter(|n| n.has_tag_name("Maybe"))
        .collect();
    assert_eq!(maybe.len(), 2);
    assert_eq!(
        maybe[0].attribute(("http://www.w3.org/2001/XMLSchema-instance", "nil")),
        Some("true")
    );
    assert_eq!(maybe[1].text(), Some("tail"));
    Ok(())
}

fn assert_native_global(project: &Project, case: &Json) -> TestResult<()> {
    let error = native_case(project, case).expect_err("native global rule must fire");
    for current in error.chain() {
        if let Some(engine::EngineError::MappingFailure { rule, message }) = current.downcast_ref()
        {
            assert_eq!(*rule, 1);
            assert_eq!(message.as_deref(), Some("blocked-by-global-rule"));
            return Ok(());
        }
    }
    Err(format!("native failure did not retain the exact global rule: {error:#}").into())
}

fn exercise_counterexamples(language: &str) -> TestResult<()> {
    let _guard = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let directory = TempDir::new(&format!("selected-xml-counterexamples-{language}"))?;
    let root = &directory.0;
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    for large in [false, true] {
        let dir = root.join(if large { "aggregate" } else { "scalar-global" });
        std::fs::create_dir(&dir)?;
        let project = if large {
            aggregate_project()
        } else {
            small_project()
        };
        assert!(engine::validate(&project).is_empty());
        let program = codegen::lower(&project)?;
        assert!(
            codegen::prepare_boundary_emission(&program, codegen::CompanionSelection::Complete)?
                .xml()
                .unwrap()
                .selected_inputs()
                .is_some()
        );
        let generated = dir.join("generated");
        let artifacts = if language == "rust" {
            codegen_rust::emit(
                &program,
                &codegen_rust::Options {
                    package_name: "xml-counterexample-mapping".into(),
                    runtime_dependency: codegen_rust::RuntimeDependency::Path(
                        runtime.display().to_string(),
                    ),
                },
            )?
        } else {
            codegen_csharp::emit(&program)?
        };
        write_artifacts(&generated, artifacts)?;
        std::fs::write(
            dir.join("project.json"),
            mapping::project_file::encode_pretty(&project)?,
        )?;
        let source_identity = artifact_fingerprints(&generated)?;
        let (cases, paths) = corpus(&dir, large)?;
        let identities = paths
            .iter()
            .map(|p| identity(p))
            .collect::<TestResult<Vec<_>>>()?;
        std::fs::write(
            dir.join("input-identities.json"),
            serde_json::to_vec_pretty(&identities)?,
        )?;
        std::fs::write(dir.join("cases.json"), serde_json::to_vec_pretty(&cases)?)?;
        let host = dir.join("host");
        std::fs::create_dir_all(host.join("src"))?;
        let binary = if language == "rust" {
            std::fs::write(
                host.join("src/main.rs"),
                include_str!("counterexamples-host.rs.txt"),
            )?;
            std::fs::write(
                host.join("Cargo.toml"),
                format!(
                    "[package]\nname=\"xml-counterexample-host\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\nxml-counterexample-mapping={{path=\"../generated\"}}\ncodegen-runtime={{path={runtime:?}}}\nserde_json=\"=1.0.150\"\n"
                ),
            )?;
            let target = root.join("rust-host-target");
            let mut build = Command::new("cargo");
            build
                .args(["build", "--quiet", "--offline", "--jobs", "2"])
                .current_dir(&host)
                .env("CARGO_TARGET_DIR", &target)
                .env("RUSTFLAGS", "-D warnings");
            recorded(&mut build, &dir, "build")?;
            target.join("debug/xml-counterexample-host")
        } else {
            std::fs::write(
                host.join("Program.cs"),
                include_str!("counterexamples-host.cs.txt"),
            )?;
            std::fs::write(
                host.join("Host.csproj"),
                "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include=\"../generated/Ferrule.Generated.csproj\" /></ItemGroup></Project>",
            )?;
            std::fs::write(
                dir.join("NuGet.Config"),
                "<configuration><packageSources><clear /></packageSources></configuration>",
            )?;
            let mut build = dotnet_command(&dir);
            build
                .args([
                    "build",
                    "host/Host.csproj",
                    "--nologo",
                    "--verbosity",
                    "quiet",
                    "-m:1",
                    "/p:UseSharedCompilation=false",
                    "--artifacts-path",
                ])
                .arg(dir.join("artifacts"))
                .current_dir(&dir);
            recorded(&mut build, &dir, "build")?;
            dir.join("artifacts/bin/Host/debug/Host.dll")
        };
        let mut command = if language == "csharp" {
            let mut command = dotnet_command(&dir);
            command.arg(&binary);
            command
        } else {
            Command::new(&binary)
        };
        command.arg(dir.join("cases.json")).current_dir(&dir);
        let output = recorded(&mut command, &dir, "host")?;
        assert_eq!(source_identity, artifact_fingerprints(&generated)?);
        let rows = std::str::from_utf8(&output.stdout)?
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<Vec<Json>, _>>()?;
        assert_eq!(rows.len(), cases.len());
        for (row, case) in rows.iter().zip(&cases) {
            assert_eq!(row["case"], case["case"]);
            match case["expected"].as_str().unwrap() {
                "ok" => {
                    assert_eq!(row["kind"], "ok", "{row}");
                    let native = native_case(&project, case)?;
                    let hex: Vec<_> = native
                        .iter()
                        .map(|b| b.iter().map(|v| format!("{v:02x}")).collect::<String>())
                        .collect();
                    assert_eq!(row["documents"], json!(hex));
                    if !large {
                        for document in native {
                            assert_small_tree(&document)?;
                        }
                    }
                }
                "global" => {
                    assert_eq!(row["kind"], "Mapping");
                    assert_eq!(row["runtime"], "MappingFailure");
                    assert_eq!(row["rule"], 1);
                    assert_eq!(row["message"], "blocked-by-global-rule");
                    assert_eq!(row["input"], Json::Null);
                    assert_eq!(row["inputIndex"], Json::Null);
                    assert_native_global(&project, case)?;
                }
                "unexpected" => {
                    assert_eq!(row["kind"], "Mapping");
                    assert_eq!(row["runtime"], "UnexpectedNamedSource");
                    assert_eq!(row["input"], Json::Null);
                    assert_eq!(row["inputIndex"], Json::Null);
                    if case["case"] == "unused-malformed" {
                        assert!(native_case(&project, case).is_ok());
                    } else {
                        assert_native_global(&project, case)?;
                    }
                }
                "missing" => {
                    assert_eq!(row["kind"], "Mapping");
                    assert_eq!(row["runtime"], "MissingNamedSource");
                }
                "aggregate" => {
                    assert_eq!(row["kind"], "Input");
                    assert_eq!(row["input"], "d");
                    assert_eq!(row["inputIndex"], 3);
                    assert_eq!(row["resource"], "xml_input_set_utf8_bytes");
                    assert_eq!(row["observed"], AGGREGATE + 1);
                    assert_eq!(row["limit"], AGGREGATE);
                    assert!(native_case(&project, case).is_err());
                }
                other => panic!("unknown counterexample expectation {other}"),
            }
            if large {
                assert_eq!(row["sizes"].as_array().unwrap().len(), 5);
                assert_eq!(row["total"], case["total"]);
                assert!(
                    row["sizes"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|size| size.as_u64().unwrap() <= DOCUMENT as u64)
                );
            }
            if case["expected"] != "ok" {
                assert!(row.get("documents").is_none());
                assert_eq!(row["output"], Json::Null);
                assert_eq!(row["preserved"], true);
            }
        }
        let after = paths
            .iter()
            .map(|p| identity(p))
            .collect::<TestResult<Vec<_>>>()?;
        assert_eq!(
            identities, after,
            "authored input files changed during execution"
        );
        std::fs::write(
            dir.join("input-identities-after.json"),
            serde_json::to_vec_pretty(&after)?,
        )?;
        // Keep the exact identities and deterministic factory; release only our
        // bulky authored input files after all native/public assertions pass.
        for path in paths {
            std::fs::remove_file(path)?;
        }
        std::fs::write(
            dir.join("bulk-inputs-cleaned.txt"),
            "Owned authored inputs removed after verified readback.\n",
        )?;
    }
    Ok(())
}

#[test]
fn selected_xml_counterexamples_rust() -> TestResult<()> {
    exercise_counterexamples("rust")
}
#[test]
fn selected_xml_counterexamples_csharp() -> TestResult<()> {
    exercise_counterexamples("csharp")
}
