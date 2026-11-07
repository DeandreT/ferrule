//! Review-84 real regressions. All actual import statuses precede assertions.
use super::review_regressions::{is_count_refusal, ordinary_design, outcome, reverse_sections};
use super::*;
const INPUT: &str = "<Source><Row><Text>keep</Text><Cell><Label>a</Label></Cell><Cell><Label>b</Label></Cell></Row><Row><Text>drop</Text></Row></Source>";
const WHOLE: &str = "<Results><Total>2</Total><Row><Cell><Label>a</Label></Cell><Cell><Label>b</Label></Cell></Row><Row/></Results>";
fn without_filter(design: &str) -> String {
    let doc = roxmltree::Document::parse(design).unwrap();
    let range = doc
        .descendants()
        .find(|n| n.has_tag_name("component") && n.attribute("name") == Some("filter"))
        .unwrap()
        .range();
    let mut text = design.to_string();
    text.replace_range(range, "");
    text.replace(r#"<edge from="11" to="40"/>"#, "")
        .replace(r#"<edge from="32" to="41"/>"#, "")
}
pub(super) fn parent_connections(design: &str, noise: usize) -> String {
    let doc = roxmltree::Document::parse(design).unwrap();
    let range = doc
        .descendants()
        .find(|n| n.has_tag_name("connections"))
        .unwrap()
        .range();
    let mut connections = design[range.clone()].to_string();
    let extra = (0..noise)
        .map(|i| format!(r#"<edge from="10" to="{}"/>"#, 200_000 + i))
        .collect::<String>();
    connections = connections.replace("</connections>", &format!("{extra}</connections>"));
    let mut text = design.to_string();
    text.replace_range(range, "");
    text.replace("</structure>", &format!("</structure>{connections}"))
}
fn preserve(label: &str, package: &Package) {
    if let Some(root) = std::env::var_os("FERRULE_COUNT_REVIEW84_EVIDENCE") {
        let path = PathBuf::from(root).join(label);
        std::fs::create_dir(&path).unwrap();
        for name in ["map.mfd", "row-cell-source.xsd", "row-cell-target.xsd"] {
            std::fs::copy(package.0.join(name), path.join(name)).unwrap();
        }
        std::fs::write(path.join("input.xml"), INPUT).unwrap();
        std::fs::write(path.join("expected.xml"), WHOLE).unwrap();
    }
}
fn ordinary(label: &str, package: &Package, strict: bool) -> bool {
    reset();
    let result = outcome(package, strict);
    match result {
        Ok(imported) => {
            let input = format_xml::from_str(INPUT, &imported.project.source).unwrap();
            let output = engine::run(&imported.project, &input).unwrap();
            let actual = format_xml::to_string(&imported.project.target, &output).unwrap();
            let okay = format_xml::from_str(&actual, &imported.project.target).unwrap()
                == format_xml::from_str(WHOLE, &imported.project.target).unwrap()
                && imported.warnings.is_empty();
            eprintln!(
                "REVIEW84 {label} strict={strict} actual={actual:?} warnings={:?} counters={:?}",
                imported.warnings,
                counts()
            );
            okay
        }
        Err(error) => {
            eprintln!(
                "REVIEW84 {label} strict={strict} actual={error:?} counters={:?}",
                counts()
            );
            false
        }
    }
}
#[test]
fn review84_parent_connection_dimensions_must_preserve_ordinary_counts() {
    let mut failures = Vec::new();
    for noise in [0, 4096] {
        for filter in [false, true] {
            let base = ordinary_design(DESIGN);
            let base = if filter { base } else { without_filter(&base) };
            let design = parent_connections(&base, noise);
            for reverse in [false, true] {
                let design = if reverse {
                    reverse_sections(&design)
                } else {
                    design.clone()
                };
                let label = format!("parent-noise-{noise}-filter-{filter}-reverse-{reverse}");
                let package = Package::new(&design);
                preserve(&label, &package);
                for limits in [None, Some((0, 0)), Some((12, 1024))] {
                    let _guard = LimitsGuard;
                    LIMITS.set(limits);
                    for strict in [false, true] {
                        if !ordinary(&label, &package, strict) {
                            failures.push(format!("{label} limits={limits:?} strict={strict}"));
                        } else {
                            assert_eq!(counts()[Counter::RawBuild as usize], 0);
                            assert_eq!(counts()[Counter::FinalBuild as usize], 0);
                            assert_eq!(counts()[Counter::Origin as usize], 0);
                            let report = routing::report();
                            assert!(report.dimensions.edges >= noise);
                            assert!(
                                report.slots <= report.limits.1 && report.bytes <= report.limits.2
                            );
                        }
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "ordinary Count changed: {}",
        failures.join("\n")
    );
}
#[test]
fn review84_later_wrapper_connections_and_graphs_must_not_hide_a_filter() {
    let mut failures = Vec::new();
    for (form, extra) in [
        (
            "connections",
            r#"<connections/><connections><edge from="42" to="50"/></connections>"#,
        ),
        (
            "graph",
            r#"<connections/><graph><vertices><vertex vertexkey="42"><edges><edge vertexkey="50"/></edges></vertex></vertices></graph>"#,
        ),
    ] {
        let design =
            ordinary_design(DESIGN).replace("</structure>", &format!("</structure>{extra}"));
        for reverse in [false, true] {
            let design = if reverse {
                reverse_sections(&design)
            } else {
                design.clone()
            };
            let label = format!("later-{form}-reverse-{reverse}");
            let package = Package::new(&design);
            preserve(&label, &package);
            for limits in [None, Some((0, 0)), Some((12, 1024))] {
                let _guard = LimitsGuard;
                LIMITS.set(limits);
                for strict in [false, true] {
                    reset();
                    let result = outcome(&package, strict);
                    let detail = match &result {
                        Ok(imported) => format!(
                            "published: {:?}",
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
                        Err(error) => format!("{error:?}"),
                    };
                    eprintln!("REVIEW84 {label} limits={limits:?} strict={strict}: {detail}");
                    if !is_count_refusal(&result) {
                        failures.push(format!(
                            "{label} limits={limits:?} strict={strict}: {detail}"
                        ));
                    }
                    capture_boundary(limits);
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "alternate wrapper authority escaped: {}",
        failures.join("\n")
    );
}
#[test]
fn review84_known_destination_uncertainty_must_not_taint_disjoint_counts() {
    let mut failures = Vec::new();
    for filter in [false, true] {
        let base = ordinary_design(DESIGN);
        let base = if filter { base } else { without_filter(&base) };
        let design = base.replace(
            "</connections>",
            r#"<edge from="bogus" to="90000"/></connections>"#,
        );
        for reverse in [false, true] {
            let design = if reverse {
                reverse_sections(&design)
            } else {
                design.clone()
            };
            let label = format!("disjoint-bad-source-filter-{filter}-reverse-{reverse}");
            let package = Package::new(&design);
            preserve(&label, &package);
            for limits in [None, Some((0, 0)), Some((12, 1024))] {
                let _guard = LimitsGuard;
                LIMITS.set(limits);
                for strict in [false, true] {
                    if !ordinary(&label, &package, strict) {
                        failures.push(format!("{label} limits={limits:?} strict={strict}"));
                    } else {
                        assert_eq!(counts()[Counter::RawBuild as usize], 0);
                        assert_eq!(counts()[Counter::FinalBuild as usize], 0);
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "attributable disjoint uncertainty became global: {}",
        failures.join("\n")
    );
}

fn refuse_all(label: &str, design: &str) {
    for reverse in [false, true] {
        let design = if reverse {
            reverse_sections(design)
        } else {
            design.to_string()
        };
        let package = Package::new(&design);
        preserve(&format!("{label}-reverse-{reverse}"), &package);
        for limits in [None, Some((0, 0)), Some((12, 1024))] {
            let _guard = LimitsGuard;
            LIMITS.set(limits);
            for strict in [false, true] {
                reset();
                let result = outcome(&package, strict);
                eprintln!(
                    "REVIEW84 {label} reverse={reverse} limits={limits:?} strict={strict} error={:?} counters={:?}",
                    result.as_ref().err(),
                    counts()
                );
                assert!(
                    is_count_refusal(&result),
                    "{label}: {:?}",
                    result.as_ref().err()
                );
                assert_eq!(counts()[Counter::RawBuild as usize], 1);
                assert_eq!(counts()[Counter::FinalBuild as usize], 1);
                capture_boundary(limits);
            }
        }
    }
}

fn capture_boundary(limits: Option<(usize, usize)>) {
    match limits {
        Some((0, 0)) => assert_eq!(counts()[Counter::Origin as usize], 0),
        Some((12, 1024)) => assert!(counts()[Counter::Origin as usize] > 0),
        _ => {}
    }
}

#[test]
fn nested_parent_graph_claim_multiplicity_is_sized_before_retention() {
    let mut graph = String::from("<connections/><graph><vertices>");
    for i in 0..256 {
        graph.push_str(&format!(r#"<vertex vertexkey="{}">"#, 300_000 + i));
    }
    graph.push_str("<edges>");
    for i in 0..256 {
        graph.push_str(&format!(r#"<edge vertexkey="{}"/>"#, 400_000 + i));
    }
    graph.push_str("</edges>");
    for _ in 0..256 {
        graph.push_str("</vertex>");
    }
    graph.push_str("</vertices></graph>");
    for filter in [false, true] {
        let base = ordinary_design(DESIGN);
        let base = if filter { base } else { without_filter(&base) };
        let design = base.replace("</structure>", &format!("</structure>{graph}"));
        let package = Package::new(&design);
        let label = format!("nested-parent-graph-filter-{filter}");
        preserve(&label, &package);
        let _guard = LimitsGuard;
        LIMITS.set(Some((0, 0)));
        for strict in [false, true] {
            assert!(ordinary(&label, &package, strict));
            let report = routing::report();
            assert!(report.dimensions.edges >= 256 * 256);
            assert!(report.slots <= report.limits.1 && report.bytes <= report.limits.2);
            assert_eq!(report.retained_graph_slots, 0);
            assert_eq!(counts()[Counter::Origin as usize], 0);
            assert_eq!(counts()[Counter::RawBuild as usize], 0);
            assert_eq!(counts()[Counter::FinalBuild as usize], 0);
            eprintln!("REVIEW84 {label} report={report:?}");
        }
    }
}

#[test]
fn closed_numeric_missing_destination_is_ignored_but_unbounded_records_refuse() {
    for extra in [
        r#"<edge from="bogus" to="not-numeric"/>"#,
        r#"<edge from="42"/>"#,
    ] {
        let design =
            ordinary_design(DESIGN).replace("</connections>", &format!("{extra}</connections>"));
        let package = Package::new(&design);
        let _guard = LimitsGuard;
        LIMITS.set(Some((0, 0)));
        for strict in [false, true] {
            assert!(ordinary("closed-missing-destination", &package, strict));
            assert_eq!(counts()[Counter::RawBuild as usize], 0);
        }
    }
    let design = ordinary_design(DESIGN).replace(
        "</structure>",
        r#"</structure><connections/><connections><unknown destination="50"/></connections>"#,
    );
    refuse_all("unbounded-record", &design);
}

#[test]
fn malformed_graph_source_is_destination_local_on_structure_and_wrapper() {
    for parent in [false, true] {
        for connected in [false, true] {
            let mut base = ordinary_design(DESIGN);
            if connected {
                base=base.replace("</children>",r#"<component name="opaque" library="unknown" kind="99"><sources><datapoint key="90000"/></sources><targets><datapoint key="81"/></targets></component></children>"#);
            }
            let graph = format!(
                r#"<graph><vertices><vertex vertexkey="bogus"><edges><edge vertexkey="90000"/></edges></vertex>{}</vertices></graph>"#,
                if connected {
                    r#"<vertex vertexkey="81"><edges><edge vertexkey="50"/></edges></vertex>"#
                } else {
                    ""
                }
            );
            let design = if parent {
                base.replace(
                    "</structure>",
                    &format!("</structure><connections/>{graph}"),
                )
            } else {
                base.replace("</structure>", &format!("{graph}</structure>"))
            };
            let label = format!("malformed-graph-parent-{parent}-connected-{connected}");
            if connected {
                refuse_all(&label, &design);
                continue;
            }
            for reverse in [false, true] {
                let design = if reverse {
                    reverse_sections(&design)
                } else {
                    design.clone()
                };
                let package = Package::new(&design);
                preserve(&format!("{label}-reverse-{reverse}"), &package);
                for limits in [None, Some((0, 0)), Some((12, 1024))] {
                    let _guard = LimitsGuard;
                    LIMITS.set(limits);
                    for strict in [false, true] {
                        assert!(ordinary(&label, &package, strict));
                        assert_eq!(counts()[Counter::RawBuild as usize], 0);
                        assert_eq!(counts()[Counter::FinalBuild as usize], 0);
                        assert_eq!(counts()[Counter::Origin as usize], 0);
                    }
                }
            }
        }
    }
}

#[test]
fn wrapper_alternates_keep_opaque_registered_and_source_alias_authority() {
    for registered in [false, true] {
        let opaque = if registered {
            r#"<component name="opaque" library="core" kind="99"><data><root><entry name="Alias" inpkey="80" outkey="81"/></root></data></component>"#
        } else {
            r#"<component name="opaque" library="unknown" kind="99"><sources><datapoint key="80"/></sources><targets><datapoint key="81"/></targets></component>"#
        };
        for source_alias in [false, true] {
            let mut base =
                ordinary_design(DESIGN).replace("</children>", &format!("{opaque}</children>"));
            if source_alias {
                base = base.replace(
                    r#"<entry name="Cell"><entry name="Label"/>"#,
                    r#"<entry name="Cell" outkey="81"><entry name="Label"/>"#,
                );
            }
            for graph in [false, true] {
                let extra = if graph {
                    r#"<connections/><graph><vertices><vertex vertexkey="42"><edges><edge vertexkey="80"/></edges></vertex><vertex vertexkey="81"><edges><edge vertexkey="50"/></edges></vertex></vertices></graph>"#
                } else {
                    r#"<connections/><connections><edge from="42" to="80"/><edge from="81" to="50"/></connections>"#
                };
                let design = base.replace("</structure>", &format!("</structure>{extra}"));
                refuse_all(
                    &format!(
                        "alternate-registered-{registered}-alias-{source_alias}-graph-{graph}"
                    ),
                    &design,
                );
            }
        }
    }
}

#[test]
fn known_destination_uncertainty_reaches_count_through_an_opaque_alias() {
    for parent in [false, true] {
        let base = ordinary_design(DESIGN)
            .replace("</children>", r#"<component name="opaque" library="unknown" kind="99"><sources><datapoint key="90000"/></sources><targets><datapoint key="81"/></targets></component></children>"#)
            .replace("</connections>", r#"<edge from="bogus" to="90000"/><edge from="81" to="50"/></connections>"#);
        let design = if parent {
            parent_connections(&base, 0)
        } else {
            base
        };
        refuse_all(&format!("local-uncertainty-parent-{parent}"), &design);
    }
    let nested = ordinary_design(DESIGN).replace("</structure>", r#"</structure><connections/><connections><edge from="10" to="90000"><edge from="42" to="50"/></edge></connections>"#);
    refuse_all("nested-malformed-record", &nested);
}

#[test]
fn parent_numeric_padding_and_namespace_metadata_are_sized_before_negative_use() {
    let padding = "0".repeat(100_000);
    let namespace = format!("urn:{}", "n".repeat(100_000));
    for filter in [false, true] {
        let base = ordinary_design(DESIGN);
        let base = if filter { base } else { without_filter(&base) };
        let design = parent_connections(&base, 0).replace("</connections>", &format!(r#"<edge xmlns:x="{namespace}" x:marker="v" from="{padding}10" to="{padding}90000"/></connections>"#));
        for reverse in [false, true] {
            let design = if reverse {
                reverse_sections(&design)
            } else {
                design.clone()
            };
            let package = Package::new(&design);
            let label = format!("parent-padding-filter-{filter}-reverse-{reverse}");
            preserve(&label, &package);
            for limits in [Some((0, 0)), Some((12, 1024))] {
                let _guard = LimitsGuard;
                LIMITS.set(limits);
                for strict in [false, true] {
                    assert!(ordinary(&label, &package, strict));
                    let report = routing::report();
                    assert!(report.dimensions.decoded >= 300_000);
                    assert!(report.text_work >= 300_000);
                    assert!(report.work <= report.limits.0);
                    assert_eq!(counts()[Counter::Origin as usize], 0);
                    assert_eq!(counts()[Counter::RawBuild as usize], 0);
                    assert_eq!(counts()[Counter::FinalBuild as usize], 0);
                    eprintln!("REVIEW84 {label} routing={report:?}");
                }
            }
        }
    }
}

#[test]
fn no_count_parent_edges_keep_exact_feeds_conflicts_and_single_reader_index() {
    for conflict in [false, true] {
        let base = without_filter(&ordinary_design(DESIGN));
        let doc = roxmltree::Document::parse(&base).unwrap();
        let range = doc
            .descendants()
            .find(|n| n.has_tag_name("component") && n.attribute("name") == Some("count"))
            .unwrap()
            .range();
        let mut base = base.clone();
        base.replace_range(range, r#"<component name="constant" library="core" kind="2"><targets><datapoint key="51"/></targets><data><constant value="2" datatype="integer"/></data></component>"#);
        base = base.replace(r#"<edge from="10" to="50"/>"#, "");
        let base = parent_connections(&base, 4096).replace(
            "</connections>",
            &format!(
                r#"<edge from="51" to="100"/>{}</connections>"#,
                if conflict {
                    r#"<edge from="11" to="200000"/>"#
                } else {
                    ""
                }
            ),
        );
        for reverse in [false, true] {
            let design = if reverse {
                reverse_sections(&base)
            } else {
                base.clone()
            };
            let doc = roxmltree::Document::parse(&design).unwrap();
            let structure = doc
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
            let mut expected = BTreeMap::from([
                (30, 11),
                (31, 20),
                (100, 51),
                (110, 10),
                (120, 12),
                (121, 13),
            ]);
            for i in 0..4096 {
                expected.insert(200_000 + i, 10);
            }
            if conflict {
                expected.remove(&200_000);
            }
            assert_eq!(edges.feeds, expected);
            assert_eq!(edges.complete, !conflict);
            assert_eq!(
                edges.conflicts,
                if conflict {
                    vec!["input port 200000 has conflicting source feeds [10, 11]; all feeds to this input were skipped".to_string()]
                } else {
                    Vec::new()
                }
            );
            assert!(negative.unwrap().is_none());
            assert_eq!(counts()[Counter::EdgeIndex as usize], 1);
            assert_eq!(counts()[Counter::NegativeBuild as usize], 0);
            assert_eq!(routing::report().slots, 0);
            let package = Package::new(&design);
            preserve(
                &format!("no-count-parent-conflict-{conflict}-reverse-{reverse}"),
                &package,
            );
            if !conflict {
                for strict in [false, true] {
                    assert!(ordinary("no-count-parent", &package, strict));
                    assert_eq!(counts()[Counter::EdgeIndex as usize], 2);
                    assert_eq!(counts()[Counter::Origin as usize], 0);
                    assert_eq!(routing::report().slots, 0);
                }
            }
        }
    }
}

#[test]
fn ten_thousand_demanded_consumers_refuse_without_renewing_positive_origin_capacity() {
    let mut consumers = String::new();
    let mut wires = String::new();
    for i in 0..10_000 {
        consumers.push_str(&format!(r#"<component name="logical-not" library="core" kind="5"><sources><datapoint key="{}"/></sources><targets><datapoint key="{}"/></targets></component>"#, 200_000+2*i, 200_001+2*i));
        wires.push_str(&format!(r#"<edge from="51" to="{}"/>"#, 200_000 + 2 * i));
    }
    let design = DESIGN
        .replace("</children>", &format!("{consumers}</children>"))
        .replace("</connections>", &format!("{wires}</connections>"));
    let package = Package::new(&design);
    preserve("ten-thousand-consumers", &package);
    for strict in [false, true] {
        reset();
        let result = outcome(&package, strict);
        eprintln!(
            "REVIEW84 10000 consumers strict={strict} error={:?} counters={:?} routing={:?}",
            result.as_ref().err(),
            counts(),
            routing::report()
        );
        assert!(is_count_refusal(&result));
        assert!(counts()[Counter::QuotaFailure as usize] > 0);
        // Geometric capture itself can fit; the same remaining ledger must
        // still admit the raw/final proof, with no new per-phase allowance.
        assert!(counts()[Counter::Origin as usize] <= 10_016);
        assert_eq!(routing::report().retained_graph_slots, 0);
    }
}
