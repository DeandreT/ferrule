use super::*;
use mapping::{XbrlBoundaryOptions, XbrlFactBinding, XbrlFactType, XbrlNamespaceBinding};

fn table_schema() -> SchemaNode {
    let mut row = SchemaNode::group(
        "table1",
        vec![
            SchemaNode::group(
                "aspect",
                vec![SchemaNode::group(
                    "period",
                    vec![
                        SchemaNode::scalar("startDate", ScalarType::String),
                        SchemaNode::scalar("endDate", ScalarType::String),
                    ],
                )],
            ),
            SchemaNode::group(
                "concepts",
                vec![
                    SchemaNode::scalar("Revenue", ScalarType::Int),
                    SchemaNode::scalar("Expense", ScalarType::Int),
                ],
            ),
        ],
    );
    row.repeating = true;
    SchemaNode::group(
        "xbrl",
        vec![SchemaNode::group(
            "view",
            vec![SchemaNode::group("tableset", vec![row])],
        )],
    )
}

#[test]
fn projects_context_periods_and_facts_in_context_order() -> Result<(), Box<dyn std::error::Error>> {
    let xml = r#"<xbrli:xbrl xmlns:xbrli="http://www.xbrl.org/2003/instance" xmlns:ex="urn:example">
      <xbrli:context id="a"><xbrli:entity><xbrli:identifier scheme="urn:id">One</xbrli:identifier></xbrli:entity><xbrli:period><xbrli:startDate>2025-01-01</xbrli:startDate><xbrli:endDate>2025-12-31</xbrli:endDate></xbrli:period></xbrli:context>
      <xbrli:context id="b"><xbrli:entity><xbrli:identifier scheme="urn:id">Two</xbrli:identifier></xbrli:entity><xbrli:period><xbrli:startDate>2024-01-01</xbrli:startDate><xbrli:endDate>2024-12-31</xbrli:endDate></xbrli:period></xbrli:context>
      <ex:Revenue contextRef="a">120</ex:Revenue><ex:Expense contextRef="a">70</ex:Expense>
      <ex:Revenue contextRef="b">100</ex:Revenue><ex:Expense contextRef="b">60</ex:Expense>
    </xbrli:xbrl>"#;
    let instance = from_str(xml, &table_schema())?;
    let rows = instance
        .field("view")
        .and_then(|view| view.field("tableset"))
        .and_then(|tableset| tableset.field("table1"))
        .and_then(Instance::as_repeated)
        .ok_or("missing projected rows")?;
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0]
            .field("concepts")
            .and_then(|concepts| concepts.field("Revenue"))
            .and_then(Instance::as_scalar),
        Some(&Value::Int(120))
    );
    assert_eq!(
        rows[1]
            .field("aspect")
            .and_then(|aspect| aspect.field("period"))
            .and_then(|period| period.field("endDate"))
            .and_then(Instance::as_scalar),
        Some(&Value::String("2024-12-31".to_string()))
    );
    Ok(())
}

#[test]
fn rejects_duplicate_context_facts_and_ambiguous_table_schemas() {
    let xml = r#"<xbrli:xbrl xmlns:xbrli="http://www.xbrl.org/2003/instance" xmlns:ex="urn:example">
      <xbrli:context id="a"><xbrli:entity/><xbrli:period><xbrli:instant>2025-01-01</xbrli:instant></xbrli:period></xbrli:context>
      <ex:Revenue contextRef="a">1</ex:Revenue><ex:Revenue contextRef="a">2</ex:Revenue>
    </xbrli:xbrl>"#;
    assert!(matches!(
        from_str(xml, &table_schema()),
        Err(XbrlFormatError::DuplicateFact { .. })
    ));
    assert!(matches!(
        from_str(xml, &SchemaNode::group("xbrl", Vec::new())),
        Err(XbrlFormatError::InvalidTableSchema)
    ));
}

#[test]
fn options_aware_reader_distinguishes_same_local_names_and_requires_xbrli_root()
-> Result<(), Box<dyn std::error::Error>> {
    let mut row = SchemaNode::group(
        "rows",
        vec![
            SchemaNode::group(
                "period",
                vec![SchemaNode::scalar("instant", ScalarType::String)],
            ),
            SchemaNode::group("north", vec![SchemaNode::scalar("Amount", ScalarType::Int)]),
            SchemaNode::group("south", vec![SchemaNode::scalar("Amount", ScalarType::Int)]),
        ],
    );
    row.repeating = true;
    let schema = SchemaNode::group("xbrl", vec![row]);
    let options =
        XbrlBoundaryOptions::external_source("taxonomy.xsd")?.with_namespace_bindings(vec![
            XbrlNamespaceBinding::new(vec!["rows".into(), "period".into()], XBRLI)?,
            XbrlNamespaceBinding::new(
                vec!["rows".into(), "period".into(), "instant".into()],
                XBRLI,
            )?,
            XbrlNamespaceBinding::new(
                vec!["rows".into(), "north".into(), "Amount".into()],
                "urn:north",
            )?,
            XbrlNamespaceBinding::new(
                vec!["rows".into(), "south".into(), "Amount".into()],
                "urn:south",
            )?,
        ])?;
    let xml = r#"<xbrli:xbrl xmlns:xbrli="http://www.xbrl.org/2003/instance" xmlns:n="urn:north" xmlns:s="urn:south">
      <xbrli:context id="c"><xbrli:entity><xbrli:identifier scheme="urn:id">Entity</xbrli:identifier></xbrli:entity><xbrli:period><xbrli:instant>2026-06-30</xbrli:instant></xbrli:period></xbrli:context>
      <n:Amount contextRef="c">11</n:Amount><s:Amount contextRef="c">22</s:Amount>
    </xbrli:xbrl>"#;

    let instance = from_str_with_options(xml, &schema, &options)?;
    let row = instance
        .field("rows")
        .and_then(Instance::as_repeated)
        .and_then(|rows| rows.first())
        .ok_or("missing namespace-aware row")?;
    assert_eq!(
        row.field("north")
            .and_then(|group| group.field("Amount"))
            .and_then(Instance::as_scalar),
        Some(&Value::Int(11))
    );
    assert_eq!(
        row.field("south")
            .and_then(|group| group.field("Amount"))
            .and_then(Instance::as_scalar),
        Some(&Value::Int(22))
    );

    let lookalike = xml.replace(
        "http://www.xbrl.org/2003/instance",
        "urn:not-an-xbrl-instance",
    );
    assert!(matches!(
        from_str_with_options(&lookalike, &schema, &options),
        Err(XbrlFormatError::UnexpectedRoot { .. })
    ));
    Ok(())
}

#[test]
fn writes_contexts_and_namespace_qualified_facts() -> Result<(), Box<dyn std::error::Error>> {
    let mut row = SchemaNode::group(
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
            SchemaNode::scalar("Status", ScalarType::String),
        ],
    );
    row.repeating = true;
    let schema = SchemaNode::group("xbrl", vec![row]);
    let instance = Instance::Group(vec![(
        "rows".to_string(),
        Instance::Repeated(vec![Instance::Group(vec![
            (
                "identifier".to_string(),
                Instance::Group(vec![
                    (
                        "scheme".to_string(),
                        Instance::Scalar(Value::String("urn:entity".to_string())),
                    ),
                    (
                        XML_TEXT_FIELD.to_string(),
                        Instance::Scalar(Value::String("Example".to_string())),
                    ),
                ]),
            ),
            (
                "period".to_string(),
                Instance::Group(vec![(
                    "instant".to_string(),
                    Instance::Scalar(Value::String("2026-06-30".to_string())),
                )]),
            ),
            (
                "Status".to_string(),
                Instance::Scalar(Value::String("filed".to_string())),
            ),
        ])]),
    )]);
    let options = XbrlBoundaryOptions::external_target("taxonomy/report.xsd", None)?
        .with_namespace_bindings(vec![
            XbrlNamespaceBinding::new(vec!["rows".to_string(), "identifier".to_string()], XBRLI)?,
            XbrlNamespaceBinding::new(vec!["rows".to_string(), "period".to_string()], XBRLI)?,
            XbrlNamespaceBinding::new(
                vec!["rows".to_string(), "Status".to_string()],
                "urn:example",
            )?,
        ])?;

    let xml = to_string(&schema, &instance, &options)?;
    let document = roxmltree::Document::parse(&xml)?;
    let root = document.root_element();
    assert_eq!(root.tag_name().name(), "xbrl");
    let context = root
        .children()
        .find(|node| node.has_tag_name((XBRLI, "context")))
        .ok_or("missing context")?;
    assert_eq!(context.attribute("id"), Some("c1"));
    let fact = root
        .children()
        .find(|node| node.has_tag_name(("urn:example", "Status")))
        .ok_or("missing fact")?;
    assert_eq!(fact.attribute("contextRef"), Some("c1"));
    assert_eq!(fact.text(), Some("filed"));
    Ok(())
}

#[test]
fn writes_only_the_direct_explicit_dimension_owner() -> Result<(), Box<dyn std::error::Error>> {
    let mut row = SchemaNode::group(
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
            SchemaNode::group(
                "contextWrapper",
                vec![SchemaNode::group(
                    "RegionAxis",
                    vec![SchemaNode::scalar("explicitMember", ScalarType::String)],
                )],
            ),
            SchemaNode::scalar("Amount", ScalarType::String),
        ],
    );
    row.repeating = true;
    let schema = SchemaNode::group("xbrl", vec![row]);
    let instance = Instance::Group(vec![(
        "rows".into(),
        Instance::Repeated(vec![Instance::Group(vec![
            (
                "identifier".into(),
                Instance::Group(vec![
                    (
                        "scheme".into(),
                        Instance::Scalar(Value::String("urn:entity".into())),
                    ),
                    (
                        XML_TEXT_FIELD.into(),
                        Instance::Scalar(Value::String("Example".into())),
                    ),
                ]),
            ),
            (
                "period".into(),
                Instance::Group(vec![(
                    "instant".into(),
                    Instance::Scalar(Value::String("2026-06-30".into())),
                )]),
            ),
            (
                "contextWrapper".into(),
                Instance::Group(vec![(
                    "RegionAxis".into(),
                    Instance::Group(vec![(
                        "explicitMember".into(),
                        Instance::Scalar(Value::String("{urn:members}RegionMember".into())),
                    )]),
                )]),
            ),
            ("Amount".into(), Instance::Scalar(Value::String("5".into()))),
        ])]),
    )]);
    let options = XbrlBoundaryOptions::external_target("taxonomy.xsd", None)?
        .with_namespace_bindings(vec![
            XbrlNamespaceBinding::new(vec!["rows".into(), "identifier".into()], XBRLI)?,
            XbrlNamespaceBinding::new(vec!["rows".into(), "period".into()], XBRLI)?,
            XbrlNamespaceBinding::new(vec!["rows".into(), "contextWrapper".into()], XBRLI)?,
            XbrlNamespaceBinding::new(
                vec!["rows".into(), "contextWrapper".into(), "RegionAxis".into()],
                "urn:dimensions",
            )?,
            XbrlNamespaceBinding::new(
                vec![
                    "rows".into(),
                    "contextWrapper".into(),
                    "RegionAxis".into(),
                    "explicitMember".into(),
                ],
                XBRLDI,
            )?,
            XbrlNamespaceBinding::new(vec!["rows".into(), "Amount".into()], "urn:facts")?,
        ])?;

    let xml = to_string(&schema, &instance, &options)?;
    let document = roxmltree::Document::parse(&xml)?;
    let members = document
        .descendants()
        .filter(|node| node.has_tag_name((XBRLDI, "explicitMember")))
        .collect::<Vec<_>>();
    assert_eq!(members.len(), 1, "{xml}");
    let dimension = members[0]
        .attribute("dimension")
        .ok_or("missing dimension attribute")?;
    assert!(dimension.ends_with(":RegionAxis"), "{xml}");
    assert_eq!(
        members[0]
            .text()
            .map(|text| text.ends_with(":RegionMember")),
        Some(true)
    );
    Ok(())
}

#[test]
fn writes_class_specific_numeric_defaults_and_leaves_unbound_facts_unnumbered()
-> Result<(), Box<dyn std::error::Error>> {
    let default = |name: &str| {
        SchemaNode::group(
            name,
            vec![SchemaNode::scalar("decimals", ScalarType::String).attribute()],
        )
    };
    let direct_unit = || {
        SchemaNode::group(
            "unit",
            vec![
                SchemaNode::scalar("id", ScalarType::String).attribute(),
                SchemaNode::scalar("measure", ScalarType::String),
            ],
        )
    };
    let divide_unit = SchemaNode::group(
        "unit",
        vec![
            SchemaNode::scalar("id", ScalarType::String).attribute(),
            SchemaNode::group(
                "divide",
                vec![
                    SchemaNode::group(
                        "unitNumerator",
                        vec![SchemaNode::scalar("measure", ScalarType::String)],
                    ),
                    SchemaNode::group(
                        "unitDenominator",
                        vec![SchemaNode::scalar("measure", ScalarType::String)],
                    ),
                ],
            ),
        ],
    );
    let mut row = SchemaNode::group(
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
            SchemaNode::scalar("Amount", ScalarType::String),
            SchemaNode::scalar("Ratio", ScalarType::String),
            SchemaNode::scalar("ShareCount", ScalarType::String),
            SchemaNode::scalar("EPS", ScalarType::String),
            SchemaNode::scalar("Label", ScalarType::String),
        ],
    );
    row.repeating = true;
    let schema = SchemaNode::group(
        "xbrl",
        vec![
            SchemaNode::group(
                "defaults",
                vec![
                    default("monetaryItemType"),
                    default("numericItemType"),
                    default("sharesItemType"),
                    default("perShareItemType"),
                ],
            ),
            direct_unit(),
            direct_unit(),
            direct_unit(),
            divide_unit,
            row,
        ],
    );
    let scalar = |value: &str| Instance::Scalar(Value::String(value.into()));
    let default_value = |value: &str| Instance::Group(vec![("decimals".into(), scalar(value))]);
    let direct_unit_value = |id: &str, measure: &str| {
        Instance::Group(vec![
            ("id".into(), scalar(id)),
            ("measure".into(), scalar(measure)),
        ])
    };
    let instance = Instance::Group(vec![
        (
            "defaults".into(),
            Instance::Group(vec![
                ("monetaryItemType".into(), default_value("-2")),
                ("numericItemType".into(), default_value("-3")),
                ("sharesItemType".into(), default_value("0")),
                ("perShareItemType".into(), default_value("-4")),
            ]),
        ),
        (
            "unit".into(),
            direct_unit_value("", "{http://www.xbrl.org/2003/iso4217}USD"),
        ),
        (
            "unit".into(),
            direct_unit_value("pure", "{http://www.xbrl.org/2003/instance}pure"),
        ),
        (
            "unit".into(),
            direct_unit_value("", "{http://www.xbrl.org/2003/instance}shares"),
        ),
        (
            "unit".into(),
            Instance::Group(vec![
                ("id".into(), scalar("")),
                (
                    "divide".into(),
                    Instance::Group(vec![
                        (
                            "unitNumerator".into(),
                            Instance::Group(vec![(
                                "measure".into(),
                                scalar("{http://www.xbrl.org/2003/iso4217}USD"),
                            )]),
                        ),
                        (
                            "unitDenominator".into(),
                            Instance::Group(vec![(
                                "measure".into(),
                                scalar("{http://www.xbrl.org/2003/instance}shares"),
                            )]),
                        ),
                    ]),
                ),
            ]),
        ),
        (
            "rows".into(),
            Instance::Repeated(vec![Instance::Group(vec![
                (
                    "identifier".into(),
                    Instance::Group(vec![
                        ("scheme".into(), scalar("urn:entity")),
                        (XML_TEXT_FIELD.into(), scalar("Example")),
                    ]),
                ),
                (
                    "period".into(),
                    Instance::Group(vec![("instant".into(), scalar("2026-06-30"))]),
                ),
                ("Amount".into(), scalar("100.00")),
                ("Ratio".into(), scalar("0.125")),
                ("ShareCount".into(), scalar("42")),
                ("EPS".into(), scalar("2.50")),
                ("Label".into(), scalar("reported")),
            ])]),
        ),
    ]);
    let fact_paths = [
        ("Amount", XbrlFactType::Monetary),
        ("Ratio", XbrlFactType::Numeric),
        ("ShareCount", XbrlFactType::Shares),
        ("EPS", XbrlFactType::PerShare),
    ];
    let mut namespaces = vec![
        XbrlNamespaceBinding::new(vec!["unit".into()], XBRLI)?,
        XbrlNamespaceBinding::new(vec!["rows".into(), "identifier".into()], XBRLI)?,
        XbrlNamespaceBinding::new(vec!["rows".into(), "period".into()], XBRLI)?,
        XbrlNamespaceBinding::new(vec!["rows".into(), "Label".into()], "urn:facts")?,
    ];
    namespaces.extend(
        fact_paths
            .iter()
            .map(|(name, _)| {
                XbrlNamespaceBinding::new(vec!["rows".into(), (*name).into()], "urn:facts")
            })
            .collect::<Result<Vec<_>, _>>()?,
    );
    let options = XbrlBoundaryOptions::external_target("taxonomy.xsd", None)?
        .with_namespace_bindings(namespaces)?
        .with_fact_bindings(
            fact_paths
                .iter()
                .map(|(name, fact_type)| {
                    XbrlFactBinding::new(vec!["rows".into(), (*name).into()], *fact_type)
                })
                .collect::<Result<Vec<_>, _>>()?,
        )?;

    let xml = to_string(&schema, &instance, &options)?;
    let document = roxmltree::Document::parse(&xml)?;
    for (name, unit_ref, decimals) in [
        ("Amount", "USD", "-2"),
        ("Ratio", "pure", "-3"),
        ("ShareCount", "shares", "0"),
        ("EPS", "USD_per_shares", "-4"),
    ] {
        let fact = document
            .descendants()
            .find(|node| node.has_tag_name(("urn:facts", name)))
            .ok_or("missing typed fact")?;
        assert_eq!(fact.attribute("unitRef"), Some(unit_ref));
        assert_eq!(fact.attribute("decimals"), Some(decimals));
    }
    let label = document
        .descendants()
        .find(|node| node.has_tag_name(("urn:facts", "Label")))
        .ok_or("missing untyped fact")?;
    assert!(label.attribute("unitRef").is_none());
    assert!(label.attribute("decimals").is_none());
    Ok(())
}

#[test]
fn derives_per_share_unit_from_one_currency_measure() -> Result<(), Box<dyn std::error::Error>> {
    let schema = SchemaNode::group(
        "xbrl",
        vec![SchemaNode::group(
            "unit",
            vec![SchemaNode::scalar("measure", ScalarType::String)],
        )],
    );
    let instance = Instance::Group(vec![(
        "unit".into(),
        Instance::Group(vec![(
            "measure".into(),
            Instance::Scalar(Value::String(
                "{http://www.xbrl.org/2003/iso4217}USD".into(),
            )),
        )]),
    )]);
    let namespaces = BTreeMap::from([(vec!["unit".into()], XBRLI.to_string())]);
    let fact_types = BTreeMap::from([(vec!["rows".into(), "EPS".into()], XbrlFactType::PerShare)]);

    let defaults = target_defaults(&schema, &instance, &namespaces, &fact_types)?;

    assert_eq!(
        defaults.per_share.unit_ref.as_deref(),
        Some("USD_per_shares")
    );
    assert!(defaults.units.iter().any(|unit| {
        unit.id == "USD_per_shares"
            && unit.numerator.as_deref() == Some("{http://www.xbrl.org/2003/iso4217}USD")
            && unit.denominator.as_deref() == Some("{http://www.xbrl.org/2003/instance}shares")
    }));
    Ok(())
}

#[test]
fn writes_xbrli_named_row_wrappers_and_prunes_fact_empty_rows()
-> Result<(), Box<dyn std::error::Error>> {
    let identifier = SchemaNode::group(
        "identifier",
        vec![
            SchemaNode::scalar("scheme", ScalarType::String).attribute(),
            SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
        ],
    );
    let period = SchemaNode::group(
        "period",
        vec![SchemaNode::scalar("instant", ScalarType::String)],
    );
    let mut rows = SchemaNode::group(
        "identifier",
        vec![
            identifier,
            SchemaNode::group(
                "period",
                vec![
                    period,
                    SchemaNode::scalar("Amount", ScalarType::Float),
                    SchemaNode::scalar("footnote", ScalarType::String),
                ],
            ),
        ],
    );
    rows.repeating = true;
    let schema = SchemaNode::group(
        "xbrl",
        vec![
            SchemaNode::group(
                "unit",
                vec![SchemaNode::scalar("measure", ScalarType::String)],
            ),
            SchemaNode::group("Report", vec![rows]),
        ],
    );
    let scalar = |value: &str| Instance::Scalar(Value::String(value.into()));
    let row = |amount: Value| {
        Instance::Group(vec![
            (
                "identifier".into(),
                Instance::Group(vec![
                    ("scheme".into(), scalar("urn:entity")),
                    (XML_TEXT_FIELD.into(), scalar("Example")),
                ]),
            ),
            (
                "period".into(),
                Instance::Group(vec![
                    (
                        "period".into(),
                        Instance::Group(vec![("instant".into(), scalar("2026-06-30"))]),
                    ),
                    ("Amount".into(), Instance::Scalar(amount)),
                    ("footnote".into(), scalar("structural note")),
                ]),
            ),
        ])
    };
    let instance = Instance::Group(vec![
        (
            "unit".into(),
            Instance::Group(vec![(
                "measure".into(),
                scalar("{http://www.xbrl.org/2003/iso4217}USD"),
            )]),
        ),
        (
            "Report".into(),
            Instance::Group(vec![(
                "identifier".into(),
                Instance::Repeated(vec![row(Value::Null), row(Value::Float(42.0))]),
            )]),
        ),
    ]);
    let row_path = vec!["Report".into(), "identifier".into()];
    let fact_path = vec![
        "Report".into(),
        "identifier".into(),
        "period".into(),
        "Amount".into(),
    ];
    let options = XbrlBoundaryOptions::external_target("taxonomy.xsd", None)?
        .with_namespace_bindings(vec![
            XbrlNamespaceBinding::new(vec!["unit".into()], XBRLI)?,
            XbrlNamespaceBinding::new(row_path.clone(), XBRLI)?,
            XbrlNamespaceBinding::new(
                [row_path.clone(), vec!["identifier".into()]].concat(),
                XBRLI,
            )?,
            XbrlNamespaceBinding::new([row_path.clone(), vec!["period".into()]].concat(), XBRLI)?,
            XbrlNamespaceBinding::new(
                [row_path, vec!["period".into(), "period".into()]].concat(),
                XBRLI,
            )?,
            XbrlNamespaceBinding::new(fact_path.clone(), "urn:facts")?,
            XbrlNamespaceBinding::new(
                vec![
                    "Report".into(),
                    "identifier".into(),
                    "period".into(),
                    "footnote".into(),
                ],
                LINK,
            )?,
        ])?
        .with_fact_bindings(vec![XbrlFactBinding::new(
            fact_path,
            XbrlFactType::Monetary,
        )?])?;

    let xml = to_string(&schema, &instance, &options)?;
    let document = roxmltree::Document::parse(&xml)?;

    assert_eq!(
        document
            .descendants()
            .filter(|node| node.has_tag_name((XBRLI, "context")))
            .count(),
        1,
        "{xml}"
    );
    assert_eq!(
        document
            .descendants()
            .filter(|node| node.has_tag_name(("urn:facts", "Amount")))
            .count(),
        1,
        "{xml}"
    );
    assert_eq!(
        document
            .descendants()
            .filter(|node| node.has_tag_name((LINK, "footnote")))
            .count(),
        0,
        "{xml}"
    );
    Ok(())
}
