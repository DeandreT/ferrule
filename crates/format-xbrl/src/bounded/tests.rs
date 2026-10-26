use super::*;
use crate::{XBRLDI, XBRLI};
use ir::{ScalarType, Value, XML_TEXT_FIELD};
use mapping::{XbrlBoundaryOptions, XbrlNamespaceBinding};

#[path = "review_tests.rs"]
mod review;

fn fixture() -> String {
    format!(
        r#"<xbrli:xbrl xmlns:xbrli="{XBRLI}" xmlns:xbrldi="{XBRLDI}" xmlns:a="urn:a" xmlns:b="urn:b"><xbrli:context id="c"><xbrli:entity><xbrli:identifier scheme="urn:id">Entity</xbrli:identifier></xbrli:entity><xbrli:period><xbrli:instant>2026-06-30</xbrli:instant></xbrli:period></xbrli:context><xbrli:unit id="u"><xbrli:measure>xbrli:pure</xbrli:measure></xbrli:unit><a:Amount contextRef="c" unitRef="u">0</a:Amount><b:Amount contextRef="c" unitRef="u">2</b:Amount></xbrli:xbrl>"#
    )
}

fn boundary() -> (SchemaNode, XbrlBoundaryOptions) {
    let schema = SchemaNode::group(
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
                    SchemaNode::group("north", vec![SchemaNode::scalar("Amount", ScalarType::Int)]),
                    SchemaNode::group("south", vec![SchemaNode::scalar("Amount", ScalarType::Int)]),
                    SchemaNode::scalar("Absent", ScalarType::Bool),
                ],
            )
            .repeating(),
        ],
    );
    let options = XbrlBoundaryOptions::external_target("taxonomy.xsd", None)
        .unwrap()
        .with_namespace_bindings(vec![
            XbrlNamespaceBinding::new(vec!["rows".into(), "identifier".into()], XBRLI).unwrap(),
            XbrlNamespaceBinding::new(vec!["rows".into(), "period".into()], XBRLI).unwrap(),
            XbrlNamespaceBinding::new(
                vec!["rows".into(), "period".into(), "instant".into()],
                XBRLI,
            )
            .unwrap(),
            XbrlNamespaceBinding::new(
                vec!["rows".into(), "north".into(), "Amount".into()],
                "urn:a",
            )
            .unwrap(),
            XbrlNamespaceBinding::new(
                vec!["rows".into(), "south".into(), "Amount".into()],
                "urn:b",
            )
            .unwrap(),
            XbrlNamespaceBinding::new(vec!["rows".into(), "Absent".into()], "urn:a").unwrap(),
        ])
        .unwrap();
    (schema, options)
}

fn read(xml: &str, limits: XbrlLimitOptions) -> Result<Instance, XbrlFormatError> {
    let (schema, options) = boundary();
    from_bytes_bounded(
        xml.as_bytes(),
        &schema,
        &options,
        &XbrlLimits::new(limits).unwrap(),
    )
}

fn limit_error(result: Result<Instance, XbrlFormatError>, limit: XbrlLimit) {
    assert!(
        matches!(result, Err(XbrlFormatError::LimitExceeded { limit: actual, .. }) if actual == limit),
        "expected {limit:?}, got {result:?}"
    );
}

#[test]
fn defaults_are_valid() {
    assert!(XbrlLimits::new(XbrlLimitOptions::default()).is_ok());
}

#[test]
fn exact_and_over_input_budgets() {
    let xml = fixture();
    macro_rules! boundary {
        ($field:ident, $limit:ident, $exact:expr) => {{
            let exact = $exact;
            let mut limits = XbrlLimitOptions::default();
            limits.$field = exact;
            assert!(read(&xml, limits.clone()).is_ok(), "{}", stringify!($field));
            limits.$field = exact - 1;
            limit_error(read(&xml, limits), XbrlLimit::$limit);
        }};
    }
    boundary!(max_input_bytes, InputBytes, xml.len());
    boundary!(max_xml_depth, XmlDepth, 4);
    boundary!(max_xml_nodes, XmlNodes, 16);
    boundary!(max_xml_attributes, XmlAttributes, 11);
    boundary!(
        max_xml_text_bytes,
        XmlTextBytes,
        XBRLI.len()
            + XBRLDI.len()
            + [
                "urn:a",
                "urn:b",
                "c",
                "urn:id",
                "u",
                "c",
                "u",
                "c",
                "u",
                "Entity",
                "2026-06-30",
                "xbrli:pure",
                "0",
                "2"
            ]
            .iter()
            .map(|s| s.len())
            .sum::<usize>()
    );
    boundary!(max_facts, Facts, 2);
    boundary!(max_contexts, Contexts, 1);
    boundary!(max_units, Units, 1);
}

#[test]
fn parity_namespace_collisions_zero_missing_and_context_order() {
    let (schema, options) = boundary();
    let xml = fixture();
    let native = crate::from_str_with_options(&xml, &schema, &options).unwrap();
    let bounded = read(&xml, XbrlLimitOptions::default()).unwrap();
    assert_eq!(native, bounded);
    let row = &bounded.field("rows").unwrap().as_repeated().unwrap()[0];
    assert_eq!(
        row.field("north")
            .unwrap()
            .field("Amount")
            .unwrap()
            .as_scalar(),
        Some(&Value::Int(0))
    );
    assert_eq!(
        row.field("south")
            .unwrap()
            .field("Amount")
            .unwrap()
            .as_scalar(),
        Some(&Value::Int(2))
    );
    assert_eq!(row.field("Absent").unwrap().as_scalar(), Some(&Value::Null));
    let xml = fixture().replace("</xbrli:context>", "</xbrli:context><xbrli:context id=\"first\"><xbrli:entity><xbrli:identifier scheme=\"urn:id\">Other</xbrli:identifier></xbrli:entity><xbrli:period><xbrli:forever/></xbrli:period></xbrli:context>").replace("</xbrli:xbrl>", "<a:Amount contextRef=\"first\">7</a:Amount></xbrli:xbrl>");
    let result = read(&xml, XbrlLimitOptions::default()).unwrap();
    let rows = result.field("rows").unwrap().as_repeated().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0]
            .field("north")
            .unwrap()
            .field("Amount")
            .unwrap()
            .as_scalar(),
        Some(&Value::Int(0))
    );
    assert_eq!(
        rows[1]
            .field("north")
            .unwrap()
            .field("Amount")
            .unwrap()
            .as_scalar(),
        Some(&Value::Int(7))
    );
}

#[test]
fn counts_unselected_structure_text_comments_and_processing_instructions() {
    let xml = fixture().replace(
        "</xbrli:xbrl>",
        "<a:Unknown contextRef=\"c\"><a:x><a:y/></a:x></a:Unknown></xbrli:xbrl>",
    );
    let limits = XbrlLimitOptions {
        max_xml_depth: 3,
        ..Default::default()
    };
    // Raise above the context depth to reach the hostile unselected subtree.
    let xml = xml.replace("<a:y/>", "<a:y><a:z/></a:y>");
    limit_error(
        read(
            &xml,
            XbrlLimitOptions {
                max_xml_depth: 4,
                ..limits
            },
        ),
        XbrlLimit::XmlDepth,
    );
    let xml = fixture().replace(
        "</xbrli:xbrl>",
        "<!--abcdefghijklmnopqrstuvwxyz--><?test abcdefghijklmnopqrstuvwxyz?></xbrli:xbrl>",
    );
    limit_error(
        read(
            &xml,
            XbrlLimitOptions {
                max_xml_nodes: 16,
                ..Default::default()
            },
        ),
        XbrlLimit::XmlNodes,
    );
    limit_error(
        read(
            &xml,
            XbrlLimitOptions {
                max_xml_text_bytes: 150,
                ..Default::default()
            },
        ),
        XbrlLimit::XmlTextBytes,
    );
    let xml = fixture().replace("</xbrli:xbrl>", "<a:Unknown contextRef=\"c\">xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx</a:Unknown></xbrli:xbrl>");
    limit_error(
        read(
            &xml,
            XbrlLimitOptions {
                max_facts: 2,
                ..Default::default()
            },
        ),
        XbrlLimit::Facts,
    );
    limit_error(
        read(
            &xml,
            XbrlLimitOptions {
                max_xml_text_bytes: 150,
                ..Default::default()
            },
        ),
        XbrlLimit::XmlTextBytes,
    );
}

#[test]
fn refuses_entities_and_audits_cdata_namespace_and_malformed_differentials() {
    for declaration in [
        r#"<!DOCTYPE xbrli:xbrl SYSTEM "file:///etc/passwd">"#,
        r#"<!DOCTYPE xbrli:xbrl [<!ENTITY x "boom">]>"#,
        r#"<!DOCTYPE xbrli:xbrl [<!ENTITY x SYSTEM "https://invalid.example/">]>"#,
    ] {
        assert!(matches!(
            read(
                &format!("{declaration}{}", fixture()),
                XbrlLimitOptions::default()
            ),
            Err(XbrlFormatError::UnsupportedBoundedProfile(_))
        ));
    }
    let xml = fixture().replace("Entity", "a&amp;<![CDATA[b<c]]>&#65;&#x42;");
    let (schema, options) = boundary();
    assert_eq!(
        read(&xml, XbrlLimitOptions::default()).unwrap(),
        crate::from_str_with_options(&xml, &schema, &options).unwrap()
    );
    for xml in [
        fixture().replace("Entity", "&unknown;"),
        fixture().replace("urn:a", "urn&#58;a"),
        fixture().replace("id=\"c\"", "id=\"c\" id=\"d\""),
        fixture().replace("a:Amount", "undeclared:Amount"),
        fixture().replace("</xbrli:xbrl>", ""),
        fixture().replace("Entity", "&#0;"),
    ] {
        assert!(read(&xml, XbrlLimitOptions::default()).is_err(), "{xml}");
    }
    let xml = fixture()
        .replace(
            "xmlns:a=\"urn:a\"",
            "xmlns:a=\"urn:a\" xmlns:alias=\"urn:a\"",
        )
        .replace(
            "</xbrli:xbrl>",
            "<alias:Amount contextRef=\"c\">3</alias:Amount></xbrli:xbrl>",
        );
    assert!(matches!(
        read(&xml, XbrlLimitOptions::default()),
        Err(XbrlFormatError::DuplicateFact { .. })
    ));
    let xml = fixture()
        .replace("<a:Amount ", "<Amount xmlns=\"urn:a\" ")
        .replace("</a:Amount>", "</Amount>");
    assert_eq!(
        read(&xml, XbrlLimitOptions::default()).unwrap(),
        read(&fixture(), XbrlLimitOptions::default()).unwrap()
    );
}

#[test]
fn validates_every_reference_context_unit_and_explicit_dimension() {
    let xml = fixture();
    for malformed in [
        xml.replace("id=\"c\"", "id=\"\""),
        xml.replace("<xbrli:instant>", "<xbrli:other>")
            .replace("</xbrli:instant>", "</xbrli:other>"),
        xml.replace(
            "<xbrli:measure>xbrli:pure</xbrli:measure>",
            "<xbrli:measure>xbrli:pure</xbrli:measure><xbrli:measure>xbrli:shares</xbrli:measure>",
        ),
        xml.replace("xbrli:pure", "unbound:pure"),
    ] {
        assert!(
            read(&malformed, XbrlLimitOptions::default()).is_err(),
            "{malformed}"
        );
    }
    let unknown = xml.replace(
        "</xbrli:xbrl>",
        "<a:Unknown contextRef=\"missing\"/></xbrli:xbrl>",
    );
    assert!(matches!(
        read(&unknown, XbrlLimitOptions::default()),
        Err(XbrlFormatError::MissingContextReference { .. })
    ));
    let unknown = xml.replace(
        "</xbrli:xbrl>",
        "<a:Unknown contextRef=\"c\" unitRef=\"missing\"/></xbrli:xbrl>",
    );
    assert!(matches!(
        read(&unknown, XbrlLimitOptions::default()),
        Err(XbrlFormatError::MissingUnitReference { .. })
    ));
    let duplicate = xml.replace(
        "</xbrli:unit>",
        "</xbrli:unit><xbrli:unit id=\"u\"><xbrli:measure>xbrli:pure</xbrli:measure></xbrli:unit>",
    );
    assert!(matches!(
        read(&duplicate, XbrlLimitOptions::default()),
        Err(XbrlFormatError::DuplicateUnit { .. })
    ));
    let dimension = r#"<xbrli:segment><xbrldi:explicitMember dimension="a:Axis">b:Member</xbrldi:explicitMember></xbrli:segment>"#;
    let xml = xml.replace("</xbrli:entity>", &format!("{dimension}</xbrli:entity>"));
    assert!(
        read(
            &xml,
            XbrlLimitOptions {
                max_dimensions: 1,
                ..Default::default()
            }
        )
        .is_ok()
    );
    limit_error(
        read(
            &xml,
            XbrlLimitOptions {
                max_dimensions: 0,
                ..Default::default()
            },
        ),
        XbrlLimit::Dimensions,
    );
    let duplicate = xml.replace("</xbrli:segment>", "<xbrldi:explicitMember dimension=\"a:Axis\">a:Other</xbrldi:explicitMember></xbrli:segment>");
    assert!(matches!(
        read(&duplicate, XbrlLimitOptions::default()),
        Err(XbrlFormatError::InvalidBoundedInstance(
            "duplicate explicit dimension"
        ))
    ));
    for malformed in [
        xml.replace("explicitMember", "typedMember"),
        xml.replace("a:Axis", "bad:Axis"),
        xml.replace("<xbrli:segment>", "<xbrli:scenario>")
            .replace("</xbrli:segment>", "</xbrli:scenario>"),
    ] {
        assert!(read(&malformed, XbrlLimitOptions::default()).is_err());
    }
    let divide = fixture().replace("<xbrli:measure>xbrli:pure</xbrli:measure>", "<xbrli:divide><xbrli:unitNumerator><xbrli:measure>xbrli:pure</xbrli:measure></xbrli:unitNumerator><xbrli:unitDenominator><xbrli:measure>xbrli:shares</xbrli:measure></xbrli:unitDenominator></xbrli:divide>");
    assert!(read(&divide, XbrlLimitOptions::default()).is_ok());
}

#[test]
fn rejects_unsupported_unselected_shapes_and_can_retry_corrected_input() {
    for extra in [
        r#"<a:Unused contextRef="c"><a:Nested/></a:Unused>"#,
        r#"<link:footnoteLink xmlns:link="http://www.xbrl.org/2003/linkbase"/>"#,
        r#"<a:Unused contextRef="c" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:nil="true"/>"#,
    ] {
        let xml = fixture().replace("</xbrli:xbrl>", &format!("{extra}</xbrli:xbrl>"));
        assert!(matches!(
            read(&xml, XbrlLimitOptions::default()),
            Err(XbrlFormatError::UnsupportedBoundedProfile(_))
        ));
    }
    for _ in 0..2 {
        assert!(matches!(
            read(
                &fixture().replace(">0<", ">invalid<"),
                XbrlLimitOptions::default()
            ),
            Err(XbrlFormatError::ScalarParse { .. })
        ));
        assert!(read(&fixture(), XbrlLimitOptions::default()).is_ok());
    }
    assert!(matches!(
        from_bytes_bounded(
            &[0xff],
            &boundary().0,
            &boundary().1,
            &XbrlLimits::default()
        ),
        Err(XbrlFormatError::XmlPreflight(_))
    ));
}

fn rendered_fixture() -> (SchemaNode, XbrlBoundaryOptions, Instance) {
    let (schema, options) = boundary();
    let instance = read(&fixture(), XbrlLimitOptions::default()).unwrap();
    (schema, options, instance)
}

#[test]
fn writer_exact_over_bytes_expansion_and_retry_preserve_native_output() {
    let (schema, options, instance) = rendered_fixture();
    let native = crate::to_string(&schema, &instance, &options).unwrap();
    let limits = XbrlLimits::new(XbrlLimitOptions {
        max_output_bytes: native.len(),
        ..Default::default()
    })
    .unwrap();
    let bytes = to_bytes_bounded(&schema, &instance, &options, &limits).unwrap();
    assert_eq!(bytes, native.as_bytes());
    let limits = XbrlLimits::new(XbrlLimitOptions {
        max_output_bytes: native.len() - 1,
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        to_bytes_bounded(&schema, &instance, &options, &limits),
        Err(XbrlFormatError::LimitExceeded {
            limit: XbrlLimit::OutputBytes,
            ..
        })
    ));
    assert_eq!(
        to_bytes_bounded(&schema, &instance, &options, &XbrlLimits::default()).unwrap(),
        native.as_bytes()
    );
    let (schema, options) = boundary();
    let expanded = fixture().replace("Entity", &"&amp;&lt;&gt;&apos;&quot;".repeat(100));
    let instance = read(&expanded, XbrlLimitOptions::default()).unwrap();
    let native = crate::to_string(&schema, &instance, &options).unwrap();
    let limits = XbrlLimits::new(XbrlLimitOptions {
        max_output_bytes: native.len() - 1,
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        to_bytes_bounded(&schema, &instance, &options, &limits),
        Err(XbrlFormatError::LimitExceeded {
            limit: XbrlLimit::OutputBytes,
            ..
        })
    ));
    assert_eq!(
        to_bytes_bounded(&schema, &instance, &options, &XbrlLimits::default()).unwrap(),
        native.as_bytes()
    );
    assert!(native.contains("&amp;&lt;&gt;&apos;&quot;"));
}

#[test]
fn writer_limits_candidates_facts_and_unselected_input_before_collection() {
    let (schema, options, instance) = rendered_fixture();
    for (limits, expected) in [
        (
            XbrlLimitOptions {
                max_contexts: 0,
                ..Default::default()
            },
            XbrlLimit::Contexts,
        ),
        (
            XbrlLimitOptions {
                max_facts: 1,
                ..Default::default()
            },
            XbrlLimit::Facts,
        ),
        (
            XbrlLimitOptions {
                max_model_depth: 2,
                ..Default::default()
            },
            XbrlLimit::ModelDepth,
        ),
        (
            XbrlLimitOptions {
                max_model_nodes: 1,
                ..Default::default()
            },
            XbrlLimit::ModelNodes,
        ),
        (
            XbrlLimitOptions {
                max_model_text_bytes: 1,
                ..Default::default()
            },
            XbrlLimit::ModelTextBytes,
        ),
        (
            XbrlLimitOptions {
                max_string_bytes: 1,
                ..Default::default()
            },
            XbrlLimit::StringBytes,
        ),
    ] {
        assert!(
            matches!(to_bytes_bounded(&schema, &instance, &options, &XbrlLimits::new(limits).unwrap()), Err(XbrlFormatError::LimitExceeded { limit, .. }) if limit == expected)
        );
    }
    let Instance::Group(fields) = &instance else {
        panic!()
    };
    let mut fields = fields.to_vec();
    fields.push((
        "unselected".into(),
        Instance::Scalar(Value::String("x".repeat(200))),
    ));
    let hostile = Instance::Group(fields.into());
    let limits = XbrlLimits::new(XbrlLimitOptions {
        max_string_bytes: 100,
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        to_bytes_bounded(&schema, &hostile, &options, &limits),
        Err(XbrlFormatError::LimitExceeded {
            limit: XbrlLimit::StringBytes,
            ..
        })
    ));
}

fn rich_target() -> (SchemaNode, XbrlBoundaryOptions, Instance) {
    let (mut schema, options, instance) = rendered_fixture();
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        panic!()
    };
    let SchemaKind::Group { children: row, .. } = &mut children[0].kind else {
        panic!()
    };
    row.push(SchemaNode::group(
        "Axis",
        vec![SchemaNode::scalar("explicitMember", ScalarType::String)],
    ));
    children.push(SchemaNode::group(
        "unit",
        vec![
            SchemaNode::scalar("id", ScalarType::String).attribute(),
            SchemaNode::scalar("measure", ScalarType::String),
        ],
    ));
    let Instance::Group(fields) = instance else {
        panic!()
    };
    let mut fields = fields.to_vec();
    let Instance::Repeated(rows) = &mut fields[0].1 else {
        panic!()
    };
    let Instance::Group(row) = &rows[0] else {
        panic!()
    };
    let mut row = row.to_vec();
    row.push((
        "Axis".into(),
        Instance::Group(
            vec![(
                "explicitMember".into(),
                Instance::Scalar(Value::String("{urn:b}Member".into())),
            )]
            .into(),
        ),
    ));
    rows[0] = Instance::Group(row.into());
    fields.push((
        "unit".into(),
        Instance::Group(
            vec![
                ("id".into(), Instance::Scalar(Value::String("u".into()))),
                (
                    "measure".into(),
                    Instance::Scalar(Value::String(format!("{{{XBRLI}}}pure"))),
                ),
            ]
            .into(),
        ),
    ));
    let mut namespaces = options.namespace_bindings().to_vec();
    namespaces
        .push(XbrlNamespaceBinding::new(vec!["rows".into(), "Axis".into()], "urn:a").unwrap());
    namespaces.push(
        XbrlNamespaceBinding::new(
            vec!["rows".into(), "Axis".into(), "explicitMember".into()],
            XBRLDI,
        )
        .unwrap(),
    );
    namespaces.push(XbrlNamespaceBinding::new(vec!["unit".into()], XBRLI).unwrap());
    (
        schema,
        options.with_namespace_bindings(namespaces).unwrap(),
        Instance::Group(fields.into()),
    )
}

#[test]
fn writer_exact_xml_counts_match_byte_preflight_including_indentation_and_references() {
    let (schema, options, instance) = rich_target();
    let native = crate::to_string(&schema, &instance, &options).unwrap();
    macro_rules! boundary {
        ($field:ident, $limit:ident) => {{
            let mut limits = XbrlLimitOptions::default();
            let exact = (0..10_000)
                .find(|count| {
                    limits.$field = *count;
                    xml::preflight(&native, &limits).is_ok()
                })
                .unwrap();
            limits.$field = exact;
            let valid = XbrlLimits::new(limits.clone()).unwrap();
            assert_eq!(
                to_bytes_bounded(&schema, &instance, &options, &valid).unwrap(),
                native.as_bytes(),
                "{}",
                stringify!($field)
            );
            limits.$field = exact - 1;
            let invalid = XbrlLimits::new(limits).unwrap();
            assert!(
                matches!(
                    to_bytes_bounded(&schema, &instance, &options, &invalid),
                    Err(XbrlFormatError::LimitExceeded {
                        limit: XbrlLimit::$limit,
                        ..
                    })
                ),
                "{}",
                stringify!($field)
            );
        }};
    }
    boundary!(max_xml_depth, XmlDepth);
    boundary!(max_xml_nodes, XmlNodes);
    boundary!(max_xml_attributes, XmlAttributes);
    boundary!(max_xml_text_bytes, XmlTextBytes);
    boundary!(max_facts, Facts);
    boundary!(max_contexts, Contexts);
    boundary!(max_units, Units);
    boundary!(max_dimensions, Dimensions);
    // The native reader does not project dimension ports. Refuse explicitly
    // instead of returning Null for a caller requesting those ports.
    assert!(matches!(
        from_bytes_bounded(native.as_bytes(), &schema, &options, &XbrlLimits::default()),
        Err(XbrlFormatError::UnsupportedBoundedProfile(_))
    ));
}

#[test]
fn materialization_and_model_limits_have_exact_edges() {
    let xml = fixture();
    macro_rules! boundary {
        ($field:ident, $limit:ident, $start:expr) => {{
            let mut limits = XbrlLimitOptions::default();
            let exact = ($start..10_000)
                .find(|count| {
                    limits.$field = *count;
                    read(&xml, limits.clone()).is_ok()
                })
                .unwrap();
            assert!(read(&xml, limits.clone()).is_ok());
            limits.$field = exact - 1;
            limit_error(read(&xml, limits), XbrlLimit::$limit);
        }};
    }
    boundary!(max_model_depth, ModelDepth, 1);
    boundary!(max_model_nodes, ModelNodes, 0);
    boundary!(max_model_text_bytes, ModelTextBytes, 0);
    boundary!(max_string_bytes, StringBytes, 0);
    assert!(matches!(
        XbrlLimits::new(XbrlLimitOptions {
            max_model_depth: 257,
            ..Default::default()
        }),
        Err(XbrlFormatError::InvalidLimit(XbrlLimit::ModelDepth))
    ));
    assert!(matches!(
        XbrlLimits::new(XbrlLimitOptions {
            max_output_bytes: usize::MAX,
            ..Default::default()
        }),
        Err(XbrlFormatError::InvalidLimit(XbrlLimit::OutputBytes))
    ));
}

fn replace_field(instance: &Instance, path: &[&str], replacement: &Instance) -> Instance {
    if path.is_empty() {
        return replacement.clone();
    }
    match instance {
        Instance::Group(fields) => Instance::Group(
            fields
                .iter()
                .map(|(name, value)| {
                    (
                        name.clone(),
                        if name == path[0] {
                            replace_field(value, &path[1..], replacement)
                        } else {
                            value.clone()
                        },
                    )
                })
                .collect::<Vec<_>>()
                .into(),
        ),
        Instance::Repeated(rows) => Instance::Repeated(
            rows.iter()
                .map(|row| replace_field(row, path, replacement))
                .collect(),
        ),
        _ => panic!("missing field path"),
    }
}

#[test]
fn bounded_writer_keeps_native_typed_failures_and_corrected_retries() {
    let (schema, options, instance) = rendered_fixture();
    let limits = XbrlLimits::default();
    let missing = replace_field(
        &instance,
        &["rows", "identifier", XML_TEXT_FIELD],
        &Instance::Scalar(Value::Null),
    );
    assert!(matches!(
        to_bytes_bounded(&schema, &missing, &options, &limits),
        Err(XbrlFormatError::MissingContext { .. })
    ));
    let decimal = replace_field(
        &instance,
        &["rows", "north", "Amount"],
        &Instance::Scalar(Value::decimal(ir::Decimal96::parse_exact("1.25").unwrap())),
    );
    assert!(
        matches!(to_bytes_bounded(&schema, &decimal, &options, &limits), Err(XbrlFormatError::TargetShape { path }) if path == "rows/north/Amount")
    );
    let numeric_options = options
        .clone()
        .with_fact_bindings(vec![
            mapping::XbrlFactBinding::new(
                vec!["rows".into(), "north".into(), "Amount".into()],
                mapping::XbrlFactType::Numeric,
            )
            .unwrap(),
        ])
        .unwrap();
    assert!(matches!(
        to_bytes_bounded(&schema, &instance, &numeric_options, &limits),
        Err(XbrlFormatError::MissingFactUnit { .. })
    ));
    assert_eq!(
        to_bytes_bounded(&schema, &instance, &options, &limits).unwrap(),
        crate::to_string(&schema, &instance, &options)
            .unwrap()
            .as_bytes()
    );

    let (schema, options, instance) = rich_target();
    let bindings = options
        .namespace_bindings()
        .iter()
        .filter(|binding| binding.path() != ["rows", "Axis"])
        .cloned()
        .collect();
    let missing_dimension = options.clone().with_namespace_bindings(bindings).unwrap();
    assert!(matches!(
        to_bytes_bounded(&schema, &instance, &missing_dimension, &limits),
        Err(XbrlFormatError::MissingFactNamespace { .. })
    ));
    let (mut rows, defaults, _) = crate::target_parts(&schema, &instance, &options, None).unwrap();
    rows[0].period.forever = true;
    assert!(matches!(
        profile::target(&rows, &defaults.units),
        Err(XbrlFormatError::InvalidBoundedInstance(
            "conflicting or incomplete target period"
        ))
    ));
}
