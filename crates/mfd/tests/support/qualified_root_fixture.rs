use ir::{GroupAlternative, ScalarType, SchemaNode, Value, XmlNamespace};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, Copy, Debug)]
pub enum Shape {
    Unqualified,
    Qualified,
    Mixed,
    Fanout,
}
impl Shape {
    pub fn namespace(self) -> Option<&'static str> {
        match self {
            Self::Qualified | Self::Mixed => Some("urn:root-view-test"),
            Self::Unqualified | Self::Fanout => None,
        }
    }
    pub fn selected(self) -> String {
        self.namespace()
            .map_or_else(|| "Extended".into(), |ns| format!("{{{ns}}}Extended"))
    }
    pub fn xml(self, type_name: Option<&str>, extra: bool) -> String {
        let namespaces = self.namespace().map_or_else(String::new, |ns| {
            format!(" xmlns=\"{ns}\" xmlns:t=\"{ns}\"")
        });
        let lexical_type = type_name.map_or_else(String::new, |local| {
            let prefix = if self.namespace().is_some() { "t:" } else { "" };
            format!(" xsi:type=\"{prefix}{local}\"")
        });
        let code_prefix = if self.namespace().is_some() { "t:" } else { "" };
        let extra_prefix = if matches!(self, Self::Qualified) {
            "t:"
        } else {
            ""
        };
        let extra = if extra {
            format!(" {extra_prefix}Extra=\"extra-value\"")
        } else {
            String::new()
        };
        format!(
            "<Envelope{namespaces} xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"{lexical_type} {code_prefix}Code=\"code-value\"{extra}/>"
        )
    }
}

pub fn schema(shape: Shape, required: bool) -> SchemaNode {
    let ns = shape.namespace();
    let attribute = |name: &str, qualified: bool| {
        let mut child = SchemaNode::scalar(name, ScalarType::String).attribute();
        child.xml_namespace = Some(if qualified {
            XmlNamespace::qualified(ns.unwrap()).unwrap()
        } else {
            XmlNamespace::Unqualified
        });
        child.xml_attribute_required = required;
        child
    };
    let identity =
        |local: &str| ns.map_or_else(|| local.to_string(), |ns| format!("{{{ns}}}{local}"));
    let mut root = SchemaNode::group(
        "Envelope",
        vec![
            attribute("Code", ns.is_some()),
            attribute("Extra", matches!(shape, Shape::Qualified)),
        ],
    )
    .with_alternatives(vec![
        GroupAlternative {
            name: identity("Basic"),
            members: vec!["Code".into()],
            required: vec![],
            constraints: vec![],
        },
        GroupAlternative {
            name: identity("Extended"),
            members: vec!["Code".into(), "Extra".into()],
            required: vec![],
            constraints: vec![],
        },
    ])
    .unwrap();
    root.xml_namespace = Some(ns.map_or(XmlNamespace::Unqualified, |ns| {
        XmlNamespace::qualified(ns).unwrap()
    }));
    root.xml_default_type = Some(identity("Basic"));
    root.xml_type_alternatives = true;
    root
}

pub fn project(shape: Shape, source_required: bool, target_required: bool) -> Project {
    let mut nodes = BTreeMap::from([
        (
            0,
            Node::SourceRootXmlTypeEquals {
                canonical_expanded_type: shape.selected(),
            },
        ),
        (1, Node::Const { value: Value::Null }),
        (
            2,
            Node::Const {
                value: Value::String(shape.selected()),
            },
        ),
        (
            3,
            Node::SourceRootField {
                path: vec!["Code".into()],
                required: source_required,
            },
        ),
        (
            4,
            Node::If {
                condition: 0,
                then: 3,
                else_: 1,
            },
        ),
    ]);
    let extra = if matches!(shape, Shape::Fanout) {
        3
    } else {
        nodes.insert(
            5,
            Node::SourceRootField {
                path: vec!["Extra".into()],
                required: source_required,
            },
        );
        5
    };
    nodes.insert(
        6,
        Node::If {
            condition: 0,
            then: extra,
            else_: 1,
        },
    );
    Project {
        source: schema(shape, source_required),
        target: schema(shape, target_required),
        source_path: Some("input.xml".into()),
        target_path: Some("output.xml".into()),
        source_options: FormatOptions {
            xml_document: true,
            xml_allow_inactive_root_type_members: true,
            xml_root_view_read_policy: true,
            ..Default::default()
        },
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: BTreeMap::new(),
        graph: Graph { nodes },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "Code".into(),
                    node: 4,
                },
                Binding {
                    target_field: "Extra".into(),
                    node: 6,
                },
                Binding {
                    target_field: ir::XML_TYPE_FIELD.into(),
                    node: 2,
                },
            ],
            ..Default::default()
        },
    }
}

#[allow(dead_code)]
pub fn read(project: &Project, xml: &str) -> ir::Instance {
    format_xml::from_str_with_options(
        xml,
        &project.source,
        &format_xml::XmlReadOptions {
            allow_inactive_root_type_members: true,
            root_view_policy: true,
        },
    )
    .unwrap()
}

pub struct Directory(pub PathBuf);
impl Directory {
    pub fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-qualified-root-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
