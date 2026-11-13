use super::*;

const PAIR: &str = include_str!("../../tests/fixtures/csv-declaration-proof/declared-pair.mfd");

fn check(text: &str, assertion: impl FnOnce(CsvDeclarationAssessment<'_, '_>)) {
    let doc = roxmltree::Document::parse(text).expect("synthetic XML parses");
    assertion(inspect(doc.root_element(), text));
}

fn declared(text: &str, assertion: impl FnOnce(&RawCensus<'_, '_>, &[SemanticObligation])) {
    check(text, |assessment| match assessment {
        CsvDeclarationAssessment::DeclarationOnly { proof, pending } => {
            assert!(proof.census.complete);
            assert!(proof.census.findings.is_empty());
            assert!(!pending.as_slice().is_empty());
            assertion(&proof.census, pending.as_slice());
        }
        other => panic!("expected declaration-only, got {other:#?}"),
    });
}

fn refused(text: &str, expected: FindingKind) {
    check(text, |assessment| match assessment {
        CsvDeclarationAssessment::Refused { partial } => {
            assert!(
                partial.findings.iter().any(|f| f.kind == expected),
                "missing {expected:?}: {:?}",
                partial.findings
            );
            assert_eq!(partial.source, text);
            for finding in &partial.findings {
                assert!(text.get(finding.origin.clone()).is_some());
                for related in &finding.related {
                    assert!(text.get(related.clone()).is_some());
                }
            }
        }
        other => panic!("expected refusal, got {other:#?}"),
    });
}

fn with_component(text: &str, component: &str) -> String {
    text.replace("</children>", &format!("{component}</children>"))
}

fn with_vertex(text: &str, vertex: &str) -> String {
    text.replace("</vertices>", &format!("{vertex}</vertices>"))
}

fn grouped() -> String {
    let text = with_component(
        PAIR,
        r#"
      <component name="group-by" library="core" uid="70006" kind="5">
        <sources><datapoint pos="0" key="80030"/><datapoint pos="1" key="80031"/></sources>
        <targets><datapoint pos="0" key="80032"/><datapoint pos="1" key="80033"/></targets>
      </component>
      <component name="concat" library="core" uid="70007" kind="5" growable="1" growablebasename="value">
        <sources><datapoint pos="0" key="80040"/><datapoint pos="1" key="80041"/></sources>
        <targets><datapoint pos="0" key="80042"/></targets>
      </component>"#,
    );
    let text = text
        .replace(
            r#"<vertex vertexkey="80002"><edges><edge vertexkey="80020"/></edges></vertex>"#,
            r#"<vertex vertexkey="80002"><edges><edge vertexkey="80040"/></edges></vertex>"#,
        )
        .replace(
            r#"<vertex vertexkey="80011"><edges><edge vertexkey="80021"/></edges></vertex>"#,
            r#"<vertex vertexkey="80011"><edges><edge vertexkey="80030"/></edges></vertex>"#,
        )
        .replace(
            r#"<vertex vertexkey="80012"><edges><edge vertexkey="80022"/></edges></vertex>"#,
            r#"<vertex vertexkey="80012"><edges><edge vertexkey="80031"/></edges></vertex>"#,
        );
    with_vertex(
        &text,
        r#"
      <vertex vertexkey="80032"><edges><edge vertexkey="80021"/></edges></vertex>
      <vertex vertexkey="80033"><edges><edge vertexkey="80022"/><edge vertexkey="80041"/></edges></vertex>
      <vertex vertexkey="80042"><edges><edge vertexkey="80020"/></edges></vertex>"#,
    )
}

#[test]
fn csv_declaration_proof_complete_source_bound_catalog_and_no_bridge_for_flat_pair() {
    declared(PAIR, |c, pending| {
        assert_eq!(c.digest, Some(Sha256::digest(PAIR.as_bytes()).into()));
        assert_eq!(c.family, Family::NotClaimed);
        assert_eq!(c.grammar_version, 1);
        assert!(c.unexamined_ranges.is_empty());
        assert_eq!(c.endpoints.len(), 2);
        assert_eq!(c.edges.len(), 4);
        assert_eq!(c.edge_visits, c.edges.len());
        assert_eq!(c.probe_edge_visits, c.edges.len());
        assert_eq!(c.probe_port_visits, c.ports.len());
        let source = c
            .endpoints
            .iter()
            .find(|e| e.role == EndpointRole::Source)
            .unwrap();
        assert_eq!(
            source
                .columns
                .iter()
                .map(|f| f.name.decoded.as_str())
                .collect::<Vec<_>>(),
            ["tag", "measureText"]
        );
        assert_eq!(source.transport.encoding, Some(1000));
        assert_eq!(source.transport.quote, Some('"'));
        assert_eq!(
            source.transport.record_terminator,
            RecordTerminator::NotDeclared
        );
        assert_eq!(source.namespace_table.len(), 2);
        assert_eq!(source.inline_block.decoded, "SampleLine");
        assert!(
            pending
                .iter()
                .any(|p| p.kind == ObligationKind::InputPhysicalSemantics)
        );
        for node in &c.nodes {
            assert_ne!(node.classification, Classification::Unknown);
            for attr in &node.attributes {
                assert_eq!(&PAIR[attr.value.origin.clone()], attr.value.raw);
                assert_ne!(attr.classification, Classification::Unknown);
            }
        }
    });
    check(PAIR, |a| {
        assert!(!a.blocks_executable());
        let mut warnings = vec!["existing".into()];
        a.append_warnings(&mut warnings);
        assert_eq!(warnings, ["existing"]);
    });
}

#[test]
fn csv_declaration_proof_grouped_receiver_uses_remain_unqualified_and_blocking() {
    let text = grouped();
    declared(&text, |c, pending| {
        assert_eq!(c.family, Family::GroupedCsvComposition);
        let group = pending
            .iter()
            .find(|p| p.kind == ObligationKind::GroupReceiverOwnership)
            .unwrap();
        assert_eq!(c.outgoing[&80033].len(), 2);
        assert!(group.ports.contains(&80033));
        assert!(
            group
                .edge_claims
                .iter()
                .filter(|&&e| c.edges[e].from == Some(80033))
                .count()
                == 2
        );
        assert!(pending.iter().all(|p| !p.origins.is_empty()));
    });
    check(&text, |a| {
        assert!(a.blocks_executable());
        let mut warnings = vec!["old conflict remains".into()];
        a.append_warnings(&mut warnings);
        assert_eq!(warnings[0], "old conflict remains");
        assert_eq!(warnings.len(), 2);
        assert!(warnings[1].contains("declaration-only"));
        assert!(!warnings[1].contains("violet"));
        assert!(!warnings[1].contains("unused.dat"));
    });
}

#[test]
fn csv_declaration_proof_source_buffer_and_root_identity_are_not_caller_assertions() {
    let doc = roxmltree::Document::parse(PAIR).unwrap();
    let copy = PAIR.to_owned();
    assert!(matches!(
        inspect(doc.root_element(), &copy),
        CsvDeclarationAssessment::Refused { .. }
    ));
    let child = doc.root_element().first_element_child().unwrap();
    assert!(matches!(
        inspect(child, PAIR),
        CsvDeclarationAssessment::Refused { .. }
    ));
}

#[test]
fn csv_declaration_proof_duplicate_containers_and_hidden_owners_never_win() {
    for (old, new) in [
        ("<resources/>", "<resources/><resources/>"),
        ("<structure>", "<structure/><structure>"),
        ("<children>", "<children/><children>"),
        ("<data><constant", "<data/><data><constant"),
        ("<header>", "<header/><header>"),
        ("<settings separator", "<settings/><settings separator"),
    ] {
        refused(&PAIR.replacen(old, new, 1), FindingKind::DuplicateContainer);
    }
    refused(
        &with_component(
            PAIR,
            r#"<component name="Shadow" library="other" uid="70003" kind="2"><targets><datapoint pos="0" key="80002"/></targets></component>"#,
        ),
        FindingKind::DuplicateComponent,
    );
    refused(
        &with_component(
            PAIR,
            r#"<component name="Shadow" library="other" uid="70020" kind="2"><targets><datapoint pos="0" key="80002"/></targets></component>"#,
        ),
        FindingKind::DuplicatePort,
    );
    refused(
        &PAIR.replace("<view ltx", r#"<view hidden="1" ltx"#),
        FindingKind::UnknownControl,
    );
    refused(
        &PAIR.replace(
            "<resources/>",
            r#"<resources><component name="Shadow" uid="70003"/></resources>"#,
        ),
        FindingKind::DuplicateComponent,
    );
}

#[test]
fn csv_declaration_proof_strict_tokens_and_parameter_preview_missing_empty_enabled() {
    for bad in ["080001", "+80001", "-1", " 80001", "4294967296", "0"] {
        refused(
            &PAIR.replacen(r#"key="80001""#, &format!("key=\"{bad}\""), 1),
            FindingKind::MalformedToken,
        );
    }
    for flag in [None, Some("0"), Some("1")] {
        let mut text = PAIR.replace(
            r#"previewvalue="examples/unused.dat""#,
            r#"previewvalue="""#,
        );
        text = text.replace(
            r#" usepreviewvalue="1""#,
            &flag
                .map(|s| format!(" usepreviewvalue=\"{s}\""))
                .unwrap_or_default(),
        );
        declared(&text, |c, _| {
            let p = &c.parameters[0];
            assert_eq!(p.required, Flag::Missing);
            assert_eq!(p.optional, Flag::Missing);
            assert_eq!(p.preview.as_ref().unwrap().decoded, "");
            assert_eq!(
                p.use_preview,
                match flag {
                    None => Flag::Missing,
                    Some("0") => Flag::Zero,
                    _ => Flag::One,
                }
            );
        });
    }
    declared(
        &PAIR.replace(
            r#" previewvalue="examples/unused.dat" usepreviewvalue="1""#,
            "",
        ),
        |c, _| assert!(c.parameters[0].preview.is_none()),
    );
    for extra in [
        r#"required="0""#,
        r#"value="not-a-default""#,
        r#"default="not-a-default""#,
    ] {
        let text = PAIR.replace(
            r#"<input datatype="string""#,
            &format!("<input {extra} datatype=\"string\""),
        );
        refused(
            &text,
            if extra.starts_with("required") {
                FindingKind::InvalidParameter
            } else {
                FindingKind::UnknownControl
            },
        );
    }
    refused(
        &PAIR.replace(r#"usepreviewvalue="1""#, r#"usepreviewvalue="true""#),
        FindingKind::InvalidParameter,
    );
    refused(
        &PAIR.replace(r#"usageKind="input""#, r#"usageKind="output""#),
        FindingKind::InvalidParameter,
    );
}

#[test]
fn csv_declaration_proof_literal_empty_escaped_wsdl_and_unknown_controls() {
    for literal in ["", "violet &amp; blue"] {
        declared(
            &PAIR.replace("value=\"violet\"", &format!("value=\"{literal}\"")),
            |c, _| assert_eq!(c.literals.len(), 1),
        );
    }
    refused(
        &PAIR.replace("<wsdl/>", "<wsdl/><wsdl/>"),
        FindingKind::DuplicateContainer,
    );
    refused(
        &PAIR.replace("<wsdl/>", "<wsdl hidden=\"1\"/>"),
        FindingKind::UnknownControl,
    );
    refused(
        &PAIR.replace(
            "<wsdl/>",
            "<wsdl/><constant datatype=\"string\" value=\"new\"/>",
        ),
        FindingKind::DuplicateContainer,
    );
    refused(
        &PAIR.replace(
            "<constant datatype=\"string\"",
            "<constant datatype=\"integer\"",
        ),
        FindingKind::InvalidLiteral,
    );
}

#[test]
fn csv_declaration_proof_namespace_lexical_declarations_shadows_and_expanded_names() {
    declared(PAIR, |c, _| {
        let ns = c
            .nodes
            .iter()
            .flat_map(|n| &n.namespaces)
            .collect::<Vec<_>>();
        assert_eq!(ns.len(), 1);
        assert_eq!(ns[0].prefix, "xsi");
        assert!(PAIR[ns[0].origin.clone()].starts_with("xmlns:xsi"));
    });
    refused(
        &PAIR.replace(
            "<view ltx",
            r#"<view xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" ltx"#,
        ),
        FindingKind::NamespaceMismatch,
    );
    refused(
        &PAIR.replace("<view ltx", r#"<view xmlns="urn:synthetic:foreign" ltx"#),
        FindingKind::UnknownXml,
    );
    refused(
        &PAIR.replace("ns=\"1\"", "ns=\"7\""),
        FindingKind::NamespaceMismatch,
    );
    refused(
        &PAIR.replace(PROTOCOL, "urn:synthetic:wrong-protocol"),
        FindingKind::NamespaceMismatch,
    );
    refused(
        &PAIR.replace(
            "<view ltx",
            r#"<view xmlns:z="urn:synthetic:z" z:hidden="1" ltx"#,
        ),
        FindingKind::UnknownControl,
    );
}

#[test]
fn csv_declaration_proof_pi_cdata_dtd_text_and_comment_controls() {
    declared(
        &PAIR.replace("<resources/>", "<!-- neutral --><resources/>"),
        |_, _| {},
    );
    refused(
        &PAIR.replace("<resources/>", "<?synthetic test?><resources/>"),
        FindingKind::UnknownXml,
    );
    refused(
        &PAIR.replace("<resources/>", "<![CDATA[ ]]><resources/>"),
        FindingKind::UnknownXml,
    );
    refused(
        &PAIR.replace("<resources/>", "unexpected<resources/>"),
        FindingKind::UnknownXml,
    );
    let text = PAIR.replace(
        "<mapping ",
        "<!DOCTYPE mapping [<!ENTITY harmless 'neutral'>]><mapping ",
    );
    let doc = roxmltree::Document::parse_with_options(
        &text,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(matches!(
        inspect(doc.root_element(), &text),
        CsvDeclarationAssessment::Refused { .. }
    ));
}

#[test]
fn csv_declaration_proof_transport_wrappers_columns_and_constraints() {
    for (old, new, kind) in [
        (
            "encoding=\"1000\"",
            "encoding=\"999\"",
            FindingKind::ConflictingSetting,
        ),
        (
            "byteordermark=\"0\"",
            "byteordermark=\"1\"",
            FindingKind::ConflictingSetting,
        ),
        (
            "removeempty=\"true\"",
            "removeempty=\"1\"",
            FindingKind::ConflictingSetting,
        ),
        ("<field1 ", "<field01 ", FindingKind::InvalidColumnOrder),
        ("<field1 ", "<field0 ", FindingKind::DuplicateColumn),
        (
            "type=\"string\"/>",
            "type=\"mystery\"/>",
            FindingKind::UnsupportedColumnType,
        ),
        (
            "<field0 ",
            "<field0 nullable=\"1\" ",
            FindingKind::UnknownControl,
        ),
        (
            "block=\"SampleLine\"",
            "block=\"OtherLine\"",
            FindingKind::MalformedWrapper,
        ),
        (
            "ns=\"1\" inpkey=\"80010\"",
            "ns=\"1\" outkey=\"80010\"",
            FindingKind::UnknownControl,
        ),
    ] {
        refused(&PAIR.replacen(old, new, 1), kind);
    }
    refused(
        &PAIR.replace(
            "<settings separator",
            "<settings recordterminator=\"invented\" separator",
        ),
        FindingKind::UnknownControl,
    );
    refused(
        &PAIR.replace(
            "<entry name=\"tag\" outkey=\"80012\"/>",
            "<entry name=\"tag\" outkey=\"80012\"/><entry name=\"extra\" outkey=\"80100\"/>",
        ),
        FindingKind::InvalidColumnOrder,
    );
    let unsupported = PAIR
        .replace("encoding=\"1000\"", "encoding=\"999\"")
        .replace("byteordermark=\"0\"", "byteordermark=\"1\"");
    check(&unsupported, |a| match a {
        CsvDeclarationAssessment::Refused { partial } => {
            // A refused partial witness must not substitute the supported profile's
            // encoding/BOM into the authored raw declaration.
            assert!(
                partial
                    .endpoints
                    .iter()
                    .all(|e| e.transport.encoding == Some(999) && e.transport.bom == Flag::One)
            );
            assert!(!partial.endpoints.is_empty());
        }
        _ => panic!("unsupported transport is not a declaration proof"),
    });
}

#[test]
fn csv_declaration_proof_numeric_column_order_not_lexical_or_xml_order() {
    let fields = (0..11)
        .rev()
        .map(|i| format!("<field{i} name=\"sample{i}\" type=\"string\"/>"))
        .collect::<String>();
    let leaves = (0..11)
        .map(|i| format!("<entry name=\"sample{i}\" outkey=\"{}\"/>", 80100 + i))
        .collect::<String>();
    let text = PAIR
        .replace(
            "<field0 name=\"tag\" type=\"string\"/><field1 name=\"measureText\" type=\"string\"/>",
            &fields,
        )
        .replace(
            "<entry name=\"tag\" outkey=\"80012\"/><entry name=\"measureText\" outkey=\"80013\"/>",
            &leaves,
        )
        .replace("vertexkey=\"80012\"", "vertexkey=\"80100\"");
    declared(&text, |c, _| {
        let s = c
            .endpoints
            .iter()
            .find(|e| e.role == EndpointRole::Source)
            .unwrap();
        assert_eq!(s.columns[10].index, 10);
        assert_eq!(s.columns[10].xml_ordinal, 0);
        assert_eq!(s.columns[2].name.decoded, "sample2");
    });
}

#[test]
fn csv_declaration_proof_all_claims_duplicates_conflicts_wrong_directions_and_cycles() {
    refused(
        &PAIR.replace(
            "<edge vertexkey=\"80022\"/>",
            "<edge vertexkey=\"80022\"/><edge vertexkey=\"80022\"/>",
        ),
        FindingKind::DuplicateEdgeClaim,
    );
    refused(
        &with_vertex(
            PAIR,
            "<vertex vertexkey=\"80013\"><edges><edge vertexkey=\"80022\"/></edges></vertex>",
        ),
        FindingKind::ConflictingFeed,
    );
    refused(
        &PAIR.replace("vertexkey=\"80022\"", "vertexkey=\"89999\""),
        FindingKind::DanglingEdge,
    );
    refused(
        &PAIR.replace("vertexkey=\"80022\"", "vertexkey=\"80013\""),
        FindingKind::WrongPortDirection,
    );
    let cyclic = grouped()
        .replace(
            "<vertex vertexkey=\"80002\"><edges><edge vertexkey=\"80040\"/></edges></vertex>",
            "<vertex vertexkey=\"80002\"><edges/></vertex>",
        )
        .replace(
            "<edge vertexkey=\"80020\"/>",
            "<edge vertexkey=\"80020\"/><edge vertexkey=\"80040\"/>",
        );
    refused(&cyclic, FindingKind::GraphCycle);
    let mixed = PAIR.replace(
        "</structure>",
        "<connections><edge from=\"80013\" to=\"80022\"/></connections></structure>",
    );
    refused(&mixed, FindingKind::DuplicateContainer);
    refused(&mixed, FindingKind::ConflictingFeed);
}

#[test]
fn csv_declaration_proof_legacy_only_and_annotation_identity() {
    let start = PAIR.find("<graph directed").unwrap();
    let end = PAIR.find("</graph>").unwrap() + "</graph>".len();
    let text = format!(
        "{}<connections><edge from=\"80001\" to=\"80010\"/><edge from=\"80002\" to=\"80020\"/><edge from=\"80011\" to=\"80021\"/><edge from=\"80012\" to=\"80022\"/></connections>{}",
        &PAIR[..start],
        &PAIR[end..]
    );
    declared(&text, |c, _| {
        assert!(c.edges.iter().all(|e| e.vocabulary == Vocabulary::Legacy))
    });
    let annotated=PAIR.replace("<graph directed=\"1\"><vertices>","<graph directed=\"1\"><edges><edge edgekey=\"90001\"><data><dataconnection name=\"neutral explanation\" starting_location=\"0\"/></data></edge></edges><vertices>")
        .replace("<edge vertexkey=\"80022\"/>","<edge vertexkey=\"80022\" edgekey=\"90001\"/>");
    declared(&annotated, |_, p| {
        assert!(
            p.iter()
                .any(|p| p.kind == ObligationKind::UnqualifiedSavedAnnotationControl)
        )
    });
    refused(
        &annotated.replace("edgekey=\"90001\"/>", "edgekey=\"90002\"/>"),
        FindingKind::UnmatchedAnnotation,
    );
    refused(
        &annotated.replace(
            "name=\"neutral explanation\"",
            "name=\"neutral explanation\" type=\"2\"",
        ),
        FindingKind::UnknownControl,
    );
}

#[test]
fn csv_declaration_proof_string_sum_and_empty_control_signatures() {
    let text=with_component(PAIR,r#"<component name="sum" library="core" uid="70008" kind="5"><sources><datapoint/><datapoint pos="1" key="80050"/></sources><targets><datapoint pos="0" key="80051"/></targets></component>"#)
        .replace("<vertex vertexkey=\"80012\"><edges><edge vertexkey=\"80022\"/></edges></vertex>","<vertex vertexkey=\"80013\"><edges><edge vertexkey=\"80050\"/></edges></vertex>");
    let text = with_vertex(
        &text,
        "<vertex vertexkey=\"80051\"><edges><edge vertexkey=\"80022\"/></edges></vertex>",
    );
    declared(&text, |c, p| {
        let sum = c
            .functions
            .iter()
            .find(|f| f.kind == CallKind::Sum)
            .unwrap();
        assert_eq!(sum.inputs[0].position, None);
        assert_eq!(sum.inputs[0].key, None);
        assert!(
            p.iter()
                .any(|p| p.kind == ObligationKind::AggregateFrameAndStringConversion)
        );
    });
    refused(
        &text.replace(
            "<datapoint/><datapoint pos=\"1\" key=\"80050\"/>",
            "<datapoint pos=\"0\" key=\"80052\"/><datapoint pos=\"1\" key=\"80050\"/>",
        ),
        FindingKind::InvalidFunctionSignature,
    );
    for (name, library, kind, sources, obligation) in [
        (
            "auto-number",
            "core",
            5,
            "<datapoint/><datapoint/><datapoint/><datapoint/>",
            ObligationKind::NumberingScope,
        ),
        ("now", "lang", 5, "", ObligationKind::ClockAndDatePicture),
    ] {
        let component = format!(
            "<component name=\"{name}\" library=\"{library}\" uid=\"70009\" kind=\"{kind}\"><sources>{sources}</sources><targets><datapoint pos=\"0\" key=\"80060\"/></targets></component>"
        );
        declared(&with_component(PAIR, &component), |_, p| {
            assert!(p.iter().any(|p| p.kind == obligation))
        });
    }
}

#[test]
fn csv_declaration_proof_unqualified_physical_and_saved_generation_controls() {
    let text=with_component(&grouped(),r#"
      <component name="NeutralDelimiter" library="core" uid="70010" kind="2"><targets><datapoint pos="0" key="80070"/></targets><data><constant datatype="string" value=";"/></data></component>
      <component name="concat" library="core" uid="70011" kind="5" growable="1" growablebasename="value"><sources><datapoint pos="0" key="80071"/><datapoint pos="1" key="80072"/></sources><targets><datapoint pos="0" key="80073"/></targets></component>"#)
        .replace("quote=\"&apos;\"","quote=\"\"")
        .replace("<edge vertexkey=\"80022\"/><edge vertexkey=\"80041\"/>","<edge vertexkey=\"80071\"/><edge vertexkey=\"80041\"/>")
        .replace("SelectedLanguage=\"builtin\"","SelectedLanguage=\"builtin\" MakePathsAbsoluteOnGeneration=\"1\" UseWindowsPathConvention=\"0\"");
    let text = with_vertex(
        &text,
        "<vertex vertexkey=\"80070\"><edges><edge vertexkey=\"80072\"/></edges></vertex><vertex vertexkey=\"80073\"><edges><edge vertexkey=\"80022\"/></edges></vertex>",
    );
    declared(&text, |c, p| {
        assert_eq!(c.incoming[&80022].len(), 1);
        assert_eq!(c.edges[c.incoming[&80022][0]].from, Some(80073));
        assert!(c.literals.iter().any(|l| l.value.decoded == ";"));
        assert!(
            p.iter()
                .any(|p| p.kind == ObligationKind::DisabledQuotePhysicalBoundary)
        );
        assert!(
            p.iter()
                .any(|p| p.kind == ObligationKind::RecordTerminatorNotDeclared)
        );
        assert!(
            p.iter()
                .any(|p| p.kind == ObligationKind::SavedGenerationPathPolicy)
        );
    });
}

#[test]
fn csv_declaration_proof_all_callable_signatures_preserve_raw_positions_and_placeholders() {
    for (name, library, kind, count, optional) in [
        ("empty", "lang", 5, 1, false),
        ("right-trim", "lang", 5, 1, false),
        ("substring-before", "core", 5, 2, false),
        ("substitute-null", "db", 5, 2, false),
        ("if-else", "core", 4, 3, false),
        ("format-dateTime", "core", 5, 2, true),
    ] {
        let mut sources = (0..count)
            .map(|p| format!("<datapoint pos=\"{p}\" key=\"{}\"/>", 80200 + p))
            .collect::<String>();
        if optional {
            sources.push_str("<datapoint/>");
        }
        let component = format!(
            "<component name=\"{name}\" library=\"{library}\" uid=\"70100\" kind=\"{kind}\"><sources>{sources}</sources><targets><datapoint pos=\"0\" key=\"80210\"/></targets></component>"
        );
        let feeds = (0..count)
            .map(|p| format!("<edge vertexkey=\"{}\"/>", 80200 + p))
            .collect::<String>();
        let text = with_component(PAIR, &component).replace(
            "<edge vertexkey=\"80020\"/>",
            &format!("<edge vertexkey=\"80020\"/>{feeds}"),
        );
        declared(&text, |c, _| {
            let call = c.functions.last().unwrap();
            assert_eq!(call.inputs.len(), count as usize + usize::from(optional));
            assert_eq!(call.inputs[0].position, Some(0));
            if optional {
                assert_eq!(call.inputs.last().unwrap().key_presence, Presence::Missing);
            }
        });
        refused(
            &text
                .replace(
                    "library=\"lang\" uid=\"70100\"",
                    "library=\"other\" uid=\"70100\"",
                )
                .replace(
                    "library=\"core\" uid=\"70100\"",
                    "library=\"other\" uid=\"70100\"",
                )
                .replace(
                    "library=\"db\" uid=\"70100\"",
                    "library=\"other\" uid=\"70100\"",
                ),
            FindingKind::InvalidFunctionSignature,
        );
    }
    let only_key = grouped()
        .replace("<datapoint pos=\"0\" key=\"80032\"/>", "<datapoint/>")
        .replace(
            "<vertex vertexkey=\"80032\"><edges><edge vertexkey=\"80021\"/></edges></vertex>",
            "",
        )
        .replace(
            "<edge vertexkey=\"80030\"/>",
            "<edge vertexkey=\"80030\"/><edge vertexkey=\"80021\"/>",
        );
    declared(&only_key, |c, p| {
        let g = c
            .functions
            .iter()
            .find(|f| f.kind == CallKind::GroupBy)
            .unwrap();
        assert_eq!(g.outputs[0].key_presence, Presence::Missing);
        assert_eq!(g.outputs[1].position, Some(1));
        assert!(
            p.iter()
                .any(|p| p.kind == ObligationKind::GroupReceiverOwnership)
        );
    });
    let shuffled = grouped().replace(
        "<datapoint pos=\"0\" key=\"80032\"/><datapoint pos=\"1\" key=\"80033\"/>",
        "<datapoint pos=\"1\" key=\"80033\"/><datapoint pos=\"0\" key=\"80032\"/>",
    );
    declared(&shuffled, |c, _| {
        let g = c
            .functions
            .iter()
            .find(|f| f.kind == CallKind::GroupBy)
            .unwrap();
        assert_eq!(g.outputs[0].position, Some(1));
        assert_eq!(g.outputs[0].ordinal, 0);
    });
    refused(
        &PAIR.replace(
            "<sources><datapoint/></sources>",
            "<sources><datapoint key=\"\"/></sources>",
        ),
        FindingKind::MalformedToken,
    );
}

#[test]
fn csv_declaration_proof_local_metadata_classes_and_empty_edges_are_explicit() {
    declared(PAIR, |c, _| {
        let scroll = c
            .nodes
            .iter()
            .flat_map(|n| &n.attributes)
            .find(|a| a.name == "scrollposition")
            .unwrap();
        assert_eq!(scroll.classification, Classification::Layout);
        let show = c
            .nodes
            .iter()
            .flat_map(|n| &n.attributes)
            .find(|a| a.name == "ShowLibraryNameInHeader")
            .unwrap();
        assert_eq!(show.classification, Classification::Layout);
        assert!(
            c.markup_events
                .iter()
                .any(|(range, class)| PAIR[range.clone()].starts_with("<?xml")
                    && *class == Classification::Syntax)
        );
    });
    refused(
        &PAIR.replace(
            "<edge vertexkey=\"80022\"/>",
            "<edge vertexkey=\"80022\"/><edge/>",
        ),
        FindingKind::DanglingEdge,
    );
    refused(
        &with_vertex(PAIR, "<vertex vertexkey=\"89998\"><edges/></vertex>"),
        FindingKind::DanglingEdge,
    );
    let unknown = PAIR.replace(
        "<resources/>",
        "<resources><preview><hidden/></preview></resources>",
    );
    refused(&unknown, FindingKind::UnknownXml);
}

#[test]
fn csv_declaration_proof_source_identity_is_generic_and_repeat_results_are_deterministic() {
    let renamed = PAIR
        .replace("SampleFeed", "NeutralAlpha")
        .replace("SampleResult", "NeutralBeta")
        .replace("tag", "symbol")
        .replace("violet", "indigo")
        .replace("7000", "7100")
        .replace("800", "810");
    declared(&renamed, |c, _| {
        assert_eq!(c.endpoints.len(), 2);
        assert_eq!(c.parameters[0].name.decoded, "feed_path");
    });
    let text = grouped().replace("<resources/>", "<resources surprise=\"1\"/>");
    let summary = |text: &str| {
        let mut result = Vec::new();
        check(text, |a| {
            if let CsvDeclarationAssessment::Refused { partial } = a {
                result = partial
                    .findings
                    .iter()
                    .map(|f| (f.kind, f.origin.clone(), f.related.clone()))
                    .collect();
            } else {
                panic!("expected refusal");
            }
        });
        result
    };
    assert_eq!(summary(&text), summary(&text));
}

#[test]
fn csv_declaration_proof_each_allocation_budget_has_a_finite_refusal_witness() {
    // No data mapping or reference process participates in these raw controls.
    let attrs = (0..MAX_ATTRIBUTES)
        .map(|i| format!(" a{i}=\"x\""))
        .collect::<String>();
    refused(
        &PAIR.replace("<resources/>", &format!("<resources{attrs} extra=\"x\"/>")),
        FindingKind::LimitExceeded,
    );
    let elements = PAIR.replace(
        "<resources/>",
        &format!("<resources/>{}", "<u/>".repeat(MAX_ELEMENTS)),
    );
    refused(&elements, FindingKind::LimitExceeded);
    let nodes = PAIR.replace(
        "<resources/>",
        &format!("<resources/>{}", "<!-- x -->".repeat(MAX_NODES)),
    );
    refused(&nodes, FindingKind::LimitExceeded);
    let components = (0..MAX_COMPONENTS)
        .map(|i| format!("<component uid=\"{}\"/>", 100000 + i))
        .collect::<String>();
    refused(
        &with_component(PAIR, &components),
        FindingKind::LimitExceeded,
    );
    let ports = (0..MAX_PORTS)
        .map(|i| format!("<entry name=\"p{i}\" outkey=\"{}\"/>", 110000 + i))
        .collect::<String>();
    refused(
        &PAIR.replace("<resources/>", &format!("<resources>{ports}</resources>")),
        FindingKind::LimitExceeded,
    );
    let edges = PAIR.replace(
        "<edge vertexkey=\"80022\"/>",
        &"<edge vertexkey=\"80022\"/>".repeat(MAX_EDGES),
    );
    refused(&edges, FindingKind::LimitExceeded);
    let fields = (0..=MAX_COLUMNS)
        .map(|i| format!("<field{i} name=\"f{i}\" type=\"string\"/>"))
        .collect::<String>();
    refused(
        &PAIR.replace("<field0 name=\"token\" type=\"string\"/>", &fields),
        FindingKind::LimitExceeded,
    );
    let slots = (0..=MAX_SLOTS).map(|_| "<datapoint/>").collect::<String>();
    refused(
        &with_component(
            PAIR,
            &format!(
                "<component name=\"auto-number\" uid=\"70101\" library=\"core\" kind=\"5\"><sources>{slots}</sources></component>"
            ),
        ),
        FindingKind::LimitExceeded,
    );
    let comment = format!("<!--{}-->", "x".repeat(MAX_VALUE + 1));
    refused(
        &PAIR.replace("<resources/>", &format!("{comment}<resources/>")),
        FindingKind::LimitExceeded,
    );
    // An exact byte boundary can remain declaration-only using bounded comments.
    let mut exact = PAIR.to_owned();
    let mut remaining = MAX_BYTES - exact.len();
    while remaining > 7 {
        let payload = (remaining - 7).min(MAX_VALUE - 7);
        exact.push_str(&format!("<!--{}-->", "x".repeat(payload)));
        remaining -= payload + 7;
    }
    exact.push_str(&" ".repeat(remaining));
    assert_eq!(exact.len(), MAX_BYTES);
    declared(&exact, |_, _| {});
}

#[test]
fn csv_declaration_proof_budgets_caps_and_work_are_not_positive_certificates() {
    let large = format!("{}{}", PAIR, " ".repeat(MAX_BYTES));
    check(&large, |a| match a {
        CsvDeclarationAssessment::Refused { partial } => {
            assert!(!partial.complete);
            assert_eq!(partial.family, Family::Uncertain);
            assert!(!partial.unexamined_ranges.is_empty());
            assert_eq!(partial.findings[0].kind, FindingKind::LimitExceeded);
        }
        _ => panic!("limit must refuse"),
    });
    let long = PAIR.replace(
        "value=\"violet\"",
        &format!("value=\"{}\"", "x".repeat(MAX_VALUE + 1)),
    );
    refused(&long, FindingKind::LimitExceeded);
    let many = PAIR.replace(
        "<resources/>",
        &format!("<resources/>{}", "<unknown/>".repeat(MAX_FINDINGS + 4)),
    );
    check(&many, |a| match a {
        CsvDeclarationAssessment::Refused { partial } => {
            assert_eq!(partial.findings.len(), MAX_FINDINGS);
            assert!(partial.suppressed_findings > 0);
            assert!(!partial.complete);
        }
        _ => panic!("cap cannot seal"),
    });
    let deep = PAIR.replace(
        "<resources/>",
        &format!(
            "<resources/>{}{}",
            "<u>".repeat(MAX_DEPTH),
            "</u>".repeat(MAX_DEPTH)
        ),
    );
    refused(&deep, FindingKind::LimitExceeded);
    declared(PAIR, |c, _| {
        assert_eq!(
            c.element_visits,
            c.nodes.iter().filter(|n| n.xml.is_element()).count()
        )
    });
    let a = grouped();
    let b = a.replace(
        "<resources/>",
        "<!-- unrelated neutral comment --><resources/>",
    );
    declared(&a, |first, _| {
        declared(&b, |second, _| {
            assert_eq!(first.element_visits, second.element_visits);
            assert_eq!(first.edge_visits, second.edge_visits);
        })
    });
}

#[test]
fn csv_declaration_proof_refused_in_family_still_blocks_and_messages_are_safe() {
    let text = grouped()
        .replace(
            "value=\"violet\"",
            "value=\"private-looking-synthetic-sentinel\"",
        )
        .replace("<resources/>", "<resources hidden=\"1\"/>");
    check(&text, |a| {
        assert!(a.blocks_executable());
        let mut warnings = vec!["existing marker".into()];
        a.append_warnings(&mut warnings);
        assert_eq!(warnings[0], "existing marker");
        assert!(warnings.len() > 1);
        assert!(
            !warnings
                .join(" ")
                .contains("private-looking-synthetic-sentinel")
        );
    });
}

#[test]
fn csv_declaration_proof_shadow_row_does_not_hide_the_in_family_refusal() {
    let text=grouped().replace("<entry name=\"SampleLine\" inpkey=\"80021\"><entry name=\"token\" inpkey=\"80022\"/></entry>","<entry name=\"SampleLine\" inpkey=\"80021\"><entry name=\"token\" inpkey=\"80022\"/></entry><entry name=\"ShadowLine\" inpkey=\"80250\"/>");
    check(&text, |a| {
        assert!(a.blocks_executable());
        assert!(matches!(a, CsvDeclarationAssessment::Refused { .. }));
    });
    let duplicated = grouped().replace(
        "<edge vertexkey=\"80022\"/>",
        &"<edge vertexkey=\"80022\"/>".repeat(3),
    );
    check(&duplicated, |a| match a {
        CsvDeclarationAssessment::Refused { partial } => {
            let duplicate = partial
                .findings
                .iter()
                .find(|f| f.kind == FindingKind::DuplicateEdgeClaim)
                .unwrap();
            assert_eq!(duplicate.related.len(), 3);
            assert_eq!(partial.probe_edge_visits, partial.edges.len());
            assert_eq!(partial.probe_port_visits, partial.ports.len());
        }
        _ => panic!("duplicate claims cannot seal"),
    });
}

#[test]
fn csv_declaration_proof_flat_import_retention_and_explicit_other_route_keep_their_policy() {
    let root = std::env::temp_dir().join(format!(
        "ferrule-csv-declaration-flat-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("synthetic-flat.mfd");
    std::fs::write(&path, PAIR).unwrap();
    let imported = super::super::import(&path).expect("ordinary synthetic repair import");
    assert!(
        !imported
            .warnings
            .iter()
            .any(|w| w.starts_with("CSV declaration proof:"))
    );
    assert!(!super::super::assess_import(&imported).executable);
    let retained = super::super::import_csv_document(&path)
        .expect("existing CSV filename retention remains available");
    assert!(
        !retained
            .warnings
            .iter()
            .any(|w| w.starts_with("CSV declaration proof:"))
    );
    assert!(
        super::super::import_filename_document(&path).is_err(),
        "the dedicated EDI/JSON route must not become a CSV route"
    );
    check("<mapping version=\"26\"/>", |a| {
        assert!(matches!(
            a,
            CsvDeclarationAssessment::NotApplicable {
                reason: ApplicabilityReason::NoInlineCsvPair
            }
        ))
    });
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_dir(&root).unwrap();
}

#[test]
fn csv_declaration_proof_suppressed_diagnostic_metadata_cannot_mutate_prior_findings() {
    let mut c = RawCensus::new(PAIR);
    for i in 0..MAX_FINDINGS {
        c.finding(FindingKind::UnknownXml, i..i);
    }
    let last = c.findings.last().unwrap();
    let prior = (
        last.kind,
        last.origin.clone(),
        last.related.clone(),
        last.limit,
    );
    c.stop("synthetic budget", 2, 1, 0);
    let last = c.findings.last().unwrap();
    assert_eq!(
        (
            last.kind,
            last.origin.clone(),
            last.related.clone(),
            last.limit
        ),
        prior
    );
    assert_eq!(c.suppressed_findings, 1);
    assert!(!c.complete);
    let text = grouped().replace(
        "<edge vertexkey=\"80022\"/>",
        "<edge vertexkey=\"80022\"/><edge vertexkey=\"80022\"/>",
    );
    check(&text, |a| match a {
        CsvDeclarationAssessment::Refused { partial } => {
            let f = partial
                .findings
                .iter()
                .find(|f| f.kind == FindingKind::DuplicateEdgeClaim)
                .unwrap();
            assert!(f.ports.contains(&80022));
            assert!(f.owners.contains(&70005));
        }
        _ => panic!("expected typed duplicate refusal"),
    });
}

#[test]
fn csv_declaration_proof_import_bridge_remains_executable_refused_without_preview_io() {
    let root = std::env::temp_dir().join(format!(
        "ferrule-csv-declaration-proof-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("synthetic-grouped.mfd");
    std::fs::write(&path, grouped()).unwrap();
    let result = super::super::import(&path);
    match result {
        Ok(imported) => {
            assert!(
                imported
                    .warnings
                    .iter()
                    .any(|w| w.starts_with("CSV declaration proof:"))
            );
            let report = super::super::assess_import(&imported);
            assert!(!report.executable);
            assert!(
                report
                    .issues
                    .iter()
                    .any(|i| i.kind == super::super::ImportIssueKind::ImportWarning)
            );
        }
        Err(error) => panic!("synthetic repair import should retain its blockers: {error}"),
    }
    assert!(!root.join("examples/unused.dat").exists());
    assert!(
        super::super::import_with_profile(
            &path,
            &super::super::ImportOptions::default(),
            super::super::ImportProfile::Executable
        )
        .is_err()
    );
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_dir(&root).unwrap();
}

#[test]
fn csv_declaration_proof_correction02_only_xml_s_text_is_inert_in_empty_containers() {
    for seed in [PAIR.to_owned(), grouped()] {
        for (empty, tag) in [
            ("<resources/>", "resources"),
            ("<wsdl/>", "wsdl"),
            ("<properties/>", "properties"),
        ] {
            assert!(seed.contains(empty));
            for space in [" ", "\t", "\r", "\n", "&#x20;", "&#x9;", "&#xD;", "&#xA;"] {
                let text = seed.replacen(empty, &format!("<{tag}>{space}</{tag}>"), 1);
                declared(&text, |c, _| {
                    assert!(c.nodes.iter().any(|node| {
                        node.xml.is_text()
                            && node
                                .xml
                                .parent()
                                .is_some_and(|parent| parent.has_tag_name(tag))
                            && node.classification == Classification::Syntax
                    }));
                });
            }
            for (raw, decoded) in [
                ("&#xA0;", "\u{a0}"),
                ("\u{a0}", "\u{a0}"),
                ("&#x2003;", "\u{2003}"),
                ("\u{2003}", "\u{2003}"),
            ] {
                let text = seed.replacen(empty, &format!("<{tag}>{raw}</{tag}>"), 1);
                check(&text, |assessment| {
                    let is_grouped = seed != PAIR;
                    assert_eq!(assessment.blocks_executable(), is_grouped);
                    let CsvDeclarationAssessment::Refused { partial } = assessment else {
                        panic!("non-XML whitespace cannot obtain a declaration witness");
                    };
                    let node = partial
                        .nodes
                        .iter()
                        .find(|node| {
                            node.xml.is_text()
                                && node.xml.text() == Some(decoded)
                                && node
                                    .xml
                                    .parent()
                                    .is_some_and(|parent| parent.has_tag_name(tag))
                        })
                        .expect("decoded text remains in the raw census");
                    assert_eq!(node.classification, Classification::Unknown);
                    assert_eq!(&text[node.xml.range()], raw);
                    assert!(
                        partial
                            .findings
                            .iter()
                            .any(|finding| finding.kind == FindingKind::UnknownXml
                                && finding.origin == node.xml.range())
                    );
                });
            }
        }
    }
}

#[test]
fn csv_declaration_proof_correction02_column_expanded_is_exact_layout_metadata() {
    for seed in [PAIR.to_owned(), grouped()] {
        let mut baseline_columns = Vec::new();
        let mut baseline_obligations = Vec::new();
        declared(&seed, |c, pending| {
            baseline_columns = c
                .endpoints
                .iter()
                .flat_map(|endpoint| {
                    endpoint.columns.iter().map(move |column| {
                        (
                            endpoint.role,
                            column.index,
                            column.name.decoded.clone(),
                            column.port,
                        )
                    })
                })
                .collect();
            baseline_obligations = pending
                .iter()
                .map(|item| (item.kind, item.owner, item.ports.clone()))
                .collect();
        });
        for (old, name, key) in [
            ("<entry name=\"tag\" outkey=\"80012\"/>", "tag", 80012),
            ("<entry name=\"token\" inpkey=\"80022\"/>", "token", 80022),
        ] {
            assert!(seed.contains(old));
            for expanded in [None, Some("0"), Some("1")] {
                let replacement = expanded
                    .map(|value| old.replace("/>", &format!(" expanded=\"{value}\"/>")))
                    .unwrap_or_else(|| old.to_owned());
                let text = seed.replacen(old, &replacement, 1);
                declared(&text, |c, pending| {
                    let columns = c
                        .endpoints
                        .iter()
                        .flat_map(|endpoint| {
                            endpoint.columns.iter().map(move |column| {
                                (
                                    endpoint.role,
                                    column.index,
                                    column.name.decoded.clone(),
                                    column.port,
                                )
                            })
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(columns, baseline_columns);
                    assert_eq!(
                        pending
                            .iter()
                            .map(|item| (item.kind, item.owner, item.ports.clone()))
                            .collect::<Vec<_>>(),
                        baseline_obligations
                    );
                    let port = &c.ports[c.ports_by_key[&key][0]];
                    assert_eq!(port.role, PortRole::Column);
                    let node = &c.nodes[port.node];
                    assert_eq!(node.xml.attribute("name"), Some(name));
                    let attribute = node
                        .attributes
                        .iter()
                        .find(|attribute| attribute.name == "expanded");
                    match (expanded, attribute) {
                        (None, None) => {}
                        (Some(value), Some(attribute)) => {
                            assert_eq!(attribute.classification, Classification::Layout);
                            assert_eq!(attribute.value.decoded, value);
                            assert_eq!(&text[attribute.value.origin.clone()], value);
                            assert!(text[attribute.origin.clone()].starts_with("expanded="));
                        }
                        _ => panic!("missing/present layout metadata changed"),
                    }
                });
            }
            for invalid in ["", "2", "true", "00", " 1"] {
                refused(
                    &seed.replacen(
                        old,
                        &old.replace("/>", &format!(" expanded=\"{invalid}\"/>")),
                        1,
                    ),
                    FindingKind::UnknownControl,
                );
            }
            refused(
                &seed.replacen(old, &old.replace("/>", " extra=\"1\"/>"), 1),
                FindingKind::UnknownControl,
            );
            refused(
                &seed.replacen(
                    old,
                    &old.replace("/>", " xmlns:q=\"urn:synthetic:layout\" q:expanded=\"1\"/>"),
                    1,
                ),
                FindingKind::UnknownControl,
            );
            refused(
                &seed.replacen(
                    old,
                    &old.replace("/>", " expanded=\"1\"><extra/></entry>"),
                    1,
                ),
                FindingKind::UnknownXml,
            );
        }
    }
}

#[test]
fn csv_declaration_proof_correction02_duplicate_role_assignment_examines_one_record() {
    let seed = grouped();
    let mut base_visits = 0;
    declared(&seed, |c, _| {
        base_visits = c.role_record_visits;
        assert_eq!(base_visits, c.ports.len());
    });
    let mut observations = Vec::new();
    for component_count in [15, 30] {
        let components = (0..component_count).map(|ordinal| {
            let inputs = (0..63).map(|position| format!("<datapoint pos=\"{position}\" key=\"91000\"/>"))
                .collect::<String>();
            format!("<component name=\"concat\" library=\"core\" uid=\"{}\" kind=\"5\" growable=\"1\" growablebasename=\"value\"><sources>{inputs}</sources><targets><datapoint pos=\"0\" key=\"{}\"/></targets></component>", 100000 + ordinal, 92000 + ordinal)
        }).collect::<String>();
        let text = with_component(&seed, &components);
        assert!(text.len() < MAX_BYTES);
        check(&text, |assessment| {
            assert!(assessment.blocks_executable());
            let CsvDeclarationAssessment::Refused { partial } = assessment else {
                panic!("ambiguous duplicate inputs must remain refused");
            };
            assert!(partial.ports.len() < MAX_PORTS);
            let duplicates = &partial.ports_by_key[&91000];
            assert_eq!(duplicates.len(), component_count * 63);
            assert!(
                duplicates
                    .iter()
                    .all(|&ordinal| partial.ports[ordinal].role == PortRole::Slot)
            );
            for &ordinal in duplicates {
                let port = &partial.ports[ordinal];
                assert_eq!(partial.port_record_at[&(port.node, "key")], ordinal);
            }
            let finding = partial
                .findings
                .iter()
                .find(|finding| finding.kind == FindingKind::DuplicatePort)
                .expect("complete duplicate-key ledger remains a refusal");
            assert_eq!(finding.related.len(), duplicates.len());
            assert!(
                finding.ports.contains(&91000),
                "finding metadata uses the authored key, not its record ordinal"
            );
            assert!(finding.owners.contains(&100000));
            assert_eq!(partial.role_record_visits, partial.ports.len());
            assert_eq!(
                partial.role_record_visits - base_visits,
                component_count * 64
            );
            assert_eq!(
                partial
                    .functions
                    .iter()
                    .filter(|call| call.kind == CallKind::Concat)
                    .count(),
                component_count + 1
            );
            println!(
                "correction02_role_work components={component_count} duplicate_records={} port_records={} actual_role_record_visits={} base_role_record_visits={base_visits}",
                duplicates.len(),
                partial.ports.len(),
                partial.role_record_visits
            );
            observations.push(partial.role_record_visits - base_visits);
        });
    }
    assert_eq!(observations[1], observations[0] * 2);
}
