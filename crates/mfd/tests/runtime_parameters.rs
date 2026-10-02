use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use engine::{ExecutionContext, ExecutionPurpose, RuntimeParameters};
use ir::{Instance, ScalarType, Value};
use mapping::Node;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_runtime_parameters_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_design(directory: &Path) -> Result<PathBuf, std::io::Error> {
    std::fs::write(
        directory.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Input"><xs:complexType><xs:sequence>
    <xs:element name="Dummy" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        directory.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Output"><xs:complexType><xs:sequence>
    <xs:element name="Echo" type="xs:string"/>
    <xs:element name="Correlation" type="xs:string"/>
    <xs:element name="Control" type="xs:int"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let design = directory.join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data>
    <root><entry name="Input"><entry name="Dummy" outkey="9"/></entry></root>
    <document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Input"/>
  </data></component>
  <component name="Correlation input" library="core" kind="6"><targets><datapoint pos="0" key="10"/></targets><data><input datatype="string"/><parameter usageKind="input" name="correlation_id"/></data></component>
  <component name="Control input" library="core" kind="6"><targets><datapoint pos="0" key="11"/></targets><data><input datatype="integer"/><parameter usageKind="input" name="control_number"/></data></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="Output"><entry name="Echo" inpkey="19"/><entry name="Correlation" inpkey="20"/><entry name="Control" inpkey="21"/></entry></root>
    <document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Output"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="9"><edges><edge vertexkey="19"/></edges></vertex>
  <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(design)
}

fn assert_declarations(project: &mapping::Project) {
    let mut declarations = project
        .graph
        .nodes
        .values()
        .filter_map(|node| match node {
            Node::RuntimeParameter { name, ty, .. } => Some((name.as_str(), *ty)),
            _ => None,
        })
        .collect::<Vec<_>>();
    declarations.sort_unstable_by_key(|(name, _)| *name);
    assert_eq!(
        declarations,
        vec![
            ("control_number", ScalarType::Int),
            ("correlation_id", ScalarType::String),
        ]
    );
}

fn execute(project: &mapping::Project) -> Result<Instance, Box<dyn std::error::Error>> {
    let source = format_xml::from_str("<Input><Dummy>source</Dummy></Input>", &project.source)?;
    let mut parameters = RuntimeParameters::new();
    parameters.insert("correlation_id", Value::String("txn-23".into()))?;
    parameters.insert("control_number", Value::Int(7001))?;
    let execution = ExecutionContext::new(Path::new("mapping.mfd")).with_parameters(&parameters);
    Ok(engine::run_with_context(project, &source, &execution)?)
}

fn assert_output(output: &Instance) {
    assert_eq!(
        output.field("Echo").and_then(Instance::as_scalar),
        Some(&Value::String("source".into()))
    );
    assert_eq!(
        output.field("Correlation").and_then(Instance::as_scalar),
        Some(&Value::String("txn-23".into()))
    );
    assert_eq!(
        output.field("Control").and_then(Instance::as_scalar),
        Some(&Value::Int(7001))
    );
}

#[test]
fn unconnected_input_parameters_become_typed_host_inputs_and_roundtrip()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    let imported = mfd::import(&write_design(&directory.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_declarations(&imported.project);
    assert_output(&execute(&imported.project)?);

    let exported_path = directory.0.join("roundtrip.mfd");
    assert!(mfd::export(&imported.project, &exported_path)?.is_empty());
    let exported = std::fs::read_to_string(&exported_path)?;
    assert!(exported.contains("name=\"correlation_id\""));
    assert!(exported.contains("name=\"control_number\""));
    assert!(exported.contains("kind=\"6\""));

    let roundtrip = mfd::import(&exported_path)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert!(engine::validate(&roundtrip.project).is_empty());
    assert_declarations(&roundtrip.project);
    assert_output(&execute(&roundtrip.project)?);
    Ok(())
}

#[test]
fn optional_input_without_preview_or_default_skips_scalar_and_rejects_executable_import()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    let design = write_design(&directory.0)?;
    let text = std::fs::read_to_string(&design)?.replace(
        "name=\"correlation_id\"/>",
        "name=\"correlation_id\" optional=\"1\"/>",
    );
    std::fs::write(&design, text)?;
    let imported = mfd::import(&design)?;
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("optional input parameter `correlation_id`")
            && warning.contains("omitted-input semantics are unsupported")
            && warning.contains("dependent value skipped")
    }));
    assert!(!imported.project.graph.nodes.values().any(|node| matches!(
        node,
        Node::RuntimeParameter { name, .. } | Node::RuntimeParameterDefault { name, .. }
            if name == "correlation_id"
    )));
    assert!(imported.project.graph.nodes.values().any(|node| matches!(
        node,
        Node::RuntimeParameter { name, ty: ScalarType::Int, .. }
            if name == "control_number"
    )));
    assert!(engine::validate(&imported.project).is_empty());
    assert!(matches!(
        mfd::import_with_profile(
            &design,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        ),
        Err(mfd::MfdError::IncompatibleImport(_))
    ));

    let source = format_xml::from_str(
        "<Input><Dummy>source</Dummy></Input>",
        &imported.project.source,
    )?;
    let mut hosts = RuntimeParameters::new();
    hosts.insert("control_number", Value::Int(7001))?;
    let absent = ExecutionContext::new(&design).with_parameters(&hosts);
    let absent_output = engine::run_with_context(&imported.project, &source, &absent)?;
    hosts.insert("correlation_id", Value::String("ignored".into()))?;
    let supplied = ExecutionContext::new(&design).with_parameters(&hosts);
    let supplied_output = engine::run_with_context(&imported.project, &source, &supplied)?;
    assert_eq!(absent_output, supplied_output);
    assert_eq!(
        supplied_output.field("Echo").and_then(Instance::as_scalar),
        Some(&Value::String("source".into()))
    );
    assert_eq!(
        supplied_output
            .field("Control")
            .and_then(Instance::as_scalar),
        Some(&Value::Int(7001))
    );
    assert!(supplied_output.field("Correlation").is_none());
    Ok(())
}

#[test]
fn required_host_input_graph_stays_bounded_across_repeated_native_roundtrips()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    let imported = mfd::import(&write_design(&directory.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let count = imported.project.graph.nodes.len();
    let expected = execute(&imported.project)?;
    let mut project = imported.project;
    for round in 0..5 {
        let path = directory.0.join(format!("round-{round}.mfd"));
        mfd::export_with_profile(&project, &path, mfd::ExportProfile::NativeMfd)?;
        let restored = mfd::import(&path)?;
        assert!(restored.warnings.is_empty(), "{:?}", restored.warnings);
        assert_eq!(restored.project.graph.nodes.len(), count, "round {round}");
        assert_eq!(execute(&restored.project)?, expected, "round {round}");
        project = restored.project;
    }
    Ok(())
}

fn write_optional_design(directory: &Path) -> Result<PathBuf, std::io::Error> {
    std::fs::write(
        directory.join("optional-source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Input"><xs:complexType><xs:sequence>
    <xs:element name="Dummy" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        directory.join("optional-target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Output"><xs:complexType><xs:sequence>
    <xs:element name="Prefix" type="xs:string"/>
    <xs:element name="Count" type="xs:int"/>
    <xs:element name="Ratio" type="xs:decimal"/>
    <xs:element name="PreviewCount" type="xs:int"/>
    <xs:element name="RequiredPreview" type="xs:string"/>
    <xs:element name="Required" type="xs:string"/>
    <xs:element name="Lazy" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let path = directory.join("optional.mfd");
    std::fs::write(
        &path,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data>
    <root><entry name="Input"><entry name="Dummy" outkey="9"/></entry></root>
    <document schema="optional-source.xsd" inputinstance="source.xml" instanceroot="{}Input"/>
  </data></component>
  <component name="constant" library="core" kind="2"><targets><datapoint key="100"/></targets><data><constant value="B" datatype="string"/></data></component>
  <component name="constant" library="core" kind="2"><targets><datapoint key="101"/></targets><data><constant value="7" datatype="string"/></data></component>
  <component name="constant" library="core" kind="2"><targets><datapoint key="102"/></targets><data><constant value="8.5" datatype="string"/></data></component>
  <component name="constant" library="core" kind="2"><targets><datapoint key="103"/></targets><data><constant value="8" datatype="integer"/></data></component>
  <component name="Prefix" library="core" kind="6"><sources><datapoint pos="0" key="110"/></sources><targets><datapoint pos="0" key="111"/></targets><data><input datatype="string"/><parameter usageKind="input" name="Prefix" optional="1"/></data></component>
  <component name="Count" library="core" kind="6"><sources><datapoint pos="0" key="112"/></sources><targets><datapoint pos="0" key="113"/></targets><data><input datatype="integer"/><parameter usageKind="input" name="Count" optional="1"/></data></component>
  <component name="Ratio" library="core" kind="6"><sources><datapoint pos="0" key="114"/></sources><targets><datapoint pos="0" key="115"/></targets><data><input datatype="decimal"/><parameter usageKind="input" name="Ratio" optional="1"/></data></component>
  <component name="PreviewCount" library="core" kind="6"><sources><datapoint pos="0" key="120"/></sources><targets><datapoint pos="0" key="121"/></targets><data><input datatype="integer" previewvalue="9" usepreviewvalue="1"/><parameter usageKind="input" name="PreviewCount" optional="1"/></data></component>
  <component name="RequiredPreview" library="core" kind="6"><sources><datapoint pos="0" key="122"/></sources><targets><datapoint pos="0" key="123"/></targets><data><input datatype="string" previewvalue="legacy" usepreviewvalue="1"/><parameter usageKind="input" name="RequiredPreview"/></data></component>
  <component name="Required" library="core" kind="6"><targets><datapoint pos="0" key="116"/></targets><data><input datatype="string"/><parameter usageKind="input" name="Required"/></data></component>
  <component name="Missing" library="core" kind="6"><targets><datapoint pos="0" key="117"/></targets><data><input datatype="string"/><parameter usageKind="input" name="Missing"/></data></component>
  <component name="Lazy" library="core" kind="6"><sources><datapoint pos="0" key="118"/></sources><targets><datapoint pos="0" key="119"/></targets><data><input datatype="string"/><parameter usageKind="input" name="Lazy" optional="1"/></data></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="Output"><entry name="Prefix" inpkey="201"/><entry name="Count" inpkey="202"/><entry name="Ratio" inpkey="203"/><entry name="PreviewCount" inpkey="206"/><entry name="RequiredPreview" inpkey="207"/><entry name="Required" inpkey="204"/><entry name="Lazy" inpkey="205"/></entry></root>
    <document schema="optional-target.xsd" outputinstance="target.xml" instanceroot="{}Output"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="100"><edges><edge vertexkey="110"/></edges></vertex>
  <vertex vertexkey="101"><edges><edge vertexkey="112"/></edges></vertex>
  <vertex vertexkey="102"><edges><edge vertexkey="114"/></edges></vertex>
  <vertex vertexkey="103"><edges><edge vertexkey="120"/></edges></vertex>
  <vertex vertexkey="117"><edges><edge vertexkey="118"/></edges></vertex>
  <vertex vertexkey="111"><edges><edge vertexkey="201"/></edges></vertex>
  <vertex vertexkey="113"><edges><edge vertexkey="202"/></edges></vertex>
  <vertex vertexkey="115"><edges><edge vertexkey="203"/></edges></vertex>
  <vertex vertexkey="121"><edges><edge vertexkey="206"/></edges></vertex>
  <vertex vertexkey="123"><edges><edge vertexkey="207"/></edges></vertex>
  <vertex vertexkey="116"><edges><edge vertexkey="204"/></edges></vertex>
  <vertex vertexkey="119"><edges><edge vertexkey="205"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(path)
}

fn assert_optional_declarations(project: &mapping::Project) {
    let mut declarations = project
        .graph
        .nodes
        .values()
        .filter_map(|node| match node {
            Node::RuntimeParameterDefault {
                name,
                ty,
                default,
                preview,
            } => Some((name.as_str(), *ty, *default, preview.as_deref())),
            _ => None,
        })
        .collect::<Vec<_>>();
    declarations.sort_unstable_by_key(|(name, _, _, _)| *name);
    assert_eq!(
        declarations
            .iter()
            .map(|(name, ty, _, _)| (*name, *ty))
            .collect::<Vec<_>>(),
        vec![
            ("Count", ScalarType::Int),
            ("Lazy", ScalarType::String),
            ("Prefix", ScalarType::String),
            ("PreviewCount", ScalarType::Int),
            ("Ratio", ScalarType::Float),
        ]
    );
    for (name, _, default, preview) in declarations {
        assert_eq!(preview, (name == "PreviewCount").then_some("9"));
        match name {
            "Count" => assert!(matches!(
                project.graph.nodes.get(&default),
                Some(Node::Const {
                    value: Value::String(value)
                }) if value == "7"
            )),
            "Prefix" => assert!(matches!(
                project.graph.nodes.get(&default),
                Some(Node::Const {
                    value: Value::String(value)
                }) if value == "B"
            )),
            "Ratio" => assert!(matches!(
                project.graph.nodes.get(&default),
                Some(Node::Const {
                    value: Value::String(value)
                }) if value == "8.5"
            )),
            "PreviewCount" => assert!(matches!(
                project.graph.nodes.get(&default),
                Some(Node::Const {
                    value: Value::Int(8)
                })
            )),
            "Lazy" => {
                let mut input = default;
                let mut found = false;
                for _ in 0..8 {
                    match project.graph.nodes.get(&input) {
                        Some(Node::Call { function, args })
                            if function == "string" && args.len() == 1 =>
                        {
                            input = args[0];
                        }
                        Some(Node::RuntimeParameter { name, .. }) if name == "Missing" => {
                            found = true;
                            break;
                        }
                        other => panic!("unexpected lazy default dependency: {other:?}"),
                    }
                }
                assert!(found, "lazy default must depend on the Missing host input");
            }
            _ => unreachable!(),
        }
    }
    assert_eq!(
        project
            .graph
            .nodes
            .values()
            .filter(|node| matches!(node, Node::RuntimeParameter { .. }))
            .count(),
        3
    );
    assert!(project.graph.nodes.values().any(|node| matches!(
        node,
        Node::RuntimeParameter { name, ty: ScalarType::String, preview: Some(preview) }
            if name == "RequiredPreview" && preview == "legacy"
    )));
}

fn run_optional(
    project: &mapping::Project,
    parameters: &RuntimeParameters,
) -> Result<Instance, engine::EngineError> {
    run_optional_for_purpose(project, parameters, ExecutionPurpose::Run)
}

fn run_optional_for_purpose(
    project: &mapping::Project,
    parameters: &RuntimeParameters,
    purpose: ExecutionPurpose,
) -> Result<Instance, engine::EngineError> {
    let source = Instance::Group(
        (vec![(
            "Dummy".into(),
            Instance::Scalar(Value::String("source".into())),
        )])
        .into(),
    );
    let context = ExecutionContext::new(Path::new("optional.mfd"))
        .with_parameters(parameters)
        .with_purpose(purpose);
    engine::run_with_context(project, &source, &context)
}

#[test]
fn connected_optional_inputs_preserve_defaults_and_host_override_through_native_roundtrip()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    let imported = mfd::import(&write_optional_design(&directory.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_optional_declarations(&imported.project);

    let serialized = serde_json::to_string(&imported.project)?;
    let serialized: mapping::Project = serde_json::from_str(&serialized)?;
    assert_optional_declarations(&serialized);

    let exported_path = directory.0.join("optional-roundtrip.mfd");
    assert!(
        mfd::export_with_profile(&serialized, &exported_path, mfd::ExportProfile::NativeMfd,)?
            .is_native_compatible()
    );
    let exported = std::fs::read_to_string(&exported_path)?;
    assert_eq!(exported.matches("optional=\"1\"").count(), 5);
    assert_eq!(exported.matches("usepreviewvalue=\"1\"").count(), 2);
    assert!(exported.contains("previewvalue=\"9\" usepreviewvalue=\"1\""));
    assert!(exported.contains("previewvalue=\"legacy\" usepreviewvalue=\"1\""));
    let roundtrip = mfd::import(&exported_path)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert!(engine::validate(&roundtrip.project).is_empty());
    assert_optional_declarations(&roundtrip.project);
    let second_path = directory.0.join("optional-second-roundtrip.mfd");
    assert!(
        mfd::export_with_profile(
            &roundtrip.project,
            &second_path,
            mfd::ExportProfile::NativeMfd,
        )?
        .is_native_compatible()
    );
    let second_roundtrip = mfd::import(&second_path)?;
    assert!(
        second_roundtrip.warnings.is_empty(),
        "{:?}",
        second_roundtrip.warnings
    );
    assert_optional_declarations(&second_roundtrip.project);
    assert_eq!(
        second_roundtrip.project.graph.nodes.len(),
        roundtrip.project.graph.nodes.len(),
    );

    for project in [
        &imported.project,
        &serialized,
        &roundtrip.project,
        &second_roundtrip.project,
    ] {
        let mut parameters = RuntimeParameters::new();
        parameters.insert("Required", Value::String("required".into()))?;
        parameters.insert("RequiredPreview", Value::String("runtime".into()))?;
        parameters.insert("Lazy", Value::String("host-lazy".into()))?;
        let output = run_optional(project, &parameters)?;
        for (name, expected) in [
            ("Prefix", Value::String("B".into())),
            ("Count", Value::Int(7)),
            ("Ratio", Value::Float(8.5)),
            ("PreviewCount", Value::Int(8)),
            ("RequiredPreview", Value::String("runtime".into())),
            ("Required", Value::String("required".into())),
            ("Lazy", Value::String("host-lazy".into())),
        ] {
            assert_eq!(
                output.field(name).and_then(Instance::as_scalar),
                Some(&expected)
            );
        }

        let mut overrides = RuntimeParameters::new();
        overrides.insert("Required", Value::String("required".into()))?;
        overrides.insert("Lazy", Value::String("host-lazy".into()))?;
        overrides.insert("Prefix", Value::String("F".into()))?;
        overrides.insert("Count", Value::String(" 42 ".into()))?;
        overrides.insert("Ratio", Value::String("3.25".into()))?;
        overrides.insert("PreviewCount", Value::String("17".into()))?;
        overrides.insert("RequiredPreview", Value::String("ignored".into()))?;
        let output = run_optional(project, &overrides)?;
        assert_eq!(
            output.field("Prefix").and_then(Instance::as_scalar),
            Some(&Value::String("F".into()))
        );
        assert_eq!(
            output.field("Count").and_then(Instance::as_scalar),
            Some(&Value::Int(42))
        );
        assert_eq!(
            output.field("Ratio").and_then(Instance::as_scalar),
            Some(&Value::Float(3.25))
        );
        assert_eq!(
            output.field("PreviewCount").and_then(Instance::as_scalar),
            Some(&Value::Int(17))
        );
        assert_eq!(
            output
                .field("RequiredPreview")
                .and_then(Instance::as_scalar),
            Some(&Value::String("ignored".into()))
        );

        let mut supplied_null = RuntimeParameters::new();
        supplied_null.insert("Required", Value::String("required".into()))?;
        supplied_null.insert("RequiredPreview", Value::Null)?;
        supplied_null.insert("Lazy", Value::String("host-lazy".into()))?;
        supplied_null.insert("Prefix", Value::Null)?;
        supplied_null.insert("Count", Value::Null)?;
        supplied_null.insert("PreviewCount", Value::Null)?;
        let output = run_optional(project, &supplied_null)?;
        assert_eq!(
            output.field("Prefix").and_then(Instance::as_scalar),
            Some(&Value::Null)
        );
        assert_eq!(
            output.field("Count").and_then(Instance::as_scalar),
            Some(&Value::Null)
        );
        assert_eq!(
            output.field("PreviewCount").and_then(Instance::as_scalar),
            Some(&Value::Null)
        );
        assert_eq!(
            output
                .field("RequiredPreview")
                .and_then(Instance::as_scalar),
            Some(&Value::Null)
        );

        let mut wrong = RuntimeParameters::new();
        wrong.insert("Required", Value::String("required".into()))?;
        wrong.insert("RequiredPreview", Value::String("runtime".into()))?;
        wrong.insert("Lazy", Value::String("host-lazy".into()))?;
        wrong.insert("Count", Value::Bool(false))?;
        assert!(matches!(
            run_optional(project, &wrong),
            Err(engine::EngineError::RuntimeParameterType { name, expected: ScalarType::Int, found: "bool", .. }) if name == "Count"
        ));
        let mut wrong_preview = RuntimeParameters::new();
        wrong_preview.insert("Required", Value::String("required".into()))?;
        wrong_preview.insert("RequiredPreview", Value::String("runtime".into()))?;
        wrong_preview.insert("Lazy", Value::String("host-lazy".into()))?;
        wrong_preview.insert("PreviewCount", Value::Bool(false))?;
        assert!(matches!(
            run_optional(project, &wrong_preview),
            Err(engine::EngineError::RuntimeParameterType { name, expected: ScalarType::Int, found: "bool", .. }) if name == "PreviewCount"
        ));

        let mut missing_lazy = RuntimeParameters::new();
        missing_lazy.insert("Required", Value::String("required".into()))?;
        missing_lazy.insert("RequiredPreview", Value::String("runtime".into()))?;
        assert!(matches!(
            run_optional(project, &missing_lazy),
            Err(engine::EngineError::MissingRuntimeParameter { name, .. }) if name == "Missing"
        ));
        let mut without_required = RuntimeParameters::new();
        without_required.insert("RequiredPreview", Value::String("runtime".into()))?;
        without_required.insert("Lazy", Value::String("host-lazy".into()))?;
        assert!(matches!(
            run_optional(project, &without_required),
            Err(engine::EngineError::MissingRuntimeParameter { name, .. }) if name == "Required"
        ));

        let mut required_only = RuntimeParameters::new();
        required_only.insert("Required", Value::String("required".into()))?;
        required_only.insert("Lazy", Value::String("host-lazy".into()))?;
        assert!(matches!(
            run_optional(project, &required_only),
            Err(engine::EngineError::MissingRuntimeParameter { name, .. }) if name == "RequiredPreview"
        ));
        let preview_output =
            run_optional_for_purpose(project, &required_only, ExecutionPurpose::Preview)?;
        assert_eq!(
            preview_output
                .field("PreviewCount")
                .and_then(Instance::as_scalar),
            Some(&Value::Int(9))
        );
        assert_eq!(
            preview_output
                .field("RequiredPreview")
                .and_then(Instance::as_scalar),
            Some(&Value::String("legacy".into()))
        );
        let overridden_preview =
            run_optional_for_purpose(project, &overrides, ExecutionPurpose::Preview)?;
        assert_eq!(
            overridden_preview
                .field("PreviewCount")
                .and_then(Instance::as_scalar),
            Some(&Value::Int(17))
        );
        assert_eq!(
            overridden_preview
                .field("RequiredPreview")
                .and_then(Instance::as_scalar),
            Some(&Value::String("ignored".into()))
        );
    }
    Ok(())
}

#[test]
fn preview_text_is_preserved_raw_and_checked_only_during_preview()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    let design = write_optional_design(&directory.0)?;
    let text = std::fs::read_to_string(&design)?
        .replace("previewvalue=\"9\"", "previewvalue=\"not-an-integer\"")
        .replace("previewvalue=\"legacy\"", "previewvalue=\"\"");
    std::fs::write(&design, text)?;
    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let exported = directory.0.join("raw-preview-roundtrip.mfd");
    assert!(
        mfd::export_with_profile(&imported.project, &exported, mfd::ExportProfile::NativeMfd,)?
            .is_native_compatible()
    );
    let text = std::fs::read_to_string(&exported)?;
    assert!(text.contains("previewvalue=\"not-an-integer\" usepreviewvalue=\"1\""));
    assert!(text.contains("previewvalue=\"\" usepreviewvalue=\"1\""));
    let restored = mfd::import(&exported)?;
    assert!(restored.warnings.is_empty(), "{:?}", restored.warnings);

    for project in [&imported.project, &restored.project] {
        let mut hosts = RuntimeParameters::new();
        hosts.insert("Required", Value::String("required".into()))?;
        hosts.insert("Lazy", Value::String("lazy".into()))?;
        hosts.insert("RequiredPreview", Value::String("runtime".into()))?;
        let run = run_optional(project, &hosts)?;
        assert_eq!(
            run.field("PreviewCount").and_then(Instance::as_scalar),
            Some(&Value::Int(8)),
        );
        assert!(matches!(
            run_optional_for_purpose(project, &hosts, ExecutionPurpose::Preview),
            Err(engine::EngineError::RuntimeParameterType {
                name,
                expected: ScalarType::Int,
                found: "string",
                ..
            }) if name == "PreviewCount"
        ));
        hosts.insert("PreviewCount", Value::Int(8))?;
        let preview = run_optional_for_purpose(project, &hosts, ExecutionPurpose::Preview)?;
        assert_eq!(
            preview.field("PreviewCount").and_then(Instance::as_scalar),
            Some(&Value::Int(8)),
        );
        let mut with_null = RuntimeParameters::new();
        with_null.insert("Required", Value::String("required".into()))?;
        with_null.insert("Lazy", Value::String("lazy".into()))?;
        with_null.insert("PreviewCount", Value::Int(8))?;
        with_null.insert("RequiredPreview", Value::Null)?;
        let preview = run_optional_for_purpose(project, &with_null, ExecutionPurpose::Preview)?;
        assert_eq!(
            preview
                .field("RequiredPreview")
                .and_then(Instance::as_scalar),
            Some(&Value::Null),
        );
        // A supplied Null is distinct from an absent host value. Only the
        // latter reads this empty lexical preview.
        let mut without_required_preview = RuntimeParameters::new();
        without_required_preview.insert("Required", Value::String("required".into()))?;
        without_required_preview.insert("Lazy", Value::String("lazy".into()))?;
        without_required_preview.insert("PreviewCount", Value::Int(8))?;
        let preview = run_optional_for_purpose(
            project,
            &without_required_preview,
            ExecutionPurpose::Preview,
        )?;
        assert_eq!(
            preview
                .field("RequiredPreview")
                .and_then(Instance::as_scalar),
            Some(&Value::String(String::new())),
        );
    }
    Ok(())
}

#[test]
fn preview_attribute_roundtrips_xml_whitespace_and_escapes()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    let design = write_optional_design(&directory.0)?;
    let text = std::fs::read_to_string(&design)?.replace(
        "previewvalue=\"legacy\"",
        "previewvalue=\"A&#xD;&#xA;&#x9;&amp;&quot;&lt;\"",
    );
    std::fs::write(&design, text)?;
    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let expected = "A\r\n\t&\"<";
    assert!(imported.project.graph.nodes.values().any(|node| matches!(
        node,
        Node::RuntimeParameter { name, preview: Some(preview), .. }
            if name == "RequiredPreview" && preview == expected
    )));
    let exported = directory.0.join("escaped-preview.mfd");
    assert!(
        mfd::export_with_profile(&imported.project, &exported, mfd::ExportProfile::NativeMfd,)?
            .is_native_compatible()
    );
    let text = std::fs::read_to_string(&exported)?;
    assert!(text.contains("previewvalue=\"A&#xD;&#xA;&#x9;&amp;&quot;&lt;\""));
    let restored = mfd::import(&exported)?;
    assert!(restored.project.graph.nodes.values().any(|node| matches!(
        node,
        Node::RuntimeParameter { name, preview: Some(preview), .. }
            if name == "RequiredPreview" && preview == expected
    )));
    Ok(())
}

#[test]
fn optional_preview_without_connected_runtime_default_is_diagnosed_and_not_frozen()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    let design = write_optional_design(&directory.0)?;
    let text = std::fs::read_to_string(&design)?.replace(
        "  <vertex vertexkey=\"103\"><edges><edge vertexkey=\"120\"/></edges></vertex>\n",
        "",
    );
    std::fs::write(&design, text)?;
    let imported = mfd::import(&design)?;
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("optional input parameter `PreviewCount`")
            && warning.contains("no connected runtime default")
            && warning.contains("dependent value skipped")
    }));
    assert!(!imported.project.graph.nodes.values().any(|node| matches!(
        node,
        Node::RuntimeParameter { name, .. } | Node::RuntimeParameterDefault { name, .. }
            if name == "PreviewCount"
    )));
    let mut supplied = RuntimeParameters::new();
    supplied.insert("Required", Value::String("required".into()))?;
    supplied.insert("RequiredPreview", Value::String("runtime".into()))?;
    supplied.insert("Lazy", Value::String("host-lazy".into()))?;
    let output = run_optional(&imported.project, &supplied)?;
    assert!(output.field("PreviewCount").is_none());
    Ok(())
}
