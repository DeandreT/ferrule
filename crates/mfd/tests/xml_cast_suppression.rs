use ir::{ScalarType, SchemaKind, XmlScalarOutputPolicy};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Package(PathBuf);
impl Package {
    fn new() -> Result<Self, Box<dyn Error>> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_suppress_cast_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        let package = Self(path);
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/target_float_cast");
        for name in ["source.xsd", "target.xsd"] {
            std::fs::copy(root.join(name), package.0.join(name))?;
        }
        let text = std::fs::read_to_string(root.join("cast-first.mfd"))?;
        assert_eq!(
            text.matches("casttotargettypemode=\"cast-in-subtree\"")
                .count(),
            1
        );
        std::fs::write(
            package.0.join("mapping.mfd"),
            text.replace(
                "casttotargettypemode=\"cast-in-subtree\"",
                "casttotargettypemode=\"suppress-cast-in-subtree\"",
            ),
        )?;
        Ok(package)
    }
    fn change(&self, from: &str, to: &str) -> Result<(), Box<dyn Error>> {
        let path = self.0.join("mapping.mfd");
        let text = std::fs::read_to_string(&path)?;
        assert!(text.contains(from));
        std::fs::write(path, text.replace(from, to))?;
        Ok(())
    }
}
impl Drop for Package {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn disabled_cast_preserves_declared_schema_values_and_native_roundtrip()
-> Result<(), Box<dyn Error>> {
    let package = Package::new()?;
    let imported = mfd::import_with_profile(
        &package.0.join("mapping.mfd"),
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )?;
    assert!(
        imported.imported.warnings.is_empty(),
        "{:?}",
        imported.imported.warnings
    );
    let project = &imported.imported.project;
    assert!(matches!(
        &project.target_options.xml_scalar_output,
        Some(XmlScalarOutputPolicy::SuppressCastInSubtreeV1 { .. })
    ));
    assert_eq!(
        project.target.child("Amount").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Float
        }
    );
    let output = package.0.join("native/mapping.mfd");
    assert!(
        mfd::export_with_profile(project, &output, mfd::ExportProfile::NativeMfd)?
            .is_native_compatible()
    );
    let text = std::fs::read_to_string(&output)?;
    assert_eq!(text.matches("suppress-cast-in-subtree").count(), 1);
    assert!(
        std::fs::read_to_string(output.parent().unwrap().join("mapping-target.xsd"))?
            .contains("type=\"xs:double\"")
    );
    let reimported = mfd::import(&output)?;
    assert!(
        reimported.warnings.is_empty(),
        "{:?}\n{}",
        reimported.warnings,
        std::fs::read_to_string(output.parent().unwrap().join("mapping-target.xsd"))?
    );
    assert_eq!(
        reimported.project.target_options.xml_scalar_output,
        project.target_options.xml_scalar_output
    );
    for value in ["9007199254740993", "\t 2.5 \n", "not-a-number", "00029"] {
        let source = format_xml::from_str(
            &format!(
                "<Envelope><Numerator>34</Numerator><Denominator>2</Denominator><Text>{value}</Text><Label>safe</Label></Envelope>"
            ),
            &project.source,
        )?;
        for project in [project, &reimported.project] {
            let result = engine::run(project, &source)?;
            assert_eq!(
                result.field("Amount").unwrap(),
                &ir::Instance::Repeated(vec![
                    ir::Instance::Scalar(ir::Value::String(value.into())),
                    ir::Instance::Scalar(ir::Value::Float(17.0))
                ])
            );
            let xml = format_xml::to_string_with_options(
                &project.target,
                &result,
                &format_xml::XmlWriteOptions {
                    scalar_output: project.target_options.xml_scalar_output.clone(),
                    ..Default::default()
                },
            )?;
            let doc = roxmltree::Document::parse(&xml)?;
            assert_eq!(
                doc.root_element()
                    .children()
                    .filter(|node| node.is_element())
                    .map(|node| (node.tag_name().name(), node.text().unwrap_or_default()))
                    .collect::<Vec<_>>(),
                [("Amount", value), ("Amount", "17"), ("Label", "safe")]
            );
        }
    }
    Ok(())
}

#[test]
fn unsupported_requests_remain_unresolved_and_non_executable() -> Result<(), Box<dyn Error>> {
    for (from, to) in [
        ("suppress-cast-in-subtree", "future-cast-mode"),
        (
            "<entry name=\"Amount\" inpkey=\"20\"/>",
            "<entry name=\"Amount\" inpkey=\"20\" casttotargettypemode=\"cast-in-subtree\"/>",
        ),
        (
            "XSLTDefaultOutput=\"1\"",
            "XSLTDefaultOutput=\"1\" UnknownSetting=\"true\"",
        ),
        ("AddSchemaLocation=\"0\"", "AddSchemaLocation=\"1\""),
    ] {
        let package = Package::new()?;
        package.change(from, to)?;
        let imported = mfd::import(&package.0.join("mapping.mfd"))?;
        assert!(matches!(
            imported.project.target_options.xml_scalar_output,
            Some(XmlScalarOutputPolicy::Unresolved { .. })
        ));
        assert!(!imported.warnings.is_empty());
        assert_eq!(
            imported
                .warnings
                .iter()
                .filter(|warning| warning.starts_with("XML scalar output"))
                .count(),
            1
        );
        assert!(engine::run(&imported.project, &ir::Instance::Scalar(ir::Value::Null)).is_err());
        assert!(
            mfd::import_with_profile(
                &package.0.join("mapping.mfd"),
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable
            )
            .is_err()
        );
        assert!(codegen::lower(&imported.project).is_err());
        assert!(mfd::export(&imported.project, &package.0.join("bad/mapping.mfd")).is_err());
        assert!(!package.0.join("bad").exists());
    }
    Ok(())
}

#[test]
fn absent_schema_location_setting_keeps_the_flat_profile() -> Result<(), Box<dyn Error>> {
    // An absent setting is the reference default for every XML target, which
    // ordinary import already treats as no schema hint.
    let package = Package::new()?;
    package.change(" AddSchemaLocation=\"0\"", "")?;
    let imported = mfd::import(&package.0.join("mapping.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(matches!(
        imported.project.target_options.xml_scalar_output,
        Some(XmlScalarOutputPolicy::SuppressCastInSubtreeV1 { .. })
    ));
    Ok(())
}

#[test]
fn inexact_occurrence_bounds_keep_the_request_through_the_document_profile()
-> Result<(), Box<dyn Error>> {
    // The flat profile retains exact declarations, so it refuses bounds it
    // cannot represent; the request itself is still kept and executed.
    for (from, to) in [
        ("maxOccurs=\"unbounded\"", "maxOccurs=\"2\""),
        ("minOccurs=\"0\"", "minOccurs=\"2\""),
        ("minOccurs=\"0\"", "minOccurs=\"1\""),
        (" minOccurs=\"0\"", ""),
    ] {
        let package = Package::new()?;
        let path = package.0.join("target.xsd");
        let text = std::fs::read_to_string(&path)?;
        assert!(text.contains(from));
        std::fs::write(&path, text.replace(from, to))?;
        let imported = mfd::import(&package.0.join("mapping.mfd"))?;
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        assert_eq!(
            imported.project.target_options.xml_scalar_output,
            Some(XmlScalarOutputPolicy::SuppressCastInDocumentV1 {})
        );
        let source = format_xml::from_str(
            "<Envelope><Numerator>34</Numerator><Denominator>2</Denominator><Text>00029</Text><Label>safe</Label></Envelope>",
            &imported.project.source,
        )?;
        let output = engine::run(&imported.project, &source)?;
        let xml = format_xml::to_string_with_options(
            &imported.project.target,
            &output,
            &format_xml::XmlWriteOptions {
                scalar_output: imported.project.target_options.xml_scalar_output.clone(),
                ..Default::default()
            },
        )?;
        assert!(xml.contains("<Amount>00029</Amount>"), "{xml}");
    }
    Ok(())
}

#[test]
fn unsupported_schema_root_settings_are_not_discarded() -> Result<(), Box<dyn Error>> {
    for attribute in ["abstract=\"true\"", "block=\"#all\"", "default=\"value\""] {
        let package = Package::new()?;
        let path = package.0.join("target.xsd");
        let text = std::fs::read_to_string(&path)?;
        std::fs::write(
            &path,
            text.replace(
                "name=\"Envelope\"",
                &format!("name=\"Envelope\" {attribute}"),
            ),
        )?;
        let imported = mfd::import(&package.0.join("mapping.mfd"))?;
        assert!(matches!(
            imported.project.target_options.xml_scalar_output,
            Some(XmlScalarOutputPolicy::Unresolved { .. })
        ));
    }
    Ok(())
}

#[test]
fn datetime_helpers_stay_explicit_under_a_suppressed_document() -> Result<(), Box<dyn Error>> {
    let package = Package::new()?;
    let mut project = mfd::import(&package.0.join("mapping.mfd"))?.project;
    let next = project.graph.nodes.keys().next_back().copied().unwrap() + 1;
    project.graph.nodes.insert(
        next,
        mapping::Node::Const {
            value: ir::Value::String("2026-10-28".into()),
        },
    );
    project.graph.nodes.insert(
        next + 1,
        mapping::Node::Call {
            function: "coerce_datetime".into(),
            args: vec![next],
        },
    );
    project
        .root
        .bindings
        .iter_mut()
        .find(|binding| binding.target_field == "Label")
        .unwrap()
        .node = next + 1;
    let output = package.0.join("mixed/mapping.mfd");
    assert!(!mfd::preflight_export(&project, &output)?.is_native_compatible());
    mfd::export(&project, &output)?;
    let text = std::fs::read_to_string(&output)?;
    assert_eq!(text.matches("suppress-cast-in-subtree").count(), 1);
    assert!(text.contains("coerce_datetime"));
    assert!(!text.contains("\"cast-in-subtree\""));
    let roundtrip = mfd::import(&output)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    let source = format_xml::from_str(
        "<Envelope><Numerator>34</Numerator><Denominator>2</Denominator><Text>00029</Text><Label>safe</Label></Envelope>",
        &project.source,
    )?;
    assert_eq!(
        engine::run(&project, &source)?,
        engine::run(&roundtrip.project, &source)?
    );
    Ok(())
}

#[test]
fn suppressed_export_never_reorders_competing_field_evaluation() -> Result<(), Box<dyn Error>> {
    let package = Package::new()?;
    let mut project = mfd::import(&package.0.join("mapping.mfd"))?.project;
    let label = project
        .root
        .bindings
        .iter()
        .position(|binding| binding.target_field == "Label")
        .unwrap();
    let first = project.root.bindings.remove(label);
    project.root.bindings.insert(0, first);
    let output = package.0.join("reordered/mapping.mfd");
    let error = mfd::export(&project, &output).unwrap_err().to_string();
    assert!(
        error.contains("target binding order cannot be preserved"),
        "{error}"
    );
    assert!(!output.parent().unwrap().exists());
    Ok(())
}

fn nested_project() -> Result<mapping::Project, Box<dyn Error>> {
    let order = |name: &str| {
        serde_json::json!({"name": name, "kind": {"kind": "group", "children": [
            {"name": "Orders", "kind": {"kind": "group", "children": [
                {"name": "Order", "repeating": true, "kind": {"kind": "group", "children": [
                    {"name": "Id", "kind": {"kind": "scalar", "ty": "int"}},
                    {"name": "Label", "kind": {"kind": "scalar", "ty": "string"}},
                    {"name": "Price", "kind": {"kind": "scalar", "ty": "float"}}
                ]}}
            ]}}
        ]}})
    };
    let project = serde_json::json!({
        "source": order("Input"),
        "target": order("Output"),
        "source_path": "input.xml",
        "target_path": "output.xml",
        "source_options": {"xml_document": true},
        "target_options": {"xml_document": true, "xml_scalar_output": {"mode": "suppress_cast_in_document_v1"}},
        "graph": {"nodes": {
            "0": {"kind": "const", "value": "00000.00"},
            "1": {"kind": "call", "function": "format_number", "args": [2, 0]},
            "2": {"kind": "source_field", "path": ["Price"], "frame": ["Orders", "Order"]},
            "3": {"kind": "source_field", "path": ["Id"], "frame": ["Orders", "Order"]},
            "4": {"kind": "source_field", "path": ["Label"], "frame": ["Orders", "Order"]}
        }},
        "root": {"target_field": "", "children": [{"target_field": "Orders", "children": [
            {"target_field": "Order", "source": ["Orders", "Order"], "bindings": [
                {"target_field": "Id", "node": 3},
                {"target_field": "Label", "node": 4},
                {"target_field": "Price", "node": 1}
            ]}
        ]}]}
    });
    Ok(serde_json::from_value(project)?)
}

fn nested_output(project: &mapping::Project) -> Result<String, Box<dyn Error>> {
    let source = format_xml::from_str(
        "<Input><Orders><Order><Id>7</Id><Label>a</Label><Price>25</Price></Order>\
         <Order><Id>8</Id><Label>b</Label><Price>2.3</Price></Order></Orders></Input>",
        &project.source,
    )?;
    let output = engine::run(project, &source)?;
    Ok(format_xml::to_string_with_options(
        &project.target,
        &output,
        &format_xml::XmlWriteOptions {
            scalar_output: project.target_options.xml_scalar_output.clone(),
            declaration: false,
            indent: false,
            ..Default::default()
        },
    )?)
}

const NESTED_OUTPUT: &str = "<Output><Orders><Order><Id>7</Id><Label>a</Label><Price>00025.00</Price></Order>\
    <Order><Id>8</Id><Label>b</Label><Price>00002.30</Price></Order></Orders></Output>";

#[test]
fn document_profile_preserves_nested_bindings_through_native_roundtrip()
-> Result<(), Box<dyn Error>> {
    let project = nested_project()?;
    assert_eq!(nested_output(&project)?, NESTED_OUTPUT);
    let mut enabled = project.clone();
    enabled.target_options.xml_scalar_output = None;
    let normalized = nested_output(&enabled)?;
    assert!(normalized.contains("<Price>25</Price>"), "{normalized}");

    let package = Package::new()?;
    let output = package.0.join("nested/mapping.mfd");
    assert!(mfd::preflight_export(&project, &output)?.is_native_compatible());
    mfd::export_with_profile(&project, &output, mfd::ExportProfile::NativeMfd)?;
    let text = std::fs::read_to_string(&output)?;
    assert_eq!(text.matches("casttotargettypemode").count(), 1);
    assert_eq!(text.matches("suppress-cast-in-subtree").count(), 1);
    let reimported = mfd::import(&output)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(
        reimported.project.target_options.xml_scalar_output,
        project.target_options.xml_scalar_output
    );
    assert_eq!(nested_output(&reimported.project)?, NESTED_OUTPUT);
    Ok(())
}

#[test]
fn document_profile_import_refuses_overrides_and_unretained_schema_settings()
-> Result<(), Box<dyn Error>> {
    let package = Package::new()?;
    let output = package.0.join("nested/mapping.mfd");
    mfd::export_with_profile(&nested_project()?, &output, mfd::ExportProfile::NativeMfd)?;
    let design = std::fs::read_to_string(&output)?;
    let schema_path = package.0.join("nested/mapping-target.xsd");
    let schema = std::fs::read_to_string(&schema_path)?;
    let write = |design: &str, schema: &str| -> Result<mapping::Project, Box<dyn Error>> {
        std::fs::write(&output, design)?;
        std::fs::write(&schema_path, schema)?;
        Ok(mfd::import(&output)?.project)
    };
    let unresolved = |project: &mapping::Project| {
        matches!(
            project.target_options.xml_scalar_output,
            Some(XmlScalarOutputPolicy::Unresolved { .. })
        )
    };
    let price = design
        .rfind("<entry name=\"Price\"")
        .ok_or("no target Price")?;
    let mut nested_mode = design.clone();
    nested_mode.insert_str(
        price + "<entry".len(),
        " casttotargettypemode=\"cast-in-subtree\"",
    );
    assert!(unresolved(&write(&nested_mode, &schema)?));
    for (from, to) in [
        ("name=\"Price\"", "name=\"Price\" nillable=\"true\""),
        ("name=\"Price\"", "name=\"Price\" default=\"1\""),
        ("name=\"Order\"", "name=\"Order\" abstract=\"true\""),
        ("<xs:complexType>", "<xs:complexType mixed=\"true\">"),
    ] {
        assert!(schema.contains(from), "{from}");
        assert!(
            unresolved(&write(&design, &schema.replacen(from, to, 1))?),
            "{to}"
        );
    }
    // Facets, named types, explicit defaults and authoring metadata do not
    // affect written values.
    let restricted = schema
        .replacen("name=\"Order\"", "name=\"Order\" nillable=\"false\"", 1)
        .replace(
            "<xs:element name=\"Price\" type=\"xs:decimal\"/>",
            "<xs:element name=\"Price\"><xs:simpleType><xs:restriction base=\"xs:decimal\">\
         <xs:fractionDigits value=\"2\"/><xs:pattern value=\"[0-9]*\\.[0-9][0-9]\"/>\
         </xs:restriction></xs:simpleType></xs:element>",
        );
    assert_ne!(restricted, schema);
    // The target is the last component in the exported design.
    let target = design.rfind("<document ").ok_or("no target document")?;
    let data = design.rfind("</data>").ok_or("no target data")?;
    assert!(target < data);
    let mut authored = design.clone();
    authored.insert_str(data, "<wsdl/>");
    authored.insert_str(target + "<document ".len(), "sps=\"orders.sps\" ");
    let project = write(&authored, &restricted)?;
    assert_eq!(
        project.target_options.xml_scalar_output,
        Some(XmlScalarOutputPolicy::SuppressCastInDocumentV1 {})
    );
    assert_eq!(nested_output(&project)?, NESTED_OUTPUT);
    Ok(())
}
