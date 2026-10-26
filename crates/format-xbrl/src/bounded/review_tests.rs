//! Small executable counterexamples for the independently reviewed boundaries.
use super::*;

fn is_unsupported<T>(result: &Result<T, XbrlFormatError>) -> bool {
    matches!(result, Err(XbrlFormatError::UnsupportedBoundedProfile(_)))
}

#[test]
fn review_r1_empty_comment_and_cdata_are_nodes_before_dom() {
    let mut caught = Vec::new();
    for (label, extra, exact) in [
        ("empty comment", "<!---->", 17),
        (
            "empty CDATA",
            "<a:Unused contextRef=\"c\"><![CDATA[]]></a:Unused>",
            18,
        ),
    ] {
        let xml = fixture().replace("</xbrli:xbrl>", &format!("{extra}</xbrli:xbrl>"));
        let limits = XbrlLimitOptions {
            max_xml_nodes: exact - 1,
            ..Default::default()
        };
        let result = xml::preflight(&xml, &limits);
        let refused = matches!(
            result,
            Err(XbrlFormatError::LimitExceeded {
                limit: XbrlLimit::XmlNodes,
                ..
            })
        );
        println!(
            "R1 {label}: DOM nodes={}, pre-DOM over-limit refused={refused}",
            roxmltree::Document::parse(&xml)
                .unwrap()
                .descendants()
                .count()
        );
        caught.push(refused);
        let exact_limits = XbrlLimitOptions {
            max_xml_nodes: exact,
            ..Default::default()
        };
        assert!(read(&xml, exact_limits).is_ok());
    }
    assert!(caught.into_iter().all(|refused| refused));
    assert!(read(&fixture(), XbrlLimitOptions::default()).is_ok());
}

#[test]
fn review_r2_split_leaf_text_cannot_silently_truncate() {
    let (schema, options) = boundary();
    let mut caught = Vec::new();
    for (label, xml) in [
        (
            "QName",
            fixture().replace("xbrli:pure", "xbrli:pure<!--x-->unbound:Other"),
        ),
        (
            "numeric fact",
            fixture().replace(">0</a:Amount>", ">1<!--x-->2</a:Amount>"),
        ),
        (
            "leading comment",
            fixture().replace(">0</a:Amount>", "><!--x-->12</a:Amount>"),
        ),
        (
            "PI and CDATA",
            fixture().replace(">0</a:Amount>", ">1<?split x?><![CDATA[2]]></a:Amount>"),
        ),
    ] {
        // Native behavior remains unchanged; the bounded profile must reject
        // before its first-child-text projection can lose the suffix.
        assert!(crate::from_str_with_options(&xml, &schema, &options).is_ok());
        let result = read(&xml, XbrlLimitOptions::default());
        println!("R2 {label}: unsupported={}", is_unsupported(&result));
        caught.push(is_unsupported(&result));
    }
    assert!(caught.into_iter().all(|refused| refused));
    // Adjacent text/reference/CDATA that the DOM merges is unambiguous.
    let valid = fixture().replace(">0</a:Amount>", "><![CDATA[1]]>&#50;</a:Amount>");
    assert_eq!(
        read(&valid, XbrlLimitOptions::default()).unwrap(),
        crate::from_str_with_options(&valid, &schema, &options).unwrap()
    );
    assert!(read(&fixture(), XbrlLimitOptions::default()).is_ok());
}

#[test]
fn review_r3_xml_nil_output_is_not_absence() {
    let (schema, options, instance) = rendered_fixture();
    let nil = replace_field(
        &instance,
        &["rows", "north", "Amount"],
        &Instance::Scalar(Value::xml_nil()),
    );
    let legacy = crate::to_string(&schema, &nil, &options).unwrap();
    assert!(
        !roxmltree::Document::parse(&legacy)
            .unwrap()
            .descendants()
            .any(|node| node.has_tag_name(("urn:a", "Amount")))
    );
    let result = to_bytes_bounded(&schema, &nil, &options, &XbrlLimits::default());
    println!("R3 populated XmlNil refused={}", is_unsupported(&result));
    assert!(is_unsupported(&result));
    let absent = replace_field(
        &instance,
        &["rows", "north", "Amount"],
        &Instance::Scalar(Value::Null),
    );
    assert_eq!(
        to_bytes_bounded(&schema, &absent, &options, &XbrlLimits::default()).unwrap(),
        crate::to_string(&schema, &absent, &options)
            .unwrap()
            .as_bytes()
    );
    assert!(to_bytes_bounded(&schema, &instance, &options, &XbrlLimits::default()).is_ok());
}

#[test]
fn review_r4_prefixed_and_expanded_dimension_ports_are_refused() {
    let mut caught = Vec::new();
    for name in [
        "d:explicitMember",
        "d:typedMember",
        &format!("{{{XBRLDI}}}explicitMember"),
    ] {
        let (mut schema, options) = boundary();
        let SchemaKind::Group { children, .. } = &mut schema.kind else {
            panic!()
        };
        let SchemaKind::Group { children, .. } = &mut children[0].kind else {
            panic!()
        };
        children.push(SchemaNode::scalar(name, ScalarType::String));
        let mut bindings = options.namespace_bindings().to_vec();
        bindings.push(XbrlNamespaceBinding::new(vec!["rows".into(), name.into()], XBRLDI).unwrap());
        let options = options.with_namespace_bindings(bindings).unwrap();
        let result = from_bytes_bounded(
            fixture().as_bytes(),
            &schema,
            &options,
            &XbrlLimits::default(),
        );
        println!("R4 {name}: unsupported={}", is_unsupported(&result));
        caught.push(is_unsupported(&result));
    }
    assert!(caught.into_iter().all(|refused| refused));
    // Ordinary prefixed concepts retain native local-name/namespace lookup.
    let schema = SchemaNode::group(
        "xbrl",
        vec![
            SchemaNode::group(
                "rows",
                vec![SchemaNode::scalar("a:Amount", ScalarType::Int)],
            )
            .repeating(),
        ],
    );
    let options = XbrlBoundaryOptions::external_source("taxonomy.xsd")
        .unwrap()
        .with_namespace_bindings(vec![
            XbrlNamespaceBinding::new(vec!["rows".into(), "a:Amount".into()], "urn:a").unwrap(),
        ])
        .unwrap();
    assert_eq!(
        from_bytes_bounded(
            fixture().as_bytes(),
            &schema,
            &options,
            &XbrlLimits::default()
        )
        .unwrap(),
        crate::from_str_with_options(&fixture(), &schema, &options).unwrap()
    );
}

#[test]
fn review_r5_returned_model_depth_counts_root_and_repeated_container() {
    let (schema, options) = boundary();
    let limits = XbrlLimits::new(XbrlLimitOptions {
        max_model_depth: 4,
        ..Default::default()
    })
    .unwrap();
    let result = from_bytes_bounded(fixture().as_bytes(), &schema, &options, &limits);
    println!("R5 depth-5 result admitted at depth 4={}", result.is_ok());
    assert!(matches!(
        result,
        Err(XbrlFormatError::LimitExceeded {
            limit: XbrlLimit::ModelDepth,
            ..
        })
    ));
    let limits = XbrlLimits::new(XbrlLimitOptions {
        max_model_depth: 5,
        ..Default::default()
    })
    .unwrap();
    let result = from_bytes_bounded(fixture().as_bytes(), &schema, &options, &limits).unwrap();
    ModelBudget::new(limits.options())
        .instance(&result, 1)
        .unwrap();
    assert_eq!(
        to_bytes_bounded(&schema, &result, &options, &limits).unwrap(),
        crate::to_string(&schema, &result, &options)
            .unwrap()
            .as_bytes()
    );
    // The minimal root/repeated-row/scalar shape is depth four, not three.
    let schema = SchemaNode::group(
        "xbrl",
        vec![
            SchemaNode::group("rows", vec![SchemaNode::scalar("Amount", ScalarType::Int)])
                .repeating(),
        ],
    );
    let options = XbrlBoundaryOptions::external_source("taxonomy.xsd")
        .unwrap()
        .with_namespace_bindings(vec![
            XbrlNamespaceBinding::new(vec!["rows".into(), "Amount".into()], "urn:a").unwrap(),
        ])
        .unwrap();
    let limits = XbrlLimits::new(XbrlLimitOptions {
        max_model_depth: 3,
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        from_bytes_bounded(fixture().as_bytes(), &schema, &options, &limits),
        Err(XbrlFormatError::LimitExceeded {
            limit: XbrlLimit::ModelDepth,
            ..
        })
    ));
}

#[test]
fn review_r6_unbound_simple_content_target_is_not_dropped() {
    let (mut schema, options, instance) = rendered_fixture();
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        panic!()
    };
    let SchemaKind::Group { children, .. } = &mut children[0].kind else {
        panic!()
    };
    children.push(SchemaNode::group(
        "Label",
        vec![SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text()],
    ));
    let Instance::Group(fields) = &instance else {
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
        "Label".into(),
        Instance::Group(
            vec![(
                XML_TEXT_FIELD.into(),
                Instance::Scalar(Value::String("must not disappear".into())),
            )]
            .into(),
        ),
    ));
    rows[0] = Instance::Group(row.into());
    let instance = Instance::Group(fields.into());
    assert!(crate::to_string(&schema, &instance, &options).is_ok());
    let result = to_bytes_bounded(&schema, &instance, &options, &XbrlLimits::default());
    println!(
        "R6 populated unbound text group refused={}",
        matches!(
            result,
            Err(XbrlFormatError::MissingFactNamespace { .. })
                | Err(XbrlFormatError::UnsupportedBoundedProfile(_))
        )
    );
    assert!(matches!(
        result,
        Err(XbrlFormatError::MissingFactNamespace { .. })
            | Err(XbrlFormatError::UnsupportedBoundedProfile(_))
    ));
    let (schema, options, instance) = rendered_fixture();
    assert_eq!(
        to_bytes_bounded(&schema, &instance, &options, &XbrlLimits::default()).unwrap(),
        crate::to_string(&schema, &instance, &options)
            .unwrap()
            .as_bytes()
    );
}

#[test]
fn review_r7_configured_counts_cannot_exceed_native_hard_maxima() {
    let mut caught = Vec::new();
    for (options, expected) in [
        (
            XbrlLimitOptions {
                max_contexts: crate::MAX_CONTEXTS + 1,
                ..Default::default()
            },
            XbrlLimit::Contexts,
        ),
        (
            XbrlLimitOptions {
                max_facts: crate::MAX_FACTS + 1,
                ..Default::default()
            },
            XbrlLimit::Facts,
        ),
    ] {
        let result = XbrlLimits::new(options);
        let refused =
            matches!(result, Err(XbrlFormatError::InvalidLimit(actual)) if actual == expected);
        println!("R7 {expected:?} hard-maximum overflow refused={refused}");
        caught.push(refused);
    }
    assert!(caught.into_iter().all(|refused| refused));
    let limits = XbrlLimits::new(XbrlLimitOptions {
        max_contexts: crate::MAX_CONTEXTS,
        max_facts: crate::MAX_FACTS,
        ..Default::default()
    })
    .unwrap();
    let (schema, options) = boundary();
    assert!(from_bytes_bounded(fixture().as_bytes(), &schema, &options, &limits).is_ok());
    assert!(read(&fixture(), XbrlLimitOptions::default()).is_ok());
}
