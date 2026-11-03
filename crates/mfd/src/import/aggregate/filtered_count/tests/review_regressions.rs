//! Independent whole-value/error controls for the authority review findings.
use super::*;

thread_local! { static DISJOINT: Cell<bool> = const { Cell::new(false) }; static DISJOINT_SEEN: Cell<bool> = const { Cell::new(false) }; }
struct DisjointGuard;
impl Drop for DisjointGuard {
    fn drop(&mut self) {
        DISJOINT.set(false);
    }
}
pub(super) fn audit_disjoint(table: &CountProofTable) {
    if DISJOINT.get() {
        assert!(
            !table.candidate_outputs.contains_key(&51),
            "the ordinary Count acquired new-profile ownership"
        );
        assert!(
            table.candidate_outputs.contains_key(&61),
            "the demanded physical Count lost negative authority"
        );
        DISJOINT_SEEN.set(true);
    }
}

const INPUT: &str = "<Source><Row><Text>keep</Text><Cell><Label>a</Label></Cell><Cell><Label>b</Label></Cell></Row><Row><Text>drop</Text></Row></Source>";
const ORDINARY: &str = "<Results><Total>2</Total><Row><Cell><Label>a</Label></Cell><Cell><Label>b</Label></Cell></Row><Row/></Results>";

pub(super) fn ordinary_design(design: &str) -> String {
    // A root-only ordinary Count has an independent two-row oracle; it does
    // not make a new claim about legacy empty-driver receiver reductions.
    design
        .replace("from=\"42\" to=\"50\"", "from=\"10\" to=\"50\"")
        .replace(r#"<edge from="51" to="111"/>"#, "")
        .replace(r#"<edge from="51" to="122"/>"#, "")
}

pub(super) fn reverse_sections(design: &str) -> String {
    let doc = roxmltree::Document::parse(design).unwrap();
    let mut changes = Vec::new();
    for tag in ["children", "connections"] {
        let node = doc
            .descendants()
            .find(|node| node.has_tag_name(tag))
            .unwrap();
        let mut children = node
            .children()
            .filter(roxmltree::Node::is_element)
            .collect::<Vec<_>>();
        children.reverse();
        let text = children
            .iter()
            .map(|node| &design[node.range()])
            .collect::<String>();
        changes.push((node.range(), format!("<{tag}>{text}</{tag}>")));
    }
    changes.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let mut result = design.to_string();
    for (range, value) in changes {
        result.replace_range(range, &value);
    }
    result
}

pub(super) fn filter_outputs(design: &str, count: usize) -> String {
    let pins = (0..count)
        .map(|i| {
            let key = match i {
                0 => 42,
                1 => 43,
                _ => 10_000 + i,
            };
            format!(r#"<datapoint pos="{i}" key="{key}"/>"#)
        })
        .collect::<String>();
    design.replace(
        r#"<targets><datapoint pos="0" key="42"/><datapoint pos="1" key="43"/></targets>"#,
        &format!("<targets>{pins}</targets>"),
    )
}

pub(super) fn outcome(package: &Package, strict: bool) -> Result<crate::Imported, crate::MfdError> {
    if strict {
        crate::import_with_profile(
            &package.0.join("map.mfd"),
            &crate::ImportOptions::default(),
            crate::ImportProfile::Executable,
        )
        .map(|outcome| outcome.imported)
    } else {
        package.import()
    }
}

pub(super) fn is_count_refusal(result: &Result<crate::Imported, crate::MfdError>) -> bool {
    matches!(result, Err(crate::MfdError::UnsupportedImport(reason)) if reason.contains("physical filtered count"))
}

pub(super) fn whole_ordinary(imported: &crate::Imported, expected: &str) {
    let project = &imported.project;
    let input = format_xml::from_str(INPUT, &project.source).unwrap();
    let output = engine::run(project, &input).unwrap();
    let actual_xml = format_xml::to_string(&project.target, &output).unwrap();
    assert_eq!(
        format_xml::from_str(&actual_xml, &project.target).unwrap(),
        format_xml::from_str(expected, &project.target).unwrap(),
        "{actual_xml}"
    );
}

#[test]
fn review_p1_failed_discovery_cannot_hide_an_indirect_filter_behind_a_source_alias() {
    let mut failures = Vec::new();
    for alias in [false, true] {
        for skipped in [false, true] {
            let mut design = filter_outputs(DESIGN, 65)
                .replace("</children>", r#"<component name="opaque" library="unknown" kind="99"><sources><datapoint key="80"/></sources><targets><datapoint key="81"/></targets></component></children>"#)
                .replace(r#"<edge from="42" to="50"/>"#, r#"<edge from="42" to="80"/><edge from="81" to="50"/>"#);
            if alias {
                design = design.replace(
                    r#"<entry name="Cell"><entry name="Label"/>"#,
                    r#"<entry name="Cell" outkey="81"><entry name="Label"/>"#,
                );
            }
            if skipped {
                design = design.replace(
                    r#"name="count" library="core""#,
                    r#"name="count" library="unknown""#,
                );
            }
            for reverse in [false, true] {
                let package = Package::new(&if reverse {
                    reverse_sections(&design)
                } else {
                    design.clone()
                });
                for limits in [None, Some((0, 0)), Some((1, 1))] {
                    let _guard = LimitsGuard;
                    LIMITS.set(limits);
                    for strict in [false, true] {
                        let result = outcome(&package, strict);
                        if !is_count_refusal(&result) {
                            let detail = match result {
                                Ok(imported) => format!(
                                    "published with {} bare Count nodes",
                                    imported
                                        .project
                                        .graph
                                        .nodes
                                        .values()
                                        .filter(|n| matches!(
                                            n,
                                            Node::Aggregate {
                                                function: AggregateOp::Count,
                                                ..
                                            }
                                        ))
                                        .count()
                                ),
                                Err(error) => format!("{error}"),
                            };
                            failures.push(format!("alias={alias} skipped={skipped} reverse={reverse} limits={limits:?} strict={strict}: {detail}"));
                        }
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn review_p1_extra_raw_aliases_on_registered_components_remain_negative_authority() {
    let design = filter_outputs(DESIGN, 65)
        .replace("</children>", r#"<component name="opaque" library="core" kind="99"><data><root><entry name="Alias" inpkey="80" outkey="81"/></root></data></component></children>"#)
        .replace(r#"<entry name="Cell"><entry name="Label"/>"#, r#"<entry name="Cell" outkey="81"><entry name="Label"/>"#)
        .replace(r#"<edge from="42" to="50"/>"#, r#"<edge from="42" to="80"/><edge from="81" to="50"/>"#);
    let mut failures = Vec::new();
    for reverse in [false, true] {
        let package = Package::new(&if reverse {
            reverse_sections(&design)
        } else {
            design.clone()
        });
        for limits in [None, Some((0, 0))] {
            let _guard = LimitsGuard;
            LIMITS.set(limits);
            for strict in [false, true] {
                if !is_count_refusal(&outcome(&package, strict)) {
                    failures.push(format!(
                        "reverse={reverse} limits={limits:?} strict={strict}"
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "extra raw aliases escaped: {}",
        failures.join("\n")
    );
}

#[test]
fn review_p2_an_unrelated_oversized_filter_never_claims_an_ordinary_count() {
    let ordinary = ordinary_design(DESIGN);
    let mut followers = r#"<component name="constant" library="core" kind="2"><targets><datapoint key="70000"/></targets><data><constant value="true" datatype="boolean"/></data></component>"#.to_string();
    let mut follower_edges = String::new();
    for i in 0..128 {
        followers.push_str(&format!(r#"<component name="constant" library="core" kind="2"><targets><datapoint key="{}"/></targets><data><constant value="{}" datatype="string"/></data></component><component name="logical-not" library="core" kind="5"><sources><datapoint key="{}"/></sources><targets><datapoint key="{}"/></targets></component>"#, 20_000 + i*3, "x".repeat(16_384), 20_001 + i*3, 20_002 + i*3));
        follower_edges.push_str(&format!(r#"<edge from="70000" to="{}"/>"#, 20_001 + i * 3));
    }
    let mut failures = Vec::new();
    for with_followers in [false, true] {
        let design = filter_outputs(&ordinary, 129)
            .replace(
                "</children>",
                &format!(
                    "{}</children>",
                    if with_followers { &followers } else { "" }
                ),
            )
            .replace(
                "</connections>",
                &format!(
                    "{}</connections>",
                    if with_followers { &follower_edges } else { "" }
                ),
            );
        let doc = roxmltree::Document::parse(&design).unwrap();
        let header_elements = doc
            .descendants()
            .find(|n| n.has_tag_name("children"))
            .unwrap()
            .children()
            .filter(|n| n.has_tag_name("component"))
            .map(|owner| {
                owner
                    .descendants()
                    .filter(roxmltree::Node::is_element)
                    .count()
            })
            .sum::<usize>();
        for reverse in [false, true] {
            let package = Package::new(&if reverse {
                reverse_sections(&design)
            } else {
                design.clone()
            });
            for limits in [None, Some((0, 0)), Some((2000, 1))] {
                let _guard = LimitsGuard;
                LIMITS.set(limits);
                for strict in [false, true] {
                    reset();
                    match outcome(&package, strict) {
                        Ok(imported) => whole_ordinary(&imported, ORDINARY),
                        Err(error) => failures.push(format!("followers={with_followers} reverse={reverse} limits={limits:?} strict={strict}: {error}")),
                    }
                    let counters = counts();
                    assert_eq!(counters[Counter::RawBuild as usize], 0);
                    assert_eq!(counters[Counter::FinalBuild as usize], 0);
                    assert_eq!(counters[Counter::RefusalElement as usize], header_elements);
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn review_p2_ordinary_and_filtered_counts_keep_disjoint_authority_with_unrelated_filters() {
    let junk_pins = (0..129)
        .map(|i| format!(r#"<datapoint pos="{i}" key="{}"/>"#, 80_000 + i))
        .collect::<String>();
    let junk = format!(
        r#"<component name="filter" library="core" kind="3"><sources><datapoint key="79000"/><datapoint pos="1" key="79001"/></sources><targets>{junk_pins}</targets></component>"#
    );
    let design = ordinary_design(DESIGN)
        .replace("</children>", &format!(r#"{junk}<component name="count" library="core" kind="5"><sources><datapoint/><datapoint pos="1" key="60"/></sources><targets><datapoint key="61"/></targets></component></children>"#))
        .replace(r#"<entry name="Total" inpkey="100"/>"#, r#"<entry name="Extra" inpkey="101"/><entry name="Total" inpkey="100"/>"#)
        .replace("</connections>", r#"<edge from="42" to="60"/><edge from="61" to="101"/></connections>"#);
    let expected = "<Results><Extra>1</Extra><Total>2</Total><Row><Cell><Label>a</Label></Cell><Cell><Label>b</Label></Cell></Row><Row/></Results>";
    for bad in [false, true] {
        let design = if bad {
            filter_outputs(&design, 65)
        } else {
            design.clone()
        };
        for reverse in [false, true] {
            let package = Package::new(&if reverse {
                reverse_sections(&design)
            } else {
                design.clone()
            });
            let path = package.0.join("row-cell-target.xsd");
            let schema = std::fs::read_to_string(&path).unwrap().replace(r#"<xs:element name="Total" type="xs:integer"/>"#, r#"<xs:element name="Extra" type="xs:integer"/><xs:element name="Total" type="xs:integer"/>"#);
            std::fs::write(path, schema).unwrap();
            for limits in [None, Some((0, 0))] {
                let _limits = LimitsGuard;
                LIMITS.set(limits);
                let _disjoint = DisjointGuard;
                DISJOINT.set(true);
                for strict in [false, true] {
                    DISJOINT_SEEN.set(false);
                    let result = outcome(&package, strict);
                    assert!(DISJOINT_SEEN.get());
                    if bad || limits.is_some() {
                        assert!(is_count_refusal(&result));
                    } else {
                        whole_ordinary(&result.unwrap(), expected);
                    }
                }
            }
        }
    }
}

#[test]
fn review_budget_partitions_and_decoded_metadata_limits_are_exact_and_sticky() {
    let budget = CountConstruction::default();
    let proof = budget.remaining_for_test();
    assert_eq!(proof, (65_536, 32 * 1024 * 1024));
    for text in [
        r#"<datapoint key="00051"/>"#,
        r#"<edge from="00042" to="00050"/>"#,
        r#"<vertex vertexkey="00042"/>"#,
        r#"<entry name="000Text" outkey="00002"/>"#,
    ] {
        let doc = roxmltree::Document::parse(text).unwrap();
        let node = doc.root_element();
        let bytes = node.tag_name().name().len()
            + node
                .attributes()
                .map(|a| a.name().len() + a.value().len())
                .sum::<usize>();
        let exact = CountConstruction::for_test(100, bytes);
        assert!(raw::metadata(node, &exact).is_ok());
        assert_eq!(exact.remaining_for_test().1, 0);
        let short = CountConstruction::for_test(100, bytes - 1);
        assert!(raw::metadata(node, &short).is_err());
        assert!(short.claim(0, 0, 0).is_err());
    }
}

#[test]
fn review_small_padded_numeric_keys_keep_the_original_filtered_oracle() {
    let design = DESIGN.replace(r#"key="51""#, r#"key="00000051""#);
    let expected = "<Results><Total>1</Total><Row><Count>1</Count><Cell><Label>a</Label><Count>1</Count></Cell><Cell><Label>b</Label><Count>1</Count></Cell></Row><Row><Count>0</Count></Row></Results>";
    for reverse in [false, true] {
        let package = Package::new(&if reverse {
            reverse_sections(&design)
        } else {
            design.clone()
        });
        for strict in [false, true] {
            whole_ordinary(&outcome(&package, strict).unwrap(), expected);
        }
    }
}

#[test]
fn review_p3_raw_numeric_text_is_charged_before_parsing() {
    let padded = format!("{}51", "0".repeat(256));
    let design = DESIGN.replace(r#"key="51""#, &format!(r#"key="{padded}""#));
    let doc = roxmltree::Document::parse(&design).unwrap();
    let structure = doc
        .descendants()
        .find(|node| node.has_tag_name("structure"))
        .unwrap();
    let edges = crate::import::graph::read_edge_inputs(&structure, structure.parent().as_ref());
    let budget = CountConstruction::for_test(65_536, 255);
    assert!(
        raw::Raw::build(structure, &edges, &budget).is_err(),
        "raw numeric text bypassed the injected text ledger"
    );
    assert!(budget.claim(0, 0, 0).is_err());
}

#[test]
fn review_p3_a_large_zero_padded_count_key_exhausts_only_a_demanded_proof() {
    let padded = format!("{}51", "0".repeat(33 * 1024 * 1024));
    let large = DESIGN.replace(r#"key="51""#, &format!(r#"key="{padded}""#));
    let mut failures = Vec::new();
    for filtered in [false, true] {
        let package = Package::new(&if filtered {
            large.clone()
        } else {
            ordinary_design(&large)
        });
        for strict in [false, true] {
            let result = outcome(&package, strict);
            if filtered {
                if !is_count_refusal(&result) {
                    failures.push(format!(
                        "filtered strict={strict}: the over-quota Count was not refused"
                    ));
                }
            } else {
                match result {
                    Ok(imported) => whole_ordinary(&imported, ORDINARY),
                    Err(error) => failures.push(format!("ordinary strict={strict}: {error}")),
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn review_p4_repeated_attribute_and_text_flags_do_not_prove_xml_occurrences() {
    let mut failures = Vec::new();
    for (name, role) in [("@Text", ""), ("#text", r#" ferrule-text="1""#)] {
        let design = format!(
            r#"<mapping version="26"><component name="map"><structure><children>
<component name="source" library="xml" kind="14"><data><root><entry name="Source" ferrule-kind="group" ferrule-repeating="0"><entry name="{name}" outkey="2" ferrule-kind="scalar" datatype="string" ferrule-repeating="1"{role}/></entry></root><document instanceroot="{{}}Source"/></data></component>
<component name="constant" library="core" kind="2"><targets><datapoint key="21"/></targets><data><constant value="true" datatype="boolean"/></data></component>
<component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="40"/><datapoint pos="1" key="41"/></sources><targets><datapoint pos="0" key="42"/><datapoint pos="1" key="43"/></targets></component>
<component name="count" library="core" kind="5"><sources><datapoint/><datapoint pos="1" key="50"/></sources><targets><datapoint key="51"/></targets></component>
<component name="count" library="core" kind="5"><sources><datapoint/><datapoint pos="1" key="52"/></sources><targets><datapoint key="53"/></targets></component>
<component name="target" library="json" kind="31"><properties XSLTDefaultOutput="1"/><data><root><entry name="document"><entry name="root"><entry name="object"><entry name="Included" type="json-property"><entry name="number" inpkey="100"/></entry><entry name="Excluded" type="json-property"><entry name="number" inpkey="101"/></entry></entry></entry></entry></root><json schema="role-target.schema.json"/></data></component>
</children><connections><edge from="2" to="40"/><edge from="21" to="41"/><edge from="42" to="50"/><edge from="43" to="52"/><edge from="51" to="100"/><edge from="53" to="101"/></connections></structure></component></mapping>"#
        );
        for notation in ["inline", "alias", "xsd"] {
            if notation == "xsd" && name != "@Text" {
                continue;
            }
            let mut design = design.clone();
            if notation == "alias" {
                let entry = format!(
                    r#"<entry name="{name}" outkey="2" ferrule-kind="scalar" datatype="string" ferrule-repeating="1"{role}/>"#
                );
                design = design.replace(
                    &entry,
                    &format!("{entry}{}", entry.replace(r#"outkey="2""#, r#"outkey="3""#)),
                );
            } else if notation == "xsd" {
                design = design.replace(
                    r#"<document instanceroot="{}Source"/>"#,
                    r#"<document schema="role-source.xsd" instanceroot="{}Source"/>"#,
                );
            }
            for reverse in [false, true] {
                let package = Package::new(&if reverse {
                    reverse_sections(&design)
                } else {
                    design.clone()
                });
                std::fs::write(package.0.join("role-target.schema.json"), r#"{"type":"object","properties":{"Included":{"type":"integer"},"Excluded":{"type":"integer"}}}"#).unwrap();
                std::fs::write(package.0.join("role-source.xsd"), r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:attribute name="Text" type="xs:string"/></xs:complexType></xs:element></xs:schema>"#).unwrap();
                for strict in [false, true] {
                    let result = outcome(&package, strict);
                    if let Ok(imported) = &result {
                        let input =
                            format_xml::from_str("<Source/>", &imported.project.source).unwrap();
                        assert!(matches!(
                            input.field(name.trim_start_matches('@')),
                            Some(ir::Instance::Scalar(_))
                        ));
                        failures.push(format!(
                            "role={name} reverse={reverse} strict={strict}: published {:?}",
                            engine::run(&imported.project, &input).unwrap()
                        ));
                    } else if !is_count_refusal(&result) {
                        failures.push(format!(
                        "role={name} notation={notation} reverse={reverse} strict={strict}: wrong refusal category"
                    ));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn review_p4_only_element_roles_can_supply_repeated_frames_on_the_final_owner_check() {
    for (attribute, text, name) in [
        (true, false, "Value"),
        (false, true, "Value"),
        (false, false, ir::XML_TEXT_FIELD),
        (false, false, ir::XML_ATTRIBUTES_FIELD),
        (false, false, ir::XML_ELEMENTS_FIELD),
    ] {
        let mut field = SchemaNode::scalar(name, ScalarType::String).repeating();
        field.attribute = attribute;
        field.text = text;
        let schema = SchemaNode::group("Source", vec![field]);
        let plan = Plan {
            owner: vec![name.into()],
            suffix: Vec::new(),
            repeated_scalar: true,
            exclude: false,
            predicate: Predicate::Literal(Value::Bool(true)),
        };
        assert!(receiver::root(&schema, &plan, &CountConstruction::default()).is_err());
    }
    for (ty, input) in [
        (
            ScalarType::String,
            "<Source><Value>a</Value><Value>b</Value></Source>",
        ),
        (
            ScalarType::Bool,
            "<Source><Value>true</Value><Value>false</Value></Source>",
        ),
    ] {
        let schema = SchemaNode::group("Source", vec![SchemaNode::scalar("Value", ty).repeating()]);
        let input = format_xml::from_str(input, &schema).unwrap();
        assert_eq!(
            input.field("Value").unwrap().as_repeated().unwrap().len(),
            2
        );
        let plan = Plan {
            owner: vec!["Value".into()],
            suffix: Vec::new(),
            repeated_scalar: true,
            exclude: false,
            predicate: Predicate::Literal(Value::Bool(true)),
        };
        assert!(matches!(
            receiver::root(&schema, &plan, &CountConstruction::default())
                .unwrap()
                .mode,
            Mode::Sum(_)
        ));
    }
}

#[test]
fn review_discovery_failure_is_terminal_even_when_a_previous_candidate_has_an_earlier_receipt() {
    let mut table = CountProofTable::EMPTY;
    table.budget = Some(CountConstruction::default());
    table.discovery_incomplete = true;
    table.failure = Some("unfinished refusal connectivity".into());
    table.candidates.push(Candidate {
        registration: Some(0),
        raw_owner: None,
        outputs: vec![1],
        connected: true,
        plan: Err("not physical".into()),
    });
    let package = Package::new(&ordinary_design(DESIGN));
    let imported = package.import().unwrap();
    let node = *imported.project.graph.nodes.keys().next().unwrap();
    let mut seal = CountSeal::default();
    seal.earlier(0, node);
    assert!(
        table
            .audit_inner(&seal, &imported.project, &[], false)
            .is_err()
    );
}
