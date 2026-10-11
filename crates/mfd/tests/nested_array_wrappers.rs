use ir::{Instance, Value};
use mapping::Project;
use std::{
    error::Error,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

const PROJECT: &str = include_str!("fixtures/nested_array_wrappers/project.json");

fn evidence() -> Result<PathBuf, Box<dyn Error>> {
    let path = std::env::temp_dir().join(format!(
        "ferrule-nested-array-mfd-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    fs::create_dir(&path)?;
    fs::write(path.join("independent-project.json"), PROJECT)?;
    Ok(path)
}

fn record(
    path: &std::path::Path,
    name: &str,
    value: &impl std::fmt::Debug,
) -> Result<(), std::io::Error> {
    fs::write(
        path.join(format!("{name}.original.txt")),
        format!("{value:#?}\n"),
    )
}

fn complete_values() -> Instance {
    Instance::Repeated(vec![
        Instance::Group(
            vec![(
                "item".into(),
                Instance::Repeated(vec![
                    Instance::Scalar(Value::Int(7)),
                    Instance::Scalar(Value::Int(9)),
                ]),
            )]
            .into(),
        ),
        Instance::Group(
            vec![(
                "item".into(),
                Instance::Repeated(vec![
                    Instance::Scalar(Value::Int(11)),
                    Instance::Scalar(Value::Int(13)),
                    Instance::Scalar(Value::Int(15)),
                ]),
            )]
            .into(),
        ),
    ])
}

#[test]
fn nested_array_wrapper_mfd_paths_and_full_value_roundtrip() -> Result<(), Box<dyn Error>> {
    let path = evidence()?;
    let authored = serde_json::from_str::<Project>(PROJECT);
    record(&path, "authored-project", &authored)?;
    let project = authored?;
    let validation = engine::validate(&project);
    let lowered = codegen::lower(&project);
    record(&path, "authored-admission", &(&validation, &lowered))?;
    assert!(validation.is_empty(), "{validation:?}; {}", path.display());
    let program = lowered.map_err(|error| format!("{error:?}"))?;
    let rust = codegen_rust::emit(
        &program,
        &codegen_rust::Options {
            package_name: "ferrule-nested-array".into(),
            runtime_dependency: codegen_rust::RuntimeDependency::Version("0.1.0".into()),
        },
    );
    let csharp = codegen_csharp::emit(&program);
    record(&path, "authored-emission", &(&rust, &csharp))?;
    rust?;
    csharp?;
    let input = format_json::from_str("[[7,9],[11,13,15]]", &project.source);
    record(&path, "authored-input", &input)?;
    let input = input?;
    let expected = complete_values();
    let output = engine::run(&project, &input);
    record(&path, "authored-output", &output)?;
    assert_eq!(input, expected);
    assert_eq!(output?, expected);

    let design = path.join("roundtrip.mfd");
    let exported = mfd::export(&project, &design);
    record(&path, "export", &exported)?;
    assert!(exported?.is_empty());
    let xml = fs::read_to_string(&design)?;
    let parsed = roxmltree::Document::parse(&xml)?;
    let arrays = parsed
        .descendants()
        .filter(|node| node.has_tag_name("entry") && node.attribute("name") == Some("array"))
        .collect::<Vec<_>>();
    assert_eq!(arrays.len(), 4, "{}", path.display());
    for outer in arrays.iter().filter(|node| {
        node.parent()
            .is_some_and(|parent| parent.attribute("name") == Some("root"))
    }) {
        assert!(outer.attribute("outkey").is_some() || outer.attribute("inpkey").is_some());
        let item = outer
            .children()
            .find(|node| node.has_tag_name("entry"))
            .ok_or("outer item")?;
        assert_eq!(item.attribute("type"), Some("json-item"));
        assert_eq!(item.attribute("name"), Some("item"));
        let inner = item
            .children()
            .find(|node| node.has_tag_name("entry"))
            .ok_or("inner array")?;
        assert_eq!(inner.attribute("name"), Some("array"));
    }
    let imported = mfd::import(&design);
    match &imported {
        Ok(imported) => record(
            &path,
            "reimport",
            &(
                &imported.project,
                &imported.warnings,
                &imported.mapping_path,
            ),
        )?,
        Err(error) => record(&path, "reimport", error)?,
    }
    let imported = imported?;
    let validation = engine::validate(&imported.project);
    let output = engine::run(&imported.project, &input);
    record(&path, "reimport-admission-output", &(&validation, &output))?;
    assert!(
        imported.warnings.is_empty(),
        "{:?}; {}",
        imported.warnings,
        path.display()
    );
    assert_eq!(imported.project.source, project.source);
    assert_eq!(imported.project.target, project.target);
    assert_eq!(imported.project.source_options, project.source_options);
    assert_eq!(imported.project.target_options, project.target_options);
    assert!(validation.is_empty(), "{validation:?}; {}", path.display());
    assert_eq!(output?, expected);
    let reexported = mfd::export(&imported.project, &path.join("second.mfd"));
    record(&path, "second-export", &reexported)?;
    assert!(reexported?.is_empty());
    let second = mfd::import(&path.join("second.mfd"))?;
    record(
        &path,
        "second-complete-project",
        &(&second.project, &second.warnings),
    )?;
    assert_eq!(
        serde_json::to_value(&second.project.graph)?,
        serde_json::to_value(&imported.project.graph)?
    );
    assert_eq!(
        serde_json::to_value(&second.project.root)?,
        serde_json::to_value(&imported.project.root)?
    );
    assert_eq!(engine::run(&second.project, &input)?, expected);
    Ok(())
}

#[test]
fn nested_array_wrapper_non_json_boundaries_refuse_complete_metadata() -> Result<(), Box<dyn Error>>
{
    let path = evidence()?;
    let project: Project = serde_json::from_str(PROJECT)?;
    let schema = &project.source;
    let value = complete_values();
    let xml_read = format_xml::from_str("<Input/>", schema);
    let xml_write = format_xml::to_string(schema, &value);
    let xsd = format_xml::xsd::export(schema);
    let csv_read = format_csv::from_str("item\n7\n", schema, None, true);
    let csv_write = format_csv::to_string(schema, &[value], None, true);
    let edi = format_edi::dialect_of(schema);
    record(
        &path,
        "non-json-originals",
        &(&xml_read, &xml_write, &xsd, &csv_read, &csv_write, &edi),
    )?;
    for result in [xml_read.map(|_| ()), xml_write.map(|_| ()), xsd.map(|_| ())] {
        assert!(
            matches!(result, Err(format_xml::XmlFormatError::UnsupportedSchemaRole {ref node, role:"element", kind:"JSON array wrapper"}) if node == "Input")
        );
    }
    assert!(matches!(
        csv_read,
        Err(format_csv::CsvFormatError::UnsupportedSchema)
    ));
    assert!(matches!(
        csv_write,
        Err(format_csv::CsvFormatError::UnsupportedSchema)
    ));
    assert!(
        matches!(edi, Err(format_edi::EdiFormatError::UnsupportedSchema(ref reason)) if reason == "JSON array-wrapper metadata requires a JSON boundary")
    );
    Ok(())
}
