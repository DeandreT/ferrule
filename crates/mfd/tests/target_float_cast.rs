use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use engine::EngineError;
use functions::FunctionError;
use mapping::{Node, Project};

struct Package(PathBuf);
impl Package {
    fn new() -> Result<Self, Box<dyn Error>> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_float_cast_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        let package = Self(path);
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/target_float_cast");
        for name in ["source.xsd", "target.xsd"] {
            std::fs::copy(fixture.join(name), package.0.join(name))?;
        }
        std::fs::copy(
            fixture.join("cast-first.mfd"),
            package.0.join("mapping.mfd"),
        )?;
        Ok(package)
    }
    fn import(&self) -> Result<Project, Box<dyn Error>> {
        let imported = mfd::import(&self.0.join("mapping.mfd"))?;
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        assert!(engine::validate(&imported.project).is_empty());
        Ok(imported.project)
    }
    fn change(&self, from: &str, to: &str) -> Result<(), Box<dyn Error>> {
        let file = self.0.join("mapping.mfd");
        let text = std::fs::read_to_string(&file)?;
        assert!(text.contains(from));
        std::fs::write(file, text.replace(from, to))?;
        Ok(())
    }
}
impl Drop for Package {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn input(
    project: &Project,
    numerator: &str,
    denominator: &str,
    text: &str,
) -> Result<ir::Instance, Box<dyn Error>> {
    Ok(format_xml::from_str(
        &format!(
            "<Envelope><Numerator>{numerator}</Numerator><Denominator>{denominator}</Denominator><Text>{text}</Text><Label>safe</Label></Envelope>"
        ),
        &project.source,
    )?)
}

fn helpers(project: &Project) -> usize {
    project
        .graph
        .nodes
        .values()
        .filter(|node| matches!(node, Node::Call { function, .. } if function == "coerce_float"))
        .count()
}

#[test]
fn explicit_double_cast_selects_the_first_repeated_binding_failure() -> Result<(), Box<dyn Error>> {
    for cast_first in [true, false] {
        let package = Package::new()?;
        if !cast_first {
            package.change(
                "<edge vertexkey=\"120\"/>",
                "<edge vertexkey=\"temporary\"/>",
            )?;
            package.change("<edge vertexkey=\"20\"/>", "<edge vertexkey=\"120\"/>")?;
            package.change(
                "<edge vertexkey=\"temporary\"/>",
                "<edge vertexkey=\"20\"/>",
            )?;
        }
        let project = package.import()?;
        assert_eq!(helpers(&project), 1);
        let cast_error = || {
            EngineError::Function(FunctionError::InvalidArgument {
                function: "coerce_float",
                message: "requires finite floating-point text",
            })
        };
        for (numerator, denominator, text, expected) in [
            ("34", "2", "not-a-number", cast_error()),
            (
                "1",
                "0",
                "29",
                EngineError::Function(FunctionError::DivideByZero),
            ),
            (
                "1",
                "0",
                "not-a-number",
                if cast_first {
                    cast_error()
                } else {
                    EngineError::Function(FunctionError::DivideByZero)
                },
            ),
        ] {
            assert_eq!(
                engine::run(&project, &input(&project, numerator, denominator, text)?),
                Err(expected)
            );
        }
        let expected = if cast_first {
            vec![29.0, 17.0]
        } else {
            vec![17.0, 29.0]
        };
        let output = engine::run(&project, &input(&project, "34", "2", "29")?)?;
        let amounts = output.field("Amount").ok_or("Amount")?;
        assert_eq!(
            amounts,
            &ir::Instance::Repeated(
                expected
                    .into_iter()
                    .map(|value| ir::Instance::Scalar(ir::Value::Float(value)))
                    .collect()
            )
        );
    }
    Ok(())
}

#[test]
fn float_cast_admission_requires_its_exact_raw_metadata() -> Result<(), Box<dyn Error>> {
    for (from, to) in [
        (
            "casttotargettypemode=\"cast-in-subtree\"",
            "casttotargettypemode=\"disabled\"",
        ),
        (
            "<entry name=\"Text\" outkey=\"12\"/>",
            "<entry name=\"Text\" outkey=\"12\" condition=\"unknown\"/>",
        ),
        (
            "<entry name=\"Amount\" inpkey=\"20\"/>",
            "<entry name=\"Amount\" inpkey=\"20\" casttotargettypemode=\"disabled\"/>",
        ),
    ] {
        let package = Package::new()?;
        package.change(from, to)?;
        let imported = mfd::import(&package.0.join("mapping.mfd"))?;
        assert_eq!(helpers(&imported.project), 0, "{from}");
    }
    for replacement in ["xs:float", "xs:decimal"] {
        let package = Package::new()?;
        let schema = package.0.join("target.xsd");
        std::fs::write(
            &schema,
            std::fs::read_to_string(&schema)?.replace("xs:double", replacement),
        )?;
        assert_eq!(helpers(&package.import()?), 0);
    }
    Ok(())
}

#[test]
fn unmarked_native_double_target_uses_the_enabled_cast_default() -> Result<(), Box<dyn Error>> {
    let package = Package::new()?;
    package.change(" casttotargettypemode=\"cast-in-subtree\"", "")?;
    let project = package.import()?;
    assert_eq!(helpers(&project), 1);
    assert_eq!(
        engine::run(&project, &input(&project, "1", "0", "not-a-number")?),
        Err(EngineError::Function(FunctionError::InvalidArgument {
            function: "coerce_float",
            message: "requires finite floating-point text",
        }))
    );
    package.change(
        "XSLTDefaultOutput=\"1\"",
        "XSLTDefaultOutput=\"1\" UnknownCastPolicy=\"1\"",
    )?;
    let imported = mfd::import(&package.0.join("mapping.mfd"))?;
    assert_eq!(helpers(&imported.project), 0);
    Ok(())
}

#[test]
fn float_extension_export_keeps_helper_and_failure_selection() -> Result<(), Box<dyn Error>> {
    let package = Package::new()?;
    let project = package.import()?;
    assert_native_refusal(
        &project,
        &package.0.join("native/mapping.mfd"),
        "coerce_float",
    )?;
    let destination = package.0.join("extension/mapping.mfd");
    mfd::export(&project, &destination)?;
    let text = std::fs::read_to_string(&destination)?;
    let document = roxmltree::Document::parse(&text)?;
    assert_eq!(
        document
            .descendants()
            .filter(|node| node.attribute("casttotargettypemode") == Some("cast-in-subtree"))
            .count(),
        0
    );
    assert!(document.descendants().any(
        |node| node.has_tag_name("component") && node.attribute("name") == Some("coerce_float")
    ));
    let imported = mfd::import(&destination)?;
    assert!(imported.warnings.is_empty());
    assert_eq!(helpers(&imported.project), 1, "{text}");
    for (numerator, denominator, value) in [
        ("34", "2", "29"),
        ("1", "0", "29"),
        ("1", "0", "not-a-number"),
    ] {
        let source = input(&project, numerator, denominator, value)?;
        assert_eq!(
            engine::run(&project, &source),
            engine::run(&imported.project, &source)
        );
    }
    Ok(())
}

#[test]
fn float_cast_raw_ports_and_canonical_metadata_must_be_unambiguous() -> Result<(), Box<dyn Error>> {
    for (from, to) in [
        (
            "<entry name=\"Text\" outkey=\"12\"/>",
            "<entry name=\"Text\" outkey=\"12\"/><entry name=\"Text\" outkey=\"12\"/>",
        ),
        (
            "<entry name=\"Amount\" inpkey=\"20\"/>",
            "<entry name=\"Amount\" inpkey=\"20\"/><entry name=\"Amount\" inpkey=\"20\"/>",
        ),
        (
            "<entry name=\"Text\" outkey=\"12\"/>",
            "<entry name=\"Text\" outkey=\"12\" ferrule-kind=\"scalar\" ferrule-repeating=\"0\" datatype=\"decimal\"/>",
        ),
        (
            "<entry name=\"Text\" outkey=\"12\"/>",
            "<entry name=\"Text\" outkey=\"12\" ns=\"1\"/>",
        ),
    ] {
        let package = Package::new()?;
        package.change(from, to)?;
        let imported = mfd::import(&package.0.join("mapping.mfd"))?;
        assert_eq!(helpers(&imported.project), 0, "{to}");
    }
    for attribute in ["minOccurs=\"0\"", "nillable=\"true\"", "fixed=\"29\""] {
        let package = Package::new()?;
        let path = package.0.join("source.xsd");
        let text = std::fs::read_to_string(&path)?;
        std::fs::write(
            path,
            text.replace(
                "name=\"Text\" type=\"xs:string\"",
                &format!("name=\"Text\" type=\"xs:string\" {attribute}"),
            ),
        )?;
        assert_eq!(helpers(&package.import()?), 0, "{attribute}");
    }
    Ok(())
}

fn first_cast(project: &Project) -> (u32, u32) {
    project
        .graph
        .nodes
        .iter()
        .find_map(|(&id, node)| match node {
            Node::Call { function, args } if function == "coerce_float" => Some((id, args[0])),
            _ => None,
        })
        .unwrap()
}

fn assert_native_refusal(
    project: &Project,
    destination: &Path,
    helper: &str,
) -> Result<(), Box<dyn Error>> {
    let report = mfd::preflight_export(project, destination)?;
    assert!(!report.is_native_compatible());
    assert!(report.issues.iter().any(|issue| issue.component == helper));
    let error =
        mfd::export_with_profile(project, destination, mfd::ExportProfile::NativeMfd).unwrap_err();
    assert!(error.to_string().contains(helper));
    assert!(!destination.parent().unwrap().exists());
    Ok(())
}

#[test]
fn shared_helpers_and_unwrapped_siblings_do_not_lose_conversion_semantics()
-> Result<(), Box<dyn Error>> {
    for shared in [true, false] {
        let package = Package::new()?;
        let mut project = package.import()?;
        let (call, argument) = first_cast(&project);
        if shared {
            project
                .root
                .bindings
                .iter_mut()
                .find(|binding| binding.target_field == "Label")
                .unwrap()
                .node = call;
        } else {
            project
                .root
                .bindings
                .iter_mut()
                .find(|binding| binding.target_field == "Amount" && binding.node != call)
                .unwrap()
                .node = argument;
        }
        assert_native_refusal(
            &project,
            &package.0.join("refused/mapping.mfd"),
            "coerce_float",
        )?;
        let extension = package.0.join("extension/mapping.mfd");
        mfd::export(&project, &extension)?;
        let text = std::fs::read_to_string(&extension)?;
        assert!(!text.contains("casttotargettypemode"));
        let roundtrip = mfd::import(&extension)?;
        assert!(roundtrip.warnings.is_empty());
        assert_eq!(helpers(&roundtrip.project), 1);
        let source = input(&project, "34", "2", "29")?;
        assert_eq!(
            engine::run(&project, &source),
            engine::run(&roundtrip.project, &source)
        );
    }
    Ok(())
}

fn with_datetime(mut project: Project) -> Project {
    let ir::SchemaKind::Group { children, .. } = &mut project.source.kind else {
        panic!("source group")
    };
    children.push(ir::SchemaNode::scalar("Date", ir::ScalarType::String));
    let ir::SchemaKind::Group { children, .. } = &mut project.target.kind else {
        panic!("target group")
    };
    children.push(ir::SchemaNode::scalar("Received", ir::ScalarType::String));
    let field = project.graph.nodes.keys().next_back().copied().unwrap() + 1;
    let cast = field + 1;
    project.graph.nodes.insert(
        field,
        Node::SourceField {
            path: vec!["Date".into()],
            frame: None,
        },
    );
    project.graph.nodes.insert(
        cast,
        Node::Call {
            function: "coerce_datetime".into(),
            args: vec![field],
        },
    );
    // Base field ports precede the extra Amount occurrence in canonical MFDs.
    project.root.bindings.insert(
        project.root.bindings.len() - 1,
        mapping::Binding {
            target_field: "Received".into(),
            node: cast,
        },
    );
    project
}

#[test]
fn cross_field_failure_order_is_not_silently_changed_by_export() -> Result<(), Box<dyn Error>> {
    let package = Package::new()?;
    let mut project = package.import()?;
    let (cast, _) = first_cast(&project);
    let arithmetic = project
        .root
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Amount" && binding.node != cast)
        .unwrap()
        .node;
    let label = project
        .root
        .bindings
        .iter()
        .position(|binding| binding.target_field == "Label")
        .unwrap();
    let mut first = project.root.bindings.remove(label);
    first.node = arithmetic;
    project.root.bindings.insert(0, first);
    assert_eq!(
        engine::run(&project, &input(&project, "1", "0", "not-a-number")?),
        Err(EngineError::Function(FunctionError::DivideByZero))
    );
    for native in [false, true] {
        let destination = package.0.join(if native {
            "native/mapping.mfd"
        } else {
            "extension/mapping.mfd"
        });
        let error = if native {
            mfd::export_with_profile(&project, &destination, mfd::ExportProfile::NativeMfd)
                .unwrap_err()
        } else {
            mfd::export(&project, &destination).unwrap_err()
        };
        assert!(
            error
                .to_string()
                .contains("target binding order cannot be preserved")
        );
        assert!(!destination.parent().unwrap().exists());
    }
    project
        .root
        .bindings
        .retain(|binding| binding.target_field != "Amount" || binding.node == cast);
    assert_eq!(
        engine::run(&project, &input(&project, "1", "0", "not-a-number")?),
        Err(EngineError::Function(FunctionError::DivideByZero))
    );
    for native in [false, true] {
        let destination = package.0.join(if native {
            "single-native/mapping.mfd"
        } else {
            "single-extension/mapping.mfd"
        });
        let error = if native {
            mfd::export_with_profile(&project, &destination, mfd::ExportProfile::NativeMfd)
                .unwrap_err()
        } else {
            mfd::export(&project, &destination).unwrap_err()
        };
        assert!(
            error
                .to_string()
                .contains("target binding order cannot be preserved")
        );
        assert!(!destination.parent().unwrap().exists());
    }
    let condition = project.graph.nodes.keys().next_back().copied().unwrap() + 1;
    let conditional = condition + 1;
    project.graph.nodes.insert(
        condition,
        Node::Const {
            value: ir::Value::Bool(true),
        },
    );
    project.graph.nodes.insert(
        conditional,
        Node::If {
            condition,
            then: cast,
            else_: cast,
        },
    );
    project
        .root
        .bindings
        .iter_mut()
        .find(|binding| binding.node == cast)
        .unwrap()
        .node = conditional;
    let destination = package.0.join("nested-helper/mapping.mfd");
    let error = mfd::export(&project, &destination).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("target binding order cannot be preserved")
    );
    assert!(!destination.parent().unwrap().exists());
    Ok(())
}

#[test]
fn native_export_never_drops_float_helpers_for_numeric_siblings() -> Result<(), Box<dyn Error>> {
    for value in [17_i64, 1_i64 << 60, 9_007_199_254_740_993_i64] {
        let package = Package::new()?;
        let mut project = package.import()?;
        let (cast, _) = first_cast(&project);
        let id = project.graph.nodes.keys().next_back().copied().unwrap() + 1;
        project.graph.nodes.insert(
            id,
            Node::Const {
                value: ir::Value::Int(value),
            },
        );
        project
            .root
            .bindings
            .iter_mut()
            .find(|binding| binding.target_field == "Amount" && binding.node != cast)
            .unwrap()
            .node = id;
        let destination = package.0.join("native/mapping.mfd");
        assert_native_refusal(&project, &destination, "coerce_float")?;
    }
    let package = Package::new()?;
    let mut project = package.import()?;
    let ir::SchemaKind::Group { children, .. } = &mut project.source.kind else {
        panic!("source")
    };
    children
        .iter_mut()
        .find(|field| field.name == "Numerator")
        .unwrap()
        .kind = ir::SchemaKind::Scalar {
        ty: ir::ScalarType::Int,
    };
    let source = project
        .graph
        .nodes
        .iter()
        .find_map(|(&id, node)| {
            matches!(node, Node::SourceField { path, .. } if path.as_slice() == ["Numerator"])
                .then_some(id)
        })
        .unwrap();
    let (cast, _) = first_cast(&project);
    project
        .root
        .bindings
        .iter_mut()
        .find(|binding| binding.target_field == "Amount" && binding.node != cast)
        .unwrap()
        .node = source;
    assert_native_refusal(
        &project,
        &package.0.join("native/mapping.mfd"),
        "coerce_float",
    )?;
    Ok(())
}

#[test]
fn raw_namespace_tables_must_match_unqualified_schema_names() -> Result<(), Box<dyn Error>> {
    for component in ["Source", "Target"] {
        for root_override in [false, true] {
            let package = Package::new()?;
            let file = package.0.join("mapping.mfd");
            let mut text = std::fs::read_to_string(&file)?;
            let start = text
                .find(&format!("<component name=\"{component}\""))
                .unwrap();
            let suffix = &text[start..];
            let (needle, replacement) = if root_override {
                (
                    "<entry name=\"Envelope\">",
                    "<entry name=\"Envelope\" ns=\"1\">",
                )
            } else {
                ("<namespace/>", "<namespace uid=\"urn:not-the-schema\"/>")
            };
            let at = start + suffix.find(needle).unwrap();
            text.replace_range(at..at + needle.len(), replacement);
            std::fs::write(file, text)?;
            let imported = mfd::import(&package.0.join("mapping.mfd"))?;
            assert_eq!(helpers(&imported.project), 0, "{component} {root_override}");
        }
    }
    Ok(())
}

#[test]
fn datetime_and_explicit_float_helpers_preserve_extension_roundtrip() -> Result<(), Box<dyn Error>>
{
    let package = Package::new()?;
    let project = with_datetime(package.import()?);
    let destination = package.0.join("mixed/mapping.mfd");
    assert_native_refusal(
        &project,
        &package.0.join("native/mapping.mfd"),
        "coerce_float",
    )?;
    mfd::export(&project, &destination)?;
    let text = std::fs::read_to_string(&destination)?;
    let document = roxmltree::Document::parse(&text)?;
    assert_eq!(
        document
            .descendants()
            .filter(|node| node.attribute("casttotargettypemode") == Some("cast-in-subtree"))
            .count(),
        1
    );
    let roundtrip = mfd::import(&destination)?;
    assert!(roundtrip.warnings.is_empty());
    assert_eq!(helpers(&roundtrip.project), 1);
    let source = format_xml::from_str(
        "<Envelope><Numerator>34</Numerator><Denominator>2</Denominator><Text>29</Text><Label>safe</Label><Date>2026-10-28</Date></Envelope>",
        &project.source,
    )?;
    assert_eq!(
        engine::run(&project, &source),
        engine::run(&roundtrip.project, &source)
    );
    Ok(())
}

#[test]
fn datetime_marker_keeps_unwrapped_generic_float_as_decimal() -> Result<(), Box<dyn Error>> {
    let package = Package::new()?;
    let mut project = with_datetime(package.import()?);
    let (call, argument) = first_cast(&project);
    project
        .root
        .bindings
        .iter_mut()
        .find(|binding| binding.node == call)
        .unwrap()
        .node = argument;
    project.prune_unreachable_nodes();
    let destination = package.0.join("native/mapping.mfd");
    assert!(
        mfd::export_with_profile(&project, &destination, mfd::ExportProfile::NativeMfd)?
            .is_native_compatible()
    );
    let target_xsd =
        std::fs::read_to_string(destination.parent().unwrap().join("mapping-target.xsd"))?;
    let xsd = roxmltree::Document::parse(&target_xsd)?;
    assert_eq!(
        xsd.descendants()
            .find(|node| node.attribute("name") == Some("Amount"))
            .and_then(|node| node.attribute("type")),
        Some("xs:decimal")
    );
    let roundtrip = mfd::import(&destination)?;
    assert!(roundtrip.warnings.is_empty());
    assert_eq!(helpers(&roundtrip.project), 0);
    Ok(())
}

#[test]
fn numeric_refusal_keeps_independent_datetime_lowering() -> Result<(), Box<dyn Error>> {
    let package = Package::new()?;
    let mut project = with_datetime(package.import()?);
    let (call, argument) = first_cast(&project);
    project
        .root
        .bindings
        .iter_mut()
        .find(|binding| binding.target_field == "Amount" && binding.node != call)
        .unwrap()
        .node = argument;
    let destination = package.0.join("extension/mapping.mfd");
    let report = mfd::preflight_export(&project, &destination)?;
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.component == "coerce_float")
    );
    assert!(
        !report
            .issues
            .iter()
            .any(|issue| issue.component == "coerce_datetime")
    );
    mfd::export(&project, &destination)?;
    let text = std::fs::read_to_string(&destination)?;
    let document = roxmltree::Document::parse(&text)?;
    assert_eq!(
        document
            .descendants()
            .filter(|node| node.attribute("casttotargettypemode") == Some("cast-in-subtree"))
            .count(),
        1
    );
    let roundtrip = mfd::import(&destination)?;
    assert!(roundtrip.warnings.is_empty());
    assert_eq!(helpers(&roundtrip.project), 1);
    let source = format_xml::from_str(
        "<Envelope><Numerator>34</Numerator><Denominator>2</Denominator><Text>29</Text><Label>safe</Label><Date>2026-10-28</Date></Envelope>",
        &project.source,
    )?;
    assert_eq!(
        engine::run(&project, &source),
        engine::run(&roundtrip.project, &source)
    );
    Ok(())
}
