use super::*;

const PREDICATE: u32 = 19;
const NON_BOOL_PREDICATE: u32 = 61;

fn collection_find_project() -> Project {
    let person = SchemaNode::group(
        "People",
        vec![
            string("First"),
            string("Title"),
            string("Email"),
            bool_("Decision"),
        ],
    )
    .repeating();
    let department = SchemaNode::group("Departments", vec![string("Office"), person]).repeating();
    let catalog_item = SchemaNode::group("Items", vec![string("Key"), string("Label")]).repeating();

    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                string("Needle"),
                string("Suffix"),
                bool_("FailNonBool"),
                department,
            ],
        ),
        target: SchemaNode::group(
            "Target",
            vec![
                string("Details"),
                int("DepartmentPosition"),
                int("PersonPosition"),
                string("CatalogValue"),
                string("NullableDecision"),
                string("LazyMiss"),
                string("NonBoolProbe"),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![mapping::NamedSource {
            name: "catalog".into(),
            path: "ignored/catalog.json".into(),
            schema: SchemaNode::group("Catalog", vec![catalog_item]),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    Node::SourceField {
                        path: vec!["Needle".into()],
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["Suffix".into()],
                        frame: None,
                    },
                ),
                (
                    3,
                    Node::SourceField {
                        path: vec!["FailNonBool".into()],
                        frame: None,
                    },
                ),
                (
                    10,
                    Node::SourceField {
                        path: vec!["Office".into()],
                        frame: Some(vec!["Departments".into()]),
                    },
                ),
                (
                    11,
                    Node::SourceField {
                        path: vec!["First".into()],
                        frame: Some(vec!["Departments".into(), "People".into()]),
                    },
                ),
                (
                    12,
                    Node::SourceField {
                        path: vec!["Title".into()],
                        frame: Some(vec!["Departments".into(), "People".into()]),
                    },
                ),
                (
                    13,
                    Node::SourceField {
                        path: vec!["Email".into()],
                        frame: Some(vec!["Departments".into(), "People".into()]),
                    },
                ),
                (
                    14,
                    Node::Position {
                        collection: vec!["Departments".into()],
                    },
                ),
                (
                    15,
                    Node::Position {
                        collection: vec!["Departments".into(), "People".into()],
                    },
                ),
                (
                    16,
                    Node::Const {
                        value: Value::String("HQ".into()),
                    },
                ),
                (
                    17,
                    Node::Call {
                        function: "equal".into(),
                        args: vec![10, 16],
                    },
                ),
                (
                    18,
                    Node::Call {
                        function: "equal".into(),
                        args: vec![11, 1],
                    },
                ),
                (
                    PREDICATE,
                    Node::Call {
                        function: "and".into(),
                        args: vec![17, 18],
                    },
                ),
                (
                    20,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![12, 13, 2],
                    },
                ),
                (
                    21,
                    Node::CollectionFind {
                        collection: vec!["Departments".into(), "People".into()],
                        predicate: PREDICATE,
                        value: 20,
                    },
                ),
                (
                    22,
                    Node::CollectionFind {
                        collection: vec!["Departments".into(), "People".into()],
                        predicate: PREDICATE,
                        value: 14,
                    },
                ),
                (
                    23,
                    Node::CollectionFind {
                        collection: vec!["Departments".into(), "People".into()],
                        predicate: PREDICATE,
                        value: 15,
                    },
                ),
                (
                    30,
                    Node::SourceField {
                        path: vec!["Key".into()],
                        frame: Some(vec!["catalog".into(), "Items".into()]),
                    },
                ),
                (
                    31,
                    Node::SourceField {
                        path: vec!["Label".into()],
                        frame: Some(vec!["catalog".into(), "Items".into()]),
                    },
                ),
                (
                    32,
                    Node::Call {
                        function: "equal".into(),
                        args: vec![30, 1],
                    },
                ),
                (
                    33,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![31, 2],
                    },
                ),
                (
                    34,
                    Node::CollectionFind {
                        collection: vec!["catalog".into(), "Items".into()],
                        predicate: 32,
                        value: 33,
                    },
                ),
                (
                    40,
                    Node::SourceField {
                        path: vec!["Decision".into()],
                        frame: Some(vec!["Departments".into(), "People".into()]),
                    },
                ),
                (
                    41,
                    Node::CollectionFind {
                        collection: vec!["Departments".into(), "People".into()],
                        predicate: 40,
                        value: 13,
                    },
                ),
                (
                    50,
                    Node::Const {
                        value: Value::String("missing".into()),
                    },
                ),
                (
                    51,
                    Node::Call {
                        function: "equal".into(),
                        args: vec![11, 50],
                    },
                ),
                (
                    52,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    53,
                    Node::Const {
                        value: Value::Int(0),
                    },
                ),
                (
                    54,
                    Node::Call {
                        function: "divide".into(),
                        args: vec![52, 53],
                    },
                ),
                (
                    55,
                    Node::CollectionFind {
                        collection: vec!["Departments".into(), "People".into()],
                        predicate: 51,
                        value: 54,
                    },
                ),
                (
                    NON_BOOL_PREDICATE,
                    Node::Const {
                        value: Value::String("not bool".into()),
                    },
                ),
                (
                    62,
                    Node::CollectionFind {
                        collection: vec!["Departments".into(), "People".into()],
                        predicate: NON_BOOL_PREDICATE,
                        value: 13,
                    },
                ),
                (
                    63,
                    Node::Const {
                        value: Value::String("safe".into()),
                    },
                ),
                (
                    64,
                    Node::If {
                        condition: 3,
                        then: 62,
                        else_: 63,
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "Details".into(),
                    node: 21,
                },
                Binding {
                    target_field: "DepartmentPosition".into(),
                    node: 22,
                },
                Binding {
                    target_field: "PersonPosition".into(),
                    node: 23,
                },
                Binding {
                    target_field: "CatalogValue".into(),
                    node: 34,
                },
                Binding {
                    target_field: "NullableDecision".into(),
                    node: 41,
                },
                Binding {
                    target_field: "LazyMiss".into(),
                    node: 55,
                },
                Binding {
                    target_field: "NonBoolProbe".into(),
                    node: 64,
                },
            ],
            ..Scope::default()
        },
    }
}

fn primary_source(fail_non_bool: bool) -> Instance {
    Instance::Group(vec![
        (
            "Needle".into(),
            Instance::Scalar(Value::String("Ada".into())),
        ),
        ("Suffix".into(), Instance::Scalar(Value::String("!".into()))),
        (
            "FailNonBool".into(),
            Instance::Scalar(Value::Bool(fail_non_bool)),
        ),
        (
            "Departments".into(),
            Instance::Repeated(vec![
                department(
                    "Remote",
                    vec![
                        person("Ada", "Wrong: ", "remote@example.test", Value::Null),
                        person("Lin", "Lead: ", "lin@example.test", Value::xml_nil()),
                    ],
                ),
                department(
                    "HQ",
                    vec![
                        person(
                            "Grace",
                            "Director: ",
                            "grace@example.test",
                            Value::Bool(false),
                        ),
                        person("Ada", "Engineer: ", "ada@example.test", Value::Bool(true)),
                    ],
                ),
            ]),
        ),
    ])
}

fn primary_without_departments() -> Instance {
    Instance::Group(vec![
        (
            "Needle".into(),
            Instance::Scalar(Value::String("Ada".into())),
        ),
        ("Suffix".into(), Instance::Scalar(Value::String("!".into()))),
        ("FailNonBool".into(), Instance::Scalar(Value::Bool(false))),
    ])
}

fn department(office: &str, people: Vec<Instance>) -> Instance {
    Instance::Group(vec![
        (
            "Office".into(),
            Instance::Scalar(Value::String(office.into())),
        ),
        ("People".into(), Instance::Repeated(people)),
    ])
}

fn person(first: &str, title: &str, email: &str, decision: Value) -> Instance {
    Instance::Group(vec![
        (
            "First".into(),
            Instance::Scalar(Value::String(first.into())),
        ),
        (
            "Title".into(),
            Instance::Scalar(Value::String(title.into())),
        ),
        (
            "Email".into(),
            Instance::Scalar(Value::String(email.into())),
        ),
        ("Decision".into(), Instance::Scalar(decision)),
    ])
}

fn catalog_source() -> Instance {
    Instance::Group(vec![(
        "Items".into(),
        Instance::Repeated(vec![
            catalog_item("Ada", "catalog-A"),
            catalog_item("Ada", "catalog-second"),
            catalog_item("Grace", "catalog-G"),
        ]),
    )])
}

fn catalog_item(key: &str, label: &str) -> Instance {
    Instance::Group(vec![
        ("Key".into(), Instance::Scalar(Value::String(key.into()))),
        (
            "Label".into(),
            Instance::Scalar(Value::String(label.into())),
        ),
    ])
}

fn expected() -> Instance {
    Instance::Group(vec![
        (
            "Details".into(),
            Instance::Scalar(Value::String("Engineer: ada@example.test!".into())),
        ),
        ("DepartmentPosition".into(), Instance::Scalar(Value::Int(2))),
        ("PersonPosition".into(), Instance::Scalar(Value::Int(2))),
        (
            "CatalogValue".into(),
            Instance::Scalar(Value::String("catalog-A!".into())),
        ),
        (
            "NullableDecision".into(),
            Instance::Scalar(Value::String("ada@example.test".into())),
        ),
        ("LazyMiss".into(), Instance::Scalar(Value::Null)),
        (
            "NonBoolProbe".into(),
            Instance::Scalar(Value::String("safe".into())),
        ),
    ])
}

fn engine_sources() -> Vec<(String, Instance)> {
    vec![("catalog".into(), catalog_source())]
}

#[test]
fn collection_find_matches_engine_and_generated_backends() -> TestResult<()> {
    let project = collection_find_project();
    assert!(engine::validate(&project).is_empty());
    assert_eq!(
        engine::run_with_sources(&project, &primary_source(false), engine_sources())?,
        expected()
    );
    assert_eq!(
        engine::run_with_sources(&project, &primary_source(true), engine_sources())
            .expect_err("non-boolean predicate must fail"),
        engine::EngineError::NotABool {
            node: NON_BOOL_PREDICATE,
            found: "string",
        }
    );
    assert_eq!(
        engine::run_with_sources(&project, &primary_without_departments(), engine_sources())
            .expect_err("missing collection root must fail"),
        engine::EngineError::MissingSourceField("Departments/People".into())
    );

    let directory = TempDir::new("collection_find")?;
    let project_path = directory.0.join("collection-find.json");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;

    let rust_output = directory.0.join("rust");
    generate_project(
        &project_path,
        &rust_output,
        GenerateTarget::Rust {
            runtime_path: Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"),
        },
    )?;
    std::fs::write(
        rust_output.join("src/main.rs"),
        include_str!("fixtures/collection_find_rust_harness.rs.txt"),
    )?;
    let rust = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .output()?;
    assert!(
        rust.status.success(),
        "generated Rust collection find failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust.stdout),
        String::from_utf8_lossy(&rust.stderr)
    );

    let csharp_output = directory.0.join("csharp");
    generate_project(&project_path, &csharp_output, GenerateTarget::CSharp)?;
    let harness = csharp_output.join("Harness");
    std::fs::create_dir(&harness)?;
    std::fs::write(
        harness.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <InvariantGlobalization>true</InvariantGlobalization>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="../Ferrule.Generated.csproj" />
  </ItemGroup>
</Project>
"#,
    )?;
    std::fs::write(
        harness.join("Program.cs"),
        include_str!("fixtures/collection_find_csharp_harness.cs.txt"),
    )?;
    let csharp = Command::new("dotnet")
        .args([
            "run",
            "--project",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
        ])
        .current_dir(&csharp_output)
        .output()?;
    assert!(
        csharp.status.success(),
        "generated C# collection find failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}
