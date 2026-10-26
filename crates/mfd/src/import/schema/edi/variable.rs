use ir::SchemaNode;
use std::collections::BTreeMap;

/// Parser acknowledgement interfaces belong to physical endpoints. A variable
/// can still expose an ordinary configured field with the same name prefix.
pub(super) fn validate_parser_error_ports(
    schema: &SchemaNode,
    ports: &BTreeMap<u32, Vec<String>>,
) -> Result<(), &'static str> {
    if ports.values().any(|path| {
        path.iter().any(|name| name.starts_with("ParserErrors_"))
            && super::schema_node_at(schema, path).is_none()
    }) {
        return Err("has parser-error ports absent from its compiled variable schema");
    }
    Ok(())
}

/// Resolve the declared schema path without adding document-wrapper roles or
/// guessing a suffix. Configuration compilation remains authoritative.
pub(super) fn select(
    data: roxmltree::Node<'_, '_>,
    schema: &SchemaNode,
) -> Result<SchemaNode, &'static str> {
    let parameters = data
        .children()
        .filter(|node| {
            node.has_tag_name("parameter") && node.attribute("usageKind") == Some("variable")
        })
        .collect::<Vec<_>>();
    let [parameter] = parameters.as_slice() else {
        return Err("variable must have one declared parameter");
    };
    if data
        .children()
        .filter(|node| node.has_tag_name("root"))
        .any(|root| {
            root.children().any(|entry| {
                entry.has_tag_name("entry")
                    && matches!(
                        entry.attribute("name"),
                        Some("compute-when" | "parent-context")
                    )
            })
        })
    {
        return Err("variable scope controls require a separate contract");
    }
    let roots = parameter
        .children()
        .filter(|node| node.has_tag_name("root"))
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Err("variable must declare one selection root");
    };
    let entries = root
        .children()
        .filter(|node| node.has_tag_name("entry"))
        .collect::<Vec<_>>();
    if entries.is_empty() || entries.len() > 64 {
        return Err("selection must contain between one and 64 entries");
    }
    let mut selected = schema;
    for (index, entry) in entries.iter().enumerate() {
        if entry
            .attributes()
            .any(|attribute| attribute.name() != "name")
            || entry.children().any(|child| child.is_element())
        {
            return Err("selection must be one flat path of named entries");
        }
        let name = entry
            .attribute("name")
            .filter(|name| !name.is_empty())
            .ok_or("selection entry has no name")?;
        if index == 0 {
            if name != schema.name {
                return Err("selection root does not match its compiled configuration");
            }
        } else {
            selected = selected
                .child(name)
                .ok_or("selected child is absent from its compiled configuration")?;
        }
    }
    Ok(selected.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_component(
        text: &str,
        fixture: &std::path::Path,
    ) -> (Option<super::super::SchemaComponent>, Vec<String>) {
        let document = roxmltree::Document::parse(text).unwrap();
        let component = document
            .descendants()
            .find(|node| {
                node.has_tag_name("component") && node.attribute("library") == Some("text")
            })
            .unwrap();
        let mut warnings = Vec::new();
        let actual = super::super::read(&component, fixture, None, &mut warnings, true);
        (actual, warnings)
    }

    fn selected_segment() -> SchemaNode {
        SchemaNode::group(
            "N1",
            vec![
                SchemaNode::scalar("Text", ir::ScalarType::String),
                SchemaNode::scalar("Count", ir::ScalarType::Int),
            ],
        )
    }

    #[test]
    fn complete_variable_reader_refuses_unconfigured_parser_error_ports() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/edi-structure-variables/mapping.mfd");
        let original = std::fs::read_to_string(&fixture).unwrap();
        let (actual, warnings) = read_component(&original, &fixture);
        assert!(warnings.is_empty(), "{warnings:?}");
        let actual = actual.unwrap();
        assert!(actual.is_variable);
        assert_eq!(actual.schema, selected_segment());
        assert_eq!(actual.options, mapping::FormatOptions::default());
        for branch in [
            "<entry name=\"ParserErrors_N1\" outkey=\"98\"/>",
            "<entry name=\"ParserErrors_N1\"><entry name=\"Detail\" outkey=\"99\"/></entry>",
        ] {
            let changed = original.replace(
                "<entry name=\"Count\" inpkey=\"24\" outkey=\"25\"/>",
                &format!("<entry name=\"Count\" inpkey=\"24\" outkey=\"25\"/>{branch}"),
            );
            assert_ne!(changed, original);
            let (actual, warnings) = read_component(&changed, &fixture);
            assert!(actual.is_none());
            assert!(
                warnings.iter().any(|warning| warning
                    .contains("has parser-error ports absent from its compiled variable schema")),
                "{warnings:?}"
            );
        }
    }

    #[test]
    fn configured_parser_prefix_field_preserves_the_complete_variable_schema() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/edi-variable-parser-fields/mapping.mfd");
        let text = std::fs::read_to_string(&fixture).unwrap();
        let (actual, warnings) = read_component(&text, &fixture);
        assert!(warnings.is_empty(), "{warnings:?}");
        let actual = actual.unwrap();
        assert!(actual.is_variable);
        assert_eq!(
            actual.schema,
            SchemaNode::group(
                "N1",
                vec![
                    SchemaNode::scalar("Text", ir::ScalarType::String),
                    SchemaNode::scalar("Count", ir::ScalarType::Int),
                    SchemaNode::scalar("ParserErrors_Code", ir::ScalarType::String),
                ],
            )
        );
        let changed = text.replace(
            "<entry name=\"ParserErrors_Code\" inpkey=\"26\" outkey=\"27\"/>",
            "<entry name=\"ParserErrors_Code\" inpkey=\"26\" outkey=\"27\"><entry name=\"Detail\" outkey=\"99\"/></entry>",
        );
        assert_ne!(changed, text);
        let (actual, warnings) = read_component(&changed, &fixture);
        assert!(actual.is_none());
        assert!(warnings.iter().any(|warning| {
            warning.contains("has parser-error ports absent from its compiled variable schema")
        }));
    }

    #[test]
    fn physical_x12_endpoint_keeps_virtual_parser_error_schema_roles() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/edi-structure-variables/mapping.mfd");
        let text = r#"<component name="physical" library="text" kind="16"><data>
          <root><entry name="document"><entry name="Envelope" outkey="70">
            <entry name="ParserErrors_Envelope"><entry name="Detail" outkey="99"/></entry>
          </entry></entry></root>
          <text type="edi" kind="EDIX12" config="Envelope.Config"><messages><message type="850"/></messages>
            <settings interchangecontrolversionnumber="00501"/>
          </text>
        </data></component>"#;
        let (actual, warnings) = read_component(text, &fixture);
        assert!(warnings.is_empty(), "{warnings:?}");
        let actual = actual.unwrap();
        assert!(!actual.is_variable);
        let segment = |name| {
            SchemaNode::group(
                name,
                vec![SchemaNode::scalar("Text", ir::ScalarType::String)],
            )
        };
        let expected = SchemaNode::group(
            "Envelope",
            vec![
                SchemaNode::group(
                    "Interchange",
                    vec![
                        segment("ISA"),
                        SchemaNode::group(
                            "Group",
                            vec![
                                segment("GS"),
                                SchemaNode::group(
                                    "Message_850",
                                    vec![
                                        SchemaNode::group(
                                            "ST",
                                            vec![SchemaNode::scalar_fixed(
                                                "Text",
                                                ir::ScalarType::String,
                                                "850",
                                            )],
                                        ),
                                        SchemaNode::group("Loop0100", vec![selected_segment()])
                                            .repeating(),
                                        segment("SE"),
                                    ],
                                )
                                .repeating(),
                                segment("GE").repeating(),
                            ],
                        )
                        .repeating(),
                        segment("IEA").repeating(),
                    ],
                )
                .repeating(),
                SchemaNode::group(
                    "ParserErrors_Envelope",
                    vec![SchemaNode::scalar("Detail", ir::ScalarType::String)],
                )
                .repeating(),
            ],
        );
        assert_eq!(actual.schema, expected);
        assert_eq!(actual.ports.get(&70), Some(&Vec::<String>::new()));
        assert_eq!(
            actual.ports.get(&99),
            Some(&vec![
                String::from("ParserErrors_Envelope"),
                String::from("Detail"),
            ])
        );
        assert_eq!(actual.options.edi_kind, Some(mapping::EdiBoundaryKind::X12));
    }

    #[test]
    fn compiled_selected_segment_has_an_independent_complete_schema() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/edi-structure-variables");
        let compiled =
            format_edi_config::import_config(&directory.join("Envelope.Config"), &["850".into()])
                .unwrap();
        let text = std::fs::read_to_string(directory.join("mapping.mfd")).unwrap();
        let document = roxmltree::Document::parse(&text).unwrap();
        let data = document
            .descendants()
            .find(|node| {
                node.has_tag_name("component") && node.attribute("name") == Some("segment")
            })
            .unwrap()
            .children()
            .find(|node| node.has_tag_name("data"))
            .unwrap();
        let selected = select(data, &compiled.schema).unwrap();
        let expected = SchemaNode::group(
            "N1",
            vec![
                SchemaNode::scalar("Text", ir::ScalarType::String),
                SchemaNode::scalar("Count", ir::ScalarType::Int),
            ],
        );
        assert_eq!(selected, expected);

        let repeated = text.replace(
            "<entry name=\"N1\"/></root></parameter>",
            "</root></parameter>",
        );
        let document = roxmltree::Document::parse(&repeated).unwrap();
        let data = document
            .descendants()
            .find(|node| {
                node.has_tag_name("component") && node.attribute("name") == Some("segment")
            })
            .unwrap()
            .children()
            .find(|node| node.has_tag_name("data"))
            .unwrap();
        let expected = SchemaNode::group("Loop0100", vec![expected]).repeating();
        assert_eq!(select(data, &compiled.schema).unwrap(), expected);
    }
}
