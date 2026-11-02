use super::fixtures::project;
use ir::{Instance, ScalarType, SchemaNode, Value, XML_TEXT_FIELD};
use mapping::{XbrlBoundaryOptions, XbrlNamespaceBinding};
use web_demo::runtime::{DataFormat, serialize_target};

const XBRLI: &str = "http://www.xbrl.org/2003/instance";

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}

#[test]
fn supported_xbrl_target_keeps_complete_document_bytes() {
    let mut project = project();
    project.target = SchemaNode::group(
        "xbrl",
        vec![
            SchemaNode::group(
                "rows",
                vec![
                    SchemaNode::group(
                        "identifier",
                        vec![
                            SchemaNode::scalar("scheme", ScalarType::String).attribute(),
                            SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
                        ],
                    ),
                    SchemaNode::group(
                        "period",
                        vec![SchemaNode::scalar("instant", ScalarType::String)],
                    ),
                    SchemaNode::scalar("Label", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    project.target_options.xbrl = Some(
        XbrlBoundaryOptions::external_target("taxonomy.xsd", None)
            .unwrap()
            .with_namespace_bindings(vec![
                XbrlNamespaceBinding::new(vec!["rows".into(), "identifier".into()], XBRLI).unwrap(),
                XbrlNamespaceBinding::new(vec!["rows".into(), "period".into()], XBRLI).unwrap(),
                XbrlNamespaceBinding::new(vec!["rows".into(), "Label".into()], "urn:facts")
                    .unwrap(),
            ])
            .unwrap(),
    );
    let target = group(vec![(
        "rows",
        Instance::Repeated(vec![group(vec![
            (
                "identifier",
                group(vec![
                    ("scheme", text("urn:id")),
                    (XML_TEXT_FIELD, text("Entity")),
                ]),
            ),
            ("period", group(vec![("instant", text("2026-06-30"))])),
            ("Label", text("Ada")),
        ])]),
    )]);
    let expected = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<xbrli:xbrl xmlns:xbrli=\"http://www.xbrl.org/2003/instance\" xmlns:xbrldi=\"http://xbrl.org/2006/xbrldi\" xmlns:link=\"http://www.xbrl.org/2003/linkbase\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" xmlns:ns1=\"urn:facts\">\n",
        "  <link:schemaRef xlink:type=\"simple\" xlink:href=\"taxonomy.xsd\"/>\n",
        "  <xbrli:context id=\"c1\">\n",
        "    <xbrli:entity>\n",
        "      <xbrli:identifier scheme=\"urn:id\">Entity</xbrli:identifier>\n",
        "    </xbrli:entity>\n",
        "    <xbrli:period>\n",
        "      <xbrli:instant>2026-06-30</xbrli:instant>\n",
        "    </xbrli:period>\n",
        "  </xbrli:context>\n",
        "  <ns1:Label contextRef=\"c1\">Ada</ns1:Label>\n",
        "</xbrli:xbrl>",
    );
    assert_eq!(
        serialize_target(&project, &target, DataFormat::Xbrl)
            .unwrap()
            .as_bytes(),
        expected.as_bytes()
    );
}
