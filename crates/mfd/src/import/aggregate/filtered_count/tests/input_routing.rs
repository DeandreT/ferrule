//! Green reconciliation of implementation-03's two real reds, plus paired
//! authority adversaries. Original failed-source/receipts remain immutable.
use super::review_regressions::{
    filter_outputs, is_count_refusal, ordinary_design, outcome, reverse_sections, whole_ordinary,
};
use super::*;

const WHOLE: &str = "<Results><Total>2</Total><Row><Cell><Label>a</Label></Cell><Cell><Label>b</Label></Cell></Row><Row/></Results>";

fn remove_filter(design: &str) -> String {
    let document = roxmltree::Document::parse(design).unwrap();
    let range = document
        .descendants()
        .find(|n| n.has_tag_name("component") && n.attribute("name") == Some("filter"))
        .unwrap()
        .range();
    let mut design = design.to_string();
    design.replace_range(range, "");
    design
        .replace(r#"<edge from="11" to="40"/>"#, "")
        .replace(r#"<edge from="32" to="41"/>"#, "")
}
fn noise(design: &str, count: usize, literal: &str) -> String {
    let constants = (0..count).map(|i| format!(r#"<component name="constant" library="core" kind="2"><targets><datapoint key="{}"/></targets><data><constant value="{literal}" datatype="string"/></data></component>"#, 100_000 + i)).collect::<String>();
    design.replace("</children>", &format!("{constants}</children>"))
}
fn report_bounds() {
    let report = routing::report();
    eprintln!("input-derived negative routing: {report:?}");
    assert!(report.header_work > 0);
    assert!(report.work <= report.limits.0);
    assert!(report.text_work <= report.work);
    assert!(report.slots <= report.limits.1 && report.bytes <= report.limits.2);
    assert!(report.frozen);
    assert_eq!(report.retained_graph_slots, 0);
}

#[test]
fn ordinary_count_with_1024_and_4096_unused_constants_is_independent_of_positive_capture() {
    let ordinary = ordinary_design(DESIGN);
    for unrelated in [0, 16, 1024, 4096] {
        for disjoint_filter in [false, true] {
            let base = if disjoint_filter {
                filter_outputs(&ordinary, 129)
            } else {
                remove_filter(&ordinary)
            };
            let base = noise(&base, unrelated, "unused");
            for reverse in [false, true] {
                let package = Package::new(&if reverse {
                    reverse_sections(&base)
                } else {
                    base.clone()
                });
                for limits in [None, Some((0, 0)), Some((2, 1))] {
                    let _guard = LimitsGuard;
                    LIMITS.set(limits);
                    for strict in [false, true] {
                        reset();
                        let result = outcome(&package, strict);
                        if let Err(error) = &result {
                            eprintln!(
                                "noise={unrelated} filter={disjoint_filter} reverse={reverse} limits={limits:?} strict={strict}: {error}"
                            );
                        }
                        whole_ordinary(&result.unwrap(), WHOLE);
                        let c = counts();
                        assert_eq!(c[Counter::RawBuild as usize], 0);
                        assert_eq!(c[Counter::FinalBuild as usize], 0);
                        assert_eq!(
                            c[Counter::Origin as usize],
                            0,
                            "unrelated registrations must not get positive origin slots"
                        );
                        assert_eq!(c[Counter::QuotaFailure as usize], 0);
                        report_bounds();
                    }
                }
            }
        }
    }
}

#[test]
fn large_unrelated_literal_metadata_does_not_consume_positive_capture() {
    let base = noise(
        &filter_outputs(&ordinary_design(DESIGN), 129),
        1,
        &"x".repeat(2 * 1024 * 1024),
    );
    let package = Package::new(&base);
    for strict in [false, true] {
        let _guard = LimitsGuard;
        LIMITS.set(Some((0, 0)));
        reset();
        whole_ordinary(&outcome(&package, strict).unwrap(), WHOLE);
        assert_eq!(counts()[Counter::Origin as usize], 0);
        let report = routing::report();
        assert!(report.dimensions.decoded >= 2 * 1024 * 1024);
        // Literal bodies are sized, not copied into negative buffers.
        assert!(report.bytes < 2 * 1024 * 1024);
        report_bounds();
    }
}

fn no_count(edges: usize, conflict: bool) -> (String, BTreeMap<u32, u32>) {
    let mut design = remove_filter(&ordinary_design(DESIGN));
    let document = roxmltree::Document::parse(&design).unwrap();
    let range = document
        .descendants()
        .find(|n| n.has_tag_name("component") && n.attribute("name") == Some("count"))
        .unwrap()
        .range();
    design.replace_range(range, r#"<component name="constant" library="core" kind="2"><targets><datapoint key="51"/></targets><data><constant value="2" datatype="integer"/></data></component>"#);
    design = design.replace(r#"<edge from="10" to="50"/>"#, "");
    let mut expected = BTreeMap::from([
        (30, 11),
        (31, 20),
        (100, 51),
        (110, 10),
        (120, 12),
        (121, 13),
    ]);
    let mut extra = r#"<edge from="51" to="100"/>"#.to_string();
    for i in 0..edges {
        extra.push_str(&format!(
            r#"<edge from="{}" to="{}"/>"#,
            200_000 + i,
            300_000 + i
        ));
        expected.insert(300_000 + i as u32, 200_000 + i as u32);
    }
    if conflict {
        extra.push_str(r#"<edge from="999999" to="300000"/>"#);
        expected.remove(&300_000);
    }
    (
        design.replace("</connections>", &format!("{extra}</connections>")),
        expected,
    )
}

#[test]
fn no_count_retains_zero_additional_claim_slots_and_preserves_the_existing_edge_reader() {
    let mut full_import_index_calls = None;
    for (edge_count, conflict) in [(0, false), (1024, false), (1024, true)] {
        let (design, expected) = no_count(edge_count, conflict);
        let document = crate::design::parse(&design).unwrap();
        let structure = document
            .descendants()
            .find(|n| n.has_tag_name("structure"))
            .unwrap();
        let header = routing::Headers::read(structure).unwrap();
        reset();
        let (edges, negative) = crate::import::graph::read_edge_inputs_with_probe(
            &structure,
            structure.parent().as_ref(),
            |claims| routing::Routing::prepare(header, structure, claims),
        );
        assert!(negative.unwrap().is_none());
        // Exhaustive pattern: the ordinary result cannot hide an extra claims
        // field. The borrowed map's lifetime ends inside this single call.
        let EdgeInputs {
            feeds,
            conflicts,
            complete,
        } = edges;
        assert_eq!(feeds, expected);
        assert_eq!(complete, !conflict);
        assert_eq!(
            conflicts,
            if conflict {
                vec!["input port 300000 has conflicting source feeds [200000, 999999]; all feeds to this input were skipped".to_string()]
            } else {
                Vec::new()
            }
        );
        assert_eq!(counts()[Counter::EdgeIndex as usize], 1);
        assert_eq!(counts()[Counter::NegativeBuild as usize], 0);
        assert_eq!(routing::report().slots, 0);
        assert_eq!(routing::report().bytes, 0);
        assert_eq!(routing::report().retained_graph_slots, 0);
        if !conflict {
            let package = Package::new(&design);
            for strict in [false, true] {
                reset();
                whole_ordinary(&outcome(&package, strict).unwrap(), WHOLE);
                let c = counts();
                assert_eq!(c[Counter::Origin as usize], 0);
                assert_eq!(c[Counter::RawBuild as usize], 0);
                assert_eq!(c[Counter::FinalBuild as usize], 0);
                assert_eq!(routing::report().slots, 0);
                if let Some(expected) = full_import_index_calls {
                    assert_eq!(c[Counter::EdgeIndex as usize], expected);
                } else {
                    full_import_index_calls = Some(c[Counter::EdgeIndex as usize]);
                }
            }
        }
    }
}

#[test]
fn complete_negative_union_retains_the_decisive_opaque_alias_first_or_last_after_large_noise() {
    for registered in [false, true] {
        let extra = if registered {
            r#"<component name="opaque" library="core" kind="99"><data><root><entry name="Alias" inpkey="80" outkey="81"/></root></data></component>"#
        } else {
            r#"<component name="opaque" library="unknown" kind="99"><sources><datapoint key="80"/></sources><targets><datapoint key="81"/></targets></component>"#
        };
        let design = filter_outputs(DESIGN, 65)
            .replace("</children>", &format!("{extra}</children>"))
            .replace(
                r#"<entry name="Cell"><entry name="Label"/>"#,
                r#"<entry name="Cell" outkey="81"><entry name="Label"/>"#,
            )
            .replace(
                r#"<edge from="42" to="50"/>"#,
                r#"<edge from="42" to="80"/><edge from="81" to="50"/>"#,
            )
            .replace(
                r#"outkey="81""#,
                &format!(r#"outkey="{}81""#, "0".repeat(256)),
            )
            .replace(r#"key="81""#, &format!(r#"key="{}81""#, "0".repeat(256)));
        let design = noise(&design, 1024, "unused");
        for reverse in [false, true] {
            let package = Package::new(&if reverse {
                reverse_sections(&design)
            } else {
                design.clone()
            });
            for limits in [None, Some((0, 0)), Some((2, 1))] {
                let _guard = LimitsGuard;
                LIMITS.set(limits);
                for strict in [false, true] {
                    reset();
                    let result = outcome(&package, strict);
                    assert!(
                        is_count_refusal(&result),
                        "registered={registered} reverse={reverse} limits={limits:?} strict={strict}"
                    );
                    assert_eq!(counts()[Counter::RawBuild as usize], 1);
                    assert_eq!(counts()[Counter::FinalBuild as usize], 1);
                    report_bounds();
                }
            }
        }
    }
}

#[test]
fn actual_late_promoted_catalog_collision_and_opposite_raw_claim_refuse_in_both_profiles() {
    const CATALOG: &str = r#"<component name="UnusedLookup" library="local"><structure><children>
<component name="needle" library="core" kind="6" uid="901"><targets><datapoint key="501"/></targets><data><input datatype="string"/></data></component>
<component name="result" library="core" kind="7" uid="902"><sources><datapoint key="500"/></sources><data><output datatype="string"/></data></component>
<component name="equal" library="core" kind="5" uid="903"><sources><datapoint pos="0" key="502"/><datapoint pos="1" key="503"/></sources><targets><datapoint key="504"/></targets></component>
<component name="Catalog" library="xml" kind="14" uid="904"><properties ParameterName="catalog"/><data><root><entry name="Catalog"><entry name="Item"><entry name="Key" outkey="11"/><entry name="Value" outkey="509"/></entry></entry></root><document schema="catalog.xsd" inputinstance="not-loaded.xml" instanceroot="{}Catalog"/></data></component>
<component name="filter" library="core" kind="3" uid="905"><sources><datapoint pos="0" key="505"/><datapoint pos="1" key="506"/></sources><targets><datapoint pos="0" key="507"/><datapoint pos="1"/></targets></component>
</children><graph><vertices><vertex vertexkey="501"><edges><edge vertexkey="502"/></edges></vertex><vertex vertexkey="11"><edges><edge vertexkey="503"/></edges></vertex><vertex vertexkey="504"><edges><edge vertexkey="506"/></edges></vertex><vertex vertexkey="509"><edges><edge vertexkey="505"/></edges></vertex><vertex vertexkey="507"><edges><edge vertexkey="500"/></edges></vertex></vertices></graph></structure></component>"#;
    const XSD: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Catalog"><xs:complexType><xs:sequence><xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Key" type="xs:string"/><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#;
    for reverse in [false, true] {
        let control = no_count(0, false)
            .0
            .replace("</mapping>", &format!("{CATALOG}</mapping>"));
        let control = Package::new(&if reverse {
            reverse_sections(&control)
        } else {
            control
        });
        std::fs::write(control.0.join("catalog.xsd"), XSD).unwrap();
        for strict in [false, true] {
            let imported = outcome(&control, strict).unwrap();
            let catalog = imported
                .project
                .extra_sources
                .iter()
                .find(|source| source.schema.name == "Catalog")
                .expect("the control must actually promote its local catalog");
            assert!(imported.project.execution_contract.is_none());
            assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
            let input = format_xml::from_str("<Source><Row><Text>keep</Text><Cell><Label>a</Label></Cell><Cell><Label>b</Label></Cell></Row><Row><Text>drop</Text></Row></Source>", &imported.project.source).unwrap();
            let secondary = format_xml::from_str(
                "<Catalog><Item><Key>unused</Key><Value>unused</Value></Item></Catalog>",
                &catalog.schema,
            )
            .unwrap();
            let output = engine::run_with_sources(
                &imported.project,
                &input,
                vec![(catalog.name.clone(), secondary)],
            )
            .unwrap();
            let actual = format_xml::to_string(&imported.project.target, &output).unwrap();
            assert_eq!(
                format_xml::from_str(&actual, &imported.project.target).unwrap(),
                format_xml::from_str(WHOLE, &imported.project.target).unwrap()
            );
        }
        for opposite in [false, true] {
            let design = if opposite {
                DESIGN.replace("</children>",r#"<component name="hidden" library="core" kind="99"><sources><datapoint key="11"/></sources></component></children>"#)
            } else {
                DESIGN.replace("</mapping>", &format!("{CATALOG}</mapping>"))
            };
            let package = Package::new(&if reverse {
                reverse_sections(&design)
            } else {
                design
            });
            std::fs::write(package.0.join("catalog.xsd"), XSD).unwrap();
            for limits in [None, Some((0, 0)), Some((2, 1))] {
                let _guard = LimitsGuard;
                LIMITS.set(limits);
                for strict in [false, true] {
                    let result = outcome(&package, strict);
                    if let Err(error) = &result {
                        eprintln!("late catalog/opposite={opposite} strict={strict}: {error}");
                    }
                    assert!(is_count_refusal(&result));
                }
            }
        }
    }
}
