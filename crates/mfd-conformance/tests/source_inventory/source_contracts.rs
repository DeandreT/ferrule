//! The original companion has its own coverage graph, independent of the
//! supplemental inventories. Check it against the canonical ID ledger and
//! explicit page/placement expectations, without accessing vendor artifacts.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

#[path = "source_contracts/command_values.rs"]
mod command_values;
#[path = "source_contracts/regressions.rs"]
mod regressions;
#[path = "source_contracts/relations.rs"]
mod relations;

const PDF_SHA: &str = "c6b981b664331329a4ab778c7dce6179ae162d27759c58dc9906d0056d815d68";
const TEXT_SHA: &str = "d2fb199b0d69294b525ef7826e1634f9a4249a64f30ce55cf142737c5846e90a";
const MANUAL_URL: &str = "https://www.altova.com/manual/mapforce/mapforceenterprise/";
const PDF_BINDING: &str = "Pinned retained Enterprise 2026r2 documentation landing and internal Release 2 text; cover says 2026. Mutable URL is not an installer/runtime binary binding.";

// Exact page order and independently reviewed decomposition sizes. The ID
// ledger supplies unit identities, not this page partition or its placements.
const API_PAGES: [(&str, usize); 120] = [
    ("cg_classref_cpp_datetime", 20),
    ("cg_classref_cpp_duration", 14),
    ("cg_classref_cpp_daytimeduration", 10),
    ("cg_classref_cpp_ymduration", 8),
    ("cg_classref_cpp_meta_attribute", 6),
    ("cg_classref_cpp_meta_complex", 10),
    ("cg_classref_cpp_meta_element", 7),
    ("cg_classref_cpp_meta_simple", 17),
    ("cg_classref_cpp_yourschema", 21),
    ("cg_classref_cpp_elementtype", 1),
    ("cg_classref_cpp_memberattribute", 5),
    ("cg_classref_cpp_memberelement", 13),
    ("cg_classref_cs_datetime", 24),
    ("cg_classref_cs_datetimeformat", 11),
    ("cg_classref_cs_duration", 15),
    ("cg_classref_cs_meta_attribute", 5),
    ("cg_classref_cs_meta_complex", 10),
    ("cg_classref_cs_meta_element", 6),
    ("cg_classref_cs_meta_simple", 16),
    ("cg_classref_cs_yourschema", 20),
    ("cg_classref_cs_elementtype", 1),
    ("cg_classref_cs_memberattribute", 5),
    ("cg_classref_cs_memberelement", 14),
    ("cg_classref_java_datetime", 30),
    ("cg_classref_java_duration", 22),
    ("cg_classref_java_meta_attribute", 4),
    ("cg_classref_java_meta_complex", 8),
    ("cg_classref_java_meta_element", 5),
    ("cg_classref_java_meta_simple", 15),
    ("cg_classref_java_yourschema", 17),
    ("cg_classref_java_elementtype", 1),
    ("cg_classref_java_memberattribute", 7),
    ("cg_classref_java_memberelement", 15),
    ("mf_comapi", 1),
    ("ax_prerequisites", 4),
    ("integpl_csintroduction", 1),
    ("integrationapplevel", 1),
    ("ax_doclevel", 2),
    ("ax_run_cs_sample", 1),
    ("integpl_csretrievingcmdinfo", 2),
    ("integpl_cshandlingeventsmf", 1),
    ("integpl_javaexample", 1),
    ("integpl_javacreatecontrols", 1),
    ("integpl_javaloaddata", 1),
    ("integpl_javabasiceventhandling", 1),
    ("integpl_javamenus", 2),
    ("integpl_javauiupdateeh", 2),
    ("integpl_javamfmapping", 1),
    ("integpl_vb", 1),
    ("ax_mf_menu1", 25),
    ("ax_mf_menu2", 10),
    ("ax_mf_menu3", 19),
    ("ax_mf_menu4", 20),
    ("ax_mf_menu5", 22),
    ("ax_mf_menu6", 7),
    ("ax_mf_menu7", 6),
    ("ax_mf_menu8", 18),
    ("ax_mf_menu9", 6),
    ("ax_mf_menu10", 18),
    ("ax_mf_menu11", 5),
    ("ax_mf_menu12", 3),
    ("ax_mf_menu13", 14),
    ("appcommandaccelerator", 1),
    ("appcommandid", 1),
    ("appcommandisseparator", 1),
    ("appcommandlabel", 1),
    ("appcommandname", 1),
    ("appcommandstatustext", 1),
    ("appcommandsubcommands", 1),
    ("appcommandtooltip", 1),
    ("appcommandscount", 1),
    ("appcommandsitem", 1),
    ("appcontrol_appearance", 1),
    ("appcontrol_application", 1),
    ("appcontrol_borderstyle", 1),
    ("appcontrol_commandslist", 1),
    ("appcontrol_enableuserprompts", 1),
    ("appcontrol_integrationlevel", 1),
    ("appcontrol_mainmenu", 1),
    ("appcontrol_toolbars", 1),
    ("appcontrol_exec", 1),
    ("appcontrol_opendocument", 1),
    ("appcontrol_querystatus", 1),
    ("control_events_oncloseeditingwindow", 1),
    ("control_events_ondocumentope", 1),
    ("controlevents_onfilechangedale", 1),
    ("controlevents_onlicenseproblem", 1),
    ("controlevents_onopenedorfocuse", 1),
    ("control_events_ontoolwinupdated", 1),
    ("control_events_onupdatecmdui", 1),
    ("controlevents_onvalidationwind", 1),
    ("appcontroldocument_appearance", 1),
    ("appcontroldocument_borderstyle", 1),
    ("appcontroldocument_document", 1),
    ("appcontroldocument_ismodified", 1),
    ("appcontroldocument_path", 1),
    ("appcontroldocument_readonly", 1),
    ("appcontroldocument_exec", 1),
    ("appcontroldocument_new", 1),
    ("appcontroldocument_open", 1),
    ("appcontroldocument_querystatus", 1),
    ("appcontroldocument_reload", 1),
    ("appcontroldocument_save", 1),
    ("appcontroldocument_saveas", 1),
    ("appcontroldoc_events_onactivate", 1),
    ("appcontroldoc_events_ondocumenclosed", 1),
    ("appcontroldoc_events_ondocumentopened", 1),
    ("appcontroldoc_events_ondocumentsaveas", 1),
    ("appcontroldoc_events_onfilechange", 1),
    ("appcontroldoc_events_onmodifiedflagchanged", 1),
    ("appcontroldoc_events_onseteditortitle", 1),
    ("appcontrolplaceh_label", 1),
    ("appcontrolplaceh_placehwindowid", 1),
    ("appcontrolplaceh_project", 1),
    ("appcontrolplaceh_openproject", 1),
    ("appcontrolplaceh_closeproject", 1),
    ("appcontrolplaceh_events_onmodifiedflagchanged", 1),
    ("appcontrolplaceh_events_onsetlabel", 1),
    ("enum_icactivexintegrationlevel", 2),
    ("enumplaceholderwindmf", 8),
];

const BASE_IDS: &str = "interop.mfd-designs interop.vendor-roundtrip interop.package-resources interop.document-fidelity mapping.iteration mapping.controls mapping.general-sequences mapping.joins mapping.stage-dag mapping.dynamic-documents functions.builtins functions.graph-udfs functions.external-code formats.xml formats.json formats.flat-files formats.xlsx formats.flextext formats.edi formats.protobuf formats.xbrl formats.pdf connectors.sqlite connectors.enterprise-databases connectors.http-services connectors.graphql authoring.blank-project authoring.format-wizards authoring.editor-integrity debug.execution-traces debug.interactive-stepping deployment.host-api deployment.vendor-backends extensions.recursive-construction extensions.edi-metadata extensions.captured-services extensions.roundtrip-metadata";

const CASE_IDS: [&str; 7] = [
    "enterprise2026r2.workflow.ax_prerequisites.Java-Eclipse-prerequisites.line-63121",
    "enterprise2026r2.workflow.integpl_csretrievingcmdinfo.CSharp-dynamic-menu-merge.line-63519",
    "enterprise2026r2.workflow.integpl_csretrievingcmdinfo.CSharp-active-document-command-routing.line-63584",
    "enterprise2026r2.workflow.integpl_javamenus.Java-recursive-menu-projection.line-63984",
    "enterprise2026r2.workflow.integpl_javamenus.Java-menu-action-close-routing.line-64061",
    "enterprise2026r2.workflow.integpl_javauiupdateeh.Java-command-enablement-callback.line-64100",
    "enterprise2026r2.workflow.integpl_javauiupdateeh.Java-active-editor-callback.line-64117",
];

const APPENDIX: [(&str, usize, usize); 11] = [
    ("fxaltova_xsltgeneral", 5, 5),
    ("fxaltova_xpxqdatetime", 39, 45),
    ("fxaltova_xpxqgeolocation", 7, 7),
    ("fxaltova_xpxqimage", 2, 2),
    ("fxaltova_xpxqnumeric", 4, 4),
    ("fxaltova_xpxqschema", 2, 3),
    ("fxaltova_xpxqsequence", 11, 13),
    ("fxaltova_xpxqstring", 15, 21),
    ("fxaltova_xpxqmisc", 7, 9),
    ("fxaltova_chartdataxmlstruc", 0, 0),
    ("fxaltova_chartfunctionssexample", 0, 0),
];

fn load() -> Value {
    serde_json::from_str(include_str!(
        "../../../../conformance/mfd-2026r2.source-contracts.json"
    ))
    .unwrap()
}

fn ledger_ids() -> BTreeSet<String> {
    let ledger: Value =
        serde_json::from_str(include_str!("../../../../conformance/mfd-2026r2.json")).unwrap();
    assert_eq!(ledger["inventory_complete"], false);
    index(&ledger["capabilities"], "id")
        .keys()
        .map(|id| (*id).to_owned())
        .collect()
}

fn rows(value: &Value) -> &[Value] {
    value.as_array().unwrap()
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap()
}

fn index<'a>(value: &'a Value, field: &str) -> BTreeMap<&'a str, &'a Value> {
    let mut result = BTreeMap::new();
    for row in rows(value) {
        let id = text(&row[field]);
        assert!(result.insert(id, row).is_none(), "duplicate {field}: {id}");
    }
    result
}

fn names(value: &Value) -> BTreeSet<&str> {
    let result: BTreeSet<_> = rows(value).iter().map(text).collect();
    assert_eq!(result.len(), rows(value).len(), "duplicate reference");
    result
}

fn units(inventory: &Value) -> BTreeMap<&str, &Value> {
    let mut result = BTreeMap::new();
    for (group, kind, count) in [
        ("API", "documented_API_or_bounded_workflow", 576),
        (
            "existing_ID_enrichments",
            "exact_owning_research_leaf_or_outcome",
            67,
        ),
        ("appendix", "appendix_signature", 109),
    ] {
        let values = &inventory["source_units"][group];
        assert_eq!(rows(values).len(), count, "{group}");
        for (id, unit) in index(values, "research_id") {
            assert!(result.insert(id, unit).is_none(), "duplicate unit: {id}");
            assert_eq!(unit["source_kind"], kind);
            let data = &unit["data"];
            assert_eq!(data["applicability"]["status"], "uncertain");
            assert_eq!(data["execution_evidence"], json!([]));
            assert!(data["source_assessments"].is_object());
            assert_eq!(
                data.get("id").or_else(|| data.get("existing_id")).unwrap(),
                id
            );
        }
    }
    assert_eq!(inventory["source_units"].as_object().unwrap().len(), 3);
    assert_eq!(result.len(), 752);
    result
}

fn check_identities(inventory: &Value, ledger: &BTreeSet<String>) {
    let maps = index(
        &inventory["identity_policy"]["canonical_identity_map"],
        "research_id",
    );
    assert_eq!(maps.len(), 789);
    let canonical: BTreeSet<_> = maps
        .values()
        .map(|row| text(&row["canonical_id"]).to_owned())
        .collect();
    assert_eq!(&canonical, ledger);
    let normalized: BTreeSet<_> = maps
        .iter()
        .filter_map(|(id, row)| {
            let canonical = text(&row["canonical_id"]);
            assert_eq!(canonical, id.to_ascii_lowercase());
            (*id != canonical).then_some(*id)
        })
        .collect();
    assert_eq!(normalized, CASE_IDS.into_iter().collect());
    let all_units = units(inventory);
    let base: BTreeSet<_> = BASE_IDS.split_whitespace().collect();
    assert_eq!(base.len(), 37);
    let expected: BTreeSet<_> = maps
        .keys()
        .copied()
        .filter(|id| !base.contains(id))
        .collect();
    assert_eq!(all_units.keys().copied().collect::<BTreeSet<_>>(), expected);
    for (id, unit) in &all_units {
        assert_eq!(unit["canonical_id"], maps[id]["canonical_id"]);
    }
    let contexts = index(&inventory["research_base_contexts"], "research_id");
    assert_eq!(contexts.len(), 1252);
    let enriched = index(
        &inventory["source_units"]["existing_ID_enrichments"],
        "research_id",
    );
    let mut adopted = BTreeSet::new();
    let mut absent = 0;
    for (id, context) in &contexts {
        assert_eq!(
            text(&context["official_url"]).split('#').next().unwrap(),
            format!("{MANUAL_URL}{}", text(&context["source_file"]))
        );
        // Some unadopted context pages have no broad canonical parent either.
        if let Some(parent) = context["broad_canonical_context"].as_str() {
            assert!(ledger.contains(parent));
        } else {
            assert!(context["broad_canonical_context"].is_null());
        }
        if let Some(unit) = enriched.get(id) {
            assert_eq!(context["canonical_id"], unit["canonical_id"]);
            adopted.insert(*id);
        } else {
            assert_eq!(
                context.get("canonical_id"),
                Some(&Value::Null),
                "context-only {id}"
            );
            assert!(!maps.contains_key(id));
            absent += 1;
        }
    }
    assert_eq!(adopted, enriched.keys().copied().collect());
    assert_eq!(absent, 1185);
}

fn classification(page: usize) -> &'static str {
    match page {
        13 | 118 | 119 => "enum_type_container_values_decomposed",
        0..=32 => "generated_class_reference_container",
        33 => "landing_to_separate_automation_API_manual_no_local_member_catalog",
        34..=48 => "setup_integration_workflow_or_example_not_member_declaration",
        49..=61 => "menu_command_ID_table_not_method_catalog",
        62..=117 => "already_independently_named_property_method_or_event_leaf",
        _ => unreachable!(),
    }
}

fn check_page_source(source: &Value, page: &Value, file_key: &str, url_key: &str) {
    assert_eq!(source[file_key], page["source_identity"]["file"]);
    assert_eq!(source[url_key], page["source_identity"]["url"]);
    assert_eq!(source["section"], page["section"]);
    if let Some(path) = source.get("manual_path").or_else(|| source.get("path")) {
        assert_eq!(path, &page["source_identity"]["manual_path"]);
    }
}

fn check_family_scopes(data: &Value, placements: &[&str], contexts: &BTreeMap<&str, &Value>) {
    for id in placements {
        assert!(contexts.contains_key(id), "missing family context {id}");
    }
    if data["source"].get("occurrences").is_some() {
        // The two repeated commands retain both page placements, in source
        // order, rather than inventing a second command identity.
        assert_eq!(data["existing_family_scopes"], json!(placements));
        assert!(data.get("existing_family_scope").is_none());
    } else if data["source"].get("page").is_some() {
        assert_eq!(placements.len(), 1);
        assert_eq!(data["existing_parent_id"], placements[0]);
        assert!(data.get("existing_family_scope").is_none());
        assert!(data.get("existing_family_scopes").is_none());
    } else if data.get("id").is_some() {
        assert_eq!(placements.len(), 1);
        assert_eq!(data["existing_family_scope"], placements[0]);
        assert!(data.get("existing_family_scopes").is_none());
    } else {
        // Existing ActiveX leaves are already their own context identity.
        assert_eq!(placements, [text(&data["existing_id"])]);
        assert!(data.get("existing_family_scope").is_none());
        assert!(data.get("existing_family_scopes").is_none());
    }
}

fn check_pages(inventory: &Value) {
    let boundary = &inventory["bounded_source_pages"];
    let partitions = index(&boundary["API_partition"], "group");
    assert_eq!(partitions.len(), 4);
    for (group, expected) in [
        ("generated", (0..=32).collect::<Vec<_>>()),
        ("com", (33..=48).collect()),
        ("command-enum", (49..=61).chain(118..=119).collect()),
        ("activex", (62..=117).collect()),
    ] {
        assert_eq!(partitions[group]["indices"], json!(expected));
    }
    let pages = rows(&boundary["API_pages"]);
    assert_eq!(pages.len(), API_PAGES.len());
    let contexts = index(&inventory["research_base_contexts"], "research_id");
    let all_units = units(inventory);
    let mut expected_refs: Vec<BTreeSet<&str>> = vec![BTreeSet::new(); 120];
    let mut by_file = BTreeMap::new();
    for (i, (page, (stem, count))) in pages.iter().zip(API_PAGES).enumerate() {
        assert_eq!(page["index"], i);
        assert_eq!(page["source_identity"]["file"], format!("{stem}.html"));
        assert_eq!(
            page["source_identity"]["url"],
            format!("{MANUAL_URL}{stem}.html")
        );
        assert_eq!(page["source_classification"], classification(i));
        assert_eq!(names(&page["source_unit_ids"]).len(), count);
        let page_id = format!("enterprise2026r2.doc.{stem}");
        assert_eq!(page["research_page_ids"], json!([page_id]));
        let context = contexts[page_id.as_str()];
        assert_eq!(context["source_file"], page["source_identity"]["file"]);
        assert_eq!(
            context["manual_path"],
            page["source_identity"]["manual_path"]
        );
        assert!(
            by_file
                .insert(text(&page["source_identity"]["file"]), i)
                .is_none()
        );
    }
    for (id, unit) in all_units
        .iter()
        .filter(|(_, u)| u["source_kind"] != "appendix_signature")
    {
        let data = &unit["data"];
        let source = &data["source"];
        let mut placements = Vec::new();
        if let Some(occurrences) = source.get("occurrences") {
            assert!(!rows(occurrences).is_empty());
            for occurrence in rows(occurrences) {
                let i = occurrence["master_index"].as_u64().unwrap() as usize;
                let page = &pages[i];
                check_page_source(occurrence, page, "source_file", "url");
                assert_eq!(
                    occurrence["existing_page_scopes"],
                    page["research_page_ids"]
                );
                let line = occurrence["line"].as_u64().unwrap();
                let end = occurrence["end_line"].as_u64().unwrap();
                assert!(page["text_lines"][0].as_u64().unwrap() <= line && line <= end);
                assert!(end <= page["text_lines"][1].as_u64().unwrap());
                assert!(expected_refs[i].insert(id), "duplicate unit placement {id}");
                placements.push(text(&page["research_page_ids"][0]));
            }
            let first = &occurrences[0];
            check_page_source(
                source,
                &pages[by_file[text(&source["file"])]],
                "file",
                "url",
            );
            assert_eq!(source["primary_line"], first["line"]);
            assert_eq!(source["file"], first["source_file"]);
            assert_eq!(
                source["signature_lines"],
                json!([first["line"], first["end_line"]])
            );
        } else {
            let anchor = source.get("page").unwrap_or(source);
            let i = by_file[text(&anchor["source_file"])];
            check_page_source(anchor, &pages[i], "source_file", "official_url");
            if let Some(master) = data.get("master_index") {
                assert_eq!(master, &pages[i]["index"]);
            }
            if let Some(lines) = source.get("signature_lines") {
                assert!(lines[0].as_u64().unwrap() >= pages[i]["text_lines"][0].as_u64().unwrap());
                assert!(lines[1].as_u64().unwrap() <= pages[i]["text_lines"][1].as_u64().unwrap());
                assert!(lines[0].as_u64().unwrap() <= lines[1].as_u64().unwrap());
            }
            assert!(expected_refs[i].insert(id));
            placements.push(text(&pages[i]["research_page_ids"][0]));
        }
        check_family_scopes(data, &placements, &contexts);
    }
    for (page, expected) in pages.iter().zip(expected_refs) {
        assert_eq!(
            names(&page["source_unit_ids"]),
            expected,
            "page {}",
            page["index"]
        );
    }
    let appendix = rows(&boundary["appendix_pages"]);
    assert_eq!(appendix.len(), APPENDIX.len());
    let mut seen = BTreeSet::new();
    for (page, (stem, named_count, signature_count)) in appendix.iter().zip(APPENDIX) {
        assert_eq!(page["file"], format!("{stem}.html"));
        assert_eq!(
            page["original_family_scope"],
            format!("enterprise2026r2.doc.{stem}")
        );
        assert!(contexts.contains_key(text(&page["original_family_scope"])));
        for field in ["expected_names", "actual_named_functions"] {
            assert_eq!(page[field], named_count);
        }
        for field in ["expected_signatures", "actual_signatures"] {
            assert_eq!(page[field], signature_count);
        }
        let refs = names(&page["proposal_ids"]);
        assert_eq!(refs.len(), signature_count);
        let mut function_names = BTreeSet::new();
        for id in refs {
            assert!(seen.insert(id));
            let unit = all_units[id];
            assert_eq!(unit["source_kind"], "appendix_signature");
            let source = &unit["data"]["source"];
            check_family_scopes(
                &unit["data"],
                &[text(&page["original_family_scope"])],
                &contexts,
            );
            assert_eq!(source["source_file"], page["file"]);
            assert_eq!(source["section"], page["section"]);
            assert_eq!(source["official_url"], format!("{MANUAL_URL}{stem}.html"));
            assert_eq!(
                id,
                format!(
                    "enterprise2026r2.appendix.{stem}.signature-line-{}",
                    source["signature_lines"][0]
                )
            );
            let start = source["signature_lines"][0].as_u64().unwrap();
            let end = source["signature_lines"][1].as_u64().unwrap();
            assert!(page["start"].as_u64().unwrap() <= start && start <= end);
            assert!(end <= page["end"].as_u64().unwrap());
            function_names.insert(text(&unit["data"]["member_identity"]["qname"]));
        }
        assert_eq!(function_names.len(), named_count);
    }
    assert_eq!(
        seen,
        all_units
            .iter()
            .filter_map(|(id, u)| (u["source_kind"] == "appendix_signature").then_some(*id))
            .collect()
    );
}

fn check_placements(inventory: &Value) {
    let all_units = units(inventory);
    let mut expected = BTreeMap::new();
    let mut repeats = BTreeMap::new();
    let mut constants = BTreeSet::new();
    let mut identities = BTreeSet::new();
    let mut commands = BTreeMap::new();
    for (id, unit) in all_units {
        let data = &unit["data"];
        let Some(occurrences) = data["source"].get("occurrences") else {
            continue;
        };
        let identity = &data["member_identity"];
        assert!(identities.insert((
            text(&identity["owner"]),
            text(&identity["name"]),
            identity["numeric_value"].as_i64().unwrap()
        )));
        assert!(matches!(
            text(&identity["kind"]),
            "command_identifier" | "enum_constant"
        ));
        assert_eq!(
            identity["semantic_key"],
            json!([
                identity["owner"],
                identity["name"],
                identity["numeric_value"]
            ])
        );
        assert_eq!(data["contract"]["literal_symbol"], identity["name"]);
        assert_eq!(data["contract"]["numeric_value"], identity["numeric_value"]);
        if rows(occurrences).len() > 1 {
            repeats.insert(
                id,
                rows(occurrences)
                    .iter()
                    .map(|o| {
                        (
                            o["master_index"].as_u64().unwrap(),
                            o["line"].as_u64().unwrap(),
                        )
                    })
                    .collect::<Vec<_>>(),
            );
        }
        for occurrence in rows(occurrences) {
            assert_eq!(occurrence["kind"], identity["kind"]);
            assert_eq!(occurrence["identity"], identity["semantic_key"]);
            assert_eq!(occurrence["symbol"], identity["name"]);
            assert_eq!(occurrence["owner"], identity["owner"]);
            assert_eq!(occurrence["numeric_value"], identity["numeric_value"]);
            let key = (
                occurrence["master_index"].as_u64().unwrap(),
                occurrence["line"].as_u64().unwrap(),
            );
            assert!(expected.insert(key, occurrence).is_none());
        }
        if identity["kind"] == "enum_constant" {
            constants.insert((
                text(&identity["owner"]),
                text(&identity["name"]),
                identity["numeric_value"].as_i64().unwrap(),
            ));
        } else {
            assert!(
                commands
                    .insert(
                        text(&identity["name"]),
                        identity["numeric_value"].as_i64().unwrap()
                    )
                    .is_none()
            );
        }
    }
    assert_eq!(commands, command_values::EXPECTED.into_iter().collect());
    assert_eq!(
        repeats,
        BTreeMap::from([
            (
                "enterprise2026r2.api.ax_mf_menu3.row-64474",
                vec![(51, 64474), (55, 64668)]
            ),
            (
                "enterprise2026r2.api.ax_mf_menu3.row-64476",
                vec![(51, 64476), (55, 64670)]
            ),
        ])
    );
    assert_eq!(
        constants,
        BTreeSet::from([
            (
                "ICActiveXIntegrationLevel",
                "ICActiveXIntegrationOnApplicationLevel",
                0
            ),
            (
                "ICActiveXIntegrationLevel",
                "ICActiveXIntegrationOnDocumentLevel",
                1
            ),
            ("MapForceControlPlaceholderWindow", "MapForceXNoWindow", -1),
            (
                "MapForceControlPlaceholderWindow",
                "MapForceXLibraryWindow",
                0
            ),
            (
                "MapForceControlPlaceholderWindow",
                "MapForceXOverviewWindow",
                1
            ),
            (
                "MapForceControlPlaceholderWindow",
                "MapForceXValidationWindow",
                2
            ),
            (
                "MapForceControlPlaceholderWindow",
                "MapForceXProjectWindow",
                3
            ),
            (
                "MapForceControlPlaceholderWindow",
                "MapForceXDebuggerValuesWindow",
                4
            ),
            (
                "MapForceControlPlaceholderWindow",
                "MapForceXDebuggerContextWindow",
                5
            ),
            (
                "MapForceControlPlaceholderWindow",
                "MapForceXDebuggerPointsWindow",
                6
            ),
        ])
    );
    let placements = rows(&inventory["supporting_source"]["command_placements"]);
    assert_eq!(placements.len(), 183);
    let mut actual = BTreeMap::new();
    for placement in placements {
        let key = (
            placement["master_index"].as_u64().unwrap(),
            placement["line"].as_u64().unwrap(),
        );
        assert!(actual.insert(key, placement).is_none());
    }
    assert_eq!(actual, expected);
}

fn check_boundaries(value: &Value) {
    match value {
        Value::Object(fields) => {
            for (key, child) in fields {
                match key.as_str() {
                    "inventory_complete" | "runtime_proof" | "formal_API_identity_claim"
                    | "api_identity_equal" | "status_or_support_transferred"
                    | "new_product_behavior_claim" | "canonical_status_transfer"
                    | "status_or_evidence_transfer" | "identity_equality_inferred"
                    | "new_parity_row" | "runtime_evidence_transferred"
                    | "relation_is_runtime_proof" | "exact_API_identity_claim" => assert_eq!(child, false, "{key}"),
                    "execution_evidence" | "execution_receipts" | "evidence" => assert_eq!(child, &json!([]), "{key}"),
                    "applicability" => {
                        if child.is_object() {
                            assert_eq!(child["status"], "uncertain");
                        } else {
                            assert_eq!(child, "unknown");
                        }
                    }
                    "source_assessments" => {
                        let dimensions = child.as_object().unwrap();
                        assert_eq!(dimensions.len(), 9);
                        for dimension in ["import", "interpreter", "native_export", "ferrule_roundtrip", "gui", "debug", "rust", "csharp"] {
                            assert_eq!(child[dimension]["status"], "unassessed");
                            assert_eq!(child[dimension]["evidence"], json!([]));
                            assert!(!text(&child[dimension]["detail"]).trim().is_empty());
                        }
                        assert_eq!(child["vendor_backends"].as_object().unwrap().len(), 8);
                        for backend in ["xslt1", "xslt2", "xslt3", "xquery1", "cpp", "java", "csharp", "built_in"] {
                            assert_eq!(child["vendor_backends"][backend]["status"], "unassessed");
                            assert_eq!(child["vendor_backends"][backend]["evidence"], json!([]));
                            assert!(!text(&child["vendor_backends"][backend]["detail"]).trim().is_empty());
                        }
                    }
                    "pdf_sha256" => assert_eq!(child, PDF_SHA),
                    "text_sha256" => assert_eq!(child, TEXT_SHA),
                    "source_pdf_ref" => assert_eq!(child, "#/source_documents/manual_pdf"),
                    "source_text_ref" => assert_eq!(child, "#/source_documents/manual_text"),
                    "physical_pdf_page" => assert!((1..=1751).contains(&child.as_u64().unwrap())),
                    "physical_pdf_pages" => {
                        assert!(!rows(child).is_empty());
                        for page in rows(child) {
                            assert!((1..=1751).contains(&page.as_u64().unwrap()));
                        }
                    }
                    "release_binding" => assert!([
                        PDF_BINDING,
                        "retained_official_2026r2_landing_plus_internal_Release2_text; mutable_URL_not_installer_bound",
                        "retained_official_2026r2_landing_plus_internal_Release2_text; cover_2026_mutable_URL_not_installer_bound",
                        "retained_official_2026r2_landing_and_internal_Release2; cover_2026_mutable_URL_not_installer_bound",
                        "unestablished",
                    ].contains(&text(child))),
                    _ => {}
                }
                check_boundaries(child);
            }
        }
        Value::Array(values) => values.iter().for_each(check_boundaries),
        _ => {}
    }
}

fn check_inventory(inventory: &Value, ledger: &BTreeSet<String>) {
    assert_eq!(
        inventory["format"],
        "mapforce-enterprise-2026r2-source-contracts"
    );
    assert_eq!(inventory["format_version"], 1);
    assert_eq!(inventory["inventory_complete"], false);
    assert_eq!(
        inventory["target"],
        json!({"vendor_product":"MapForce", "edition":"Enterprise", "release":"2026r2"})
    );
    assert_eq!(inventory["assessment_boundary"]["status"], "unassessed");
    assert_eq!(inventory["assessment_boundary"]["applicability"], "unknown");
    let sources = &inventory["source_documents"];
    assert_eq!(sources["manual_pdf"]["sha256"], PDF_SHA);
    assert_eq!(sources["manual_pdf"]["release_binding"], PDF_BINDING);
    assert_eq!(
        sources["manual_pdf"]["official_url"],
        "https://www.altova.com/documents/MapForceEnterprise.pdf"
    );
    assert_eq!(sources["manual_text"]["sha256"], TEXT_SHA);
    assert_eq!(
        sources["manual_text"]["derived_from"],
        "#/source_documents/manual_pdf"
    );
    assert_eq!(
        sources["release_and_scope_references"][1]["sha256"],
        PDF_SHA
    );
    assert_eq!(sources["release_and_scope_references"][1]["pages"], 1751);
    assert_eq!(
        sources["release_and_scope_references"][2]["uncompressed_sha256"],
        "2caa398befc17a69849b7a74b6a32ecbd170029747616aa1dad6542400d7b39b"
    );
    assert_eq!(inventory["remaining_gaps"]["inventory_complete"], false);
    assert_eq!(
        inventory["remaining_gaps"]["selected_API_page_boundary"],
        120
    );
    assert_eq!(
        inventory["remaining_gaps"]["completed_source_reviews"]["com_api"]["release_binding"],
        "unestablished"
    );
    check_identities(inventory, ledger);
    check_pages(inventory);
    check_placements(inventory);
    relations::check(inventory, ledger);
    check_boundaries(inventory);
}

fn reject(inventory: &mut Value, ledger: &BTreeSet<String>, pointer: &str, wrong: Value) {
    // Mutate and restore one tree: cloning the 20 MiB source companion for every
    // control obscures the graph checks with unnecessary memory pressure.
    let original = std::mem::replace(inventory.pointer_mut(pointer).unwrap(), wrong);
    let rejected = std::panic::catch_unwind(|| check_inventory(inventory, ledger)).is_err();
    *inventory.pointer_mut(pointer).unwrap() = original;
    assert!(rejected, "accepted mutation: {pointer}");
}

#[test]
fn source_contracts_preserve_exact_coverage_and_source_only_boundaries() {
    check_inventory(&load(), &ledger_ids());
}

#[test]
fn source_contracts_reject_missing_duplicate_and_dangling_coverage() {
    let mut inventory = load();
    let ledger = ledger_ids();
    check_inventory(&inventory, &ledger);
    for pointer in [
        "/bounded_source_pages/API_pages",
        "/bounded_source_pages/appendix_pages",
        "/source_units/API",
        "/source_units/existing_ID_enrichments",
        "/source_units/appendix",
        "/identity_policy/canonical_identity_map",
        "/research_base_contexts",
        "/supporting_source/command_placements",
    ] {
        let mut missing = rows(inventory.pointer(pointer).unwrap()).to_vec();
        missing.pop();
        reject(&mut inventory, &ledger, pointer, json!(missing));
        let mut duplicate = rows(inventory.pointer(pointer).unwrap()).to_vec();
        duplicate[1] = duplicate[0].clone();
        reject(&mut inventory, &ledger, pointer, json!(duplicate));
    }
    for (pointer, wrong) in [
        ("/bounded_source_pages/API_partition/0/indices/0", json!(33)),
        (
            "/bounded_source_pages/API_partition/1/group",
            json!("generated"),
        ),
        (
            "/bounded_source_pages/API_pages/0/source_unit_ids/0",
            json!("missing.unit"),
        ),
        (
            "/bounded_source_pages/API_pages/0/research_page_ids/0",
            json!("missing.page"),
        ),
        (
            "/bounded_source_pages/appendix_pages/0/proposal_ids/0",
            json!("missing.signature"),
        ),
        (
            "/source_units/API/0/canonical_id",
            json!("missing.canonical"),
        ),
        ("/source_units/API/0/research_id", json!("missing.research")),
        (
            "/source_units/API/0/data/source/source_file",
            json!("missing.html"),
        ),
        (
            "/source_units/existing_ID_enrichments/0/data/master_index",
            json!(63),
        ),
        (
            "/identity_policy/canonical_identity_map/0/canonical_id",
            json!("missing.canonical"),
        ),
        (
            "/identity_policy/canonical_identity_map/1/canonical_id",
            json!("interop.mfd-designs"),
        ),
        (
            "/research_base_contexts/0/canonical_id",
            json!("authoring.blank-project"),
        ),
        (
            "/semantic_relations/broad_research_source_containment/0/source_id",
            json!("missing.unit"),
        ),
        (
            "/semantic_relations/broad_research_source_containment/0/parent_research_id",
            json!("missing.page"),
        ),
        (
            "/semantic_relations/COM_member_use_or_conflict/0/record/existing_id",
            json!("missing.member"),
        ),
        (
            "/semantic_relations/COM_command_source_use_or_conflict/0/command_source_proposal_id",
            json!("missing.command"),
        ),
        (
            "/semantic_relations/COM_page_source_relations/0/to",
            json!(0),
        ),
    ] {
        reject(&mut inventory, &ledger, pointer, wrong);
    }
    check_inventory(&inventory, &ledger);
}

#[test]
fn source_contracts_reject_wrong_classifications_and_placement_repairs() {
    let mut inventory = load();
    let ledger = ledger_ids();
    check_inventory(&inventory, &ledger);
    for i in 0..120 {
        reject(
            &mut inventory,
            &ledger,
            &format!("/bounded_source_pages/API_pages/{i}/source_classification"),
            json!("enum_type_container_values_unprocessed"),
        );
    }
    for id in CASE_IDS {
        let i = rows(&inventory["identity_policy"]["canonical_identity_map"])
            .iter()
            .position(|row| row["research_id"] == id)
            .unwrap();
        reject(
            &mut inventory,
            &ledger,
            &format!("/identity_policy/canonical_identity_map/{i}/research_id"),
            json!(id.to_ascii_lowercase()),
        );
    }
    let i = rows(&inventory["research_base_contexts"])
        .iter()
        .position(|row| !row["canonical_id"].is_null())
        .unwrap();
    reject(
        &mut inventory,
        &ledger,
        &format!("/research_base_contexts/{i}/canonical_id"),
        Value::Null,
    );
    let api = rows(&inventory["source_units"]["API"]);
    let repeated: Vec<_> = api
        .iter()
        .enumerate()
        .filter_map(|(i, row)| {
            row["data"]["source"]["occurrences"]
                .as_array()
                .filter(|occ| occ.len() == 2)
                .map(|_| i)
        })
        .collect();
    assert_eq!(repeated.len(), 2);
    for i in repeated {
        let pointer = format!("/source_units/API/{i}/data/source/occurrences");
        let first = inventory.pointer(&pointer).unwrap()[0].clone();
        reject(&mut inventory, &ledger, &pointer, json!([first]));
        reject(
            &mut inventory,
            &ledger,
            &format!("{pointer}/1/master_index"),
            json!(51),
        );
        reject(
            &mut inventory,
            &ledger,
            &format!("{pointer}/1/identity/1"),
            json!("ID_INVENTED"),
        );
    }
    // Even a coherent value change across redundant fields and both source
    // views is not evidence for another enum value.
    let i = rows(&inventory["source_units"]["API"])
        .iter()
        .position(|row| row["data"]["member_identity"]["name"] == "MapForceXNoWindow")
        .unwrap();
    let mut data = inventory["source_units"]["API"][i]["data"].clone();
    data["member_identity"]["numeric_value"] = json!(0);
    data["member_identity"]["semantic_key"][2] = json!(0);
    data["contract"]["numeric_value"] = json!(0);
    data["source"]["occurrences"][0]["numeric_value"] = json!(0);
    data["source"]["occurrences"][0]["identity"][2] = json!(0);
    let placement = rows(&inventory["supporting_source"]["command_placements"])
        .iter()
        .position(|row| row["symbol"] == "MapForceXNoWindow")
        .unwrap();
    let original = inventory["supporting_source"]["command_placements"][placement].clone();
    inventory["supporting_source"]["command_placements"][placement] =
        data["source"]["occurrences"][0].clone();
    reject(
        &mut inventory,
        &ledger,
        &format!("/source_units/API/{i}/data"),
        data,
    );
    inventory["supporting_source"]["command_placements"][placement] = original;
    check_inventory(&inventory, &ledger);
}

#[test]
fn source_contracts_reject_execution_applicability_and_provenance_promotions() {
    let mut inventory = load();
    let ledger = ledger_ids();
    check_inventory(&inventory, &ledger);
    for (pointer, wrong) in [
        ("/inventory_complete", json!(true)),
        ("/remaining_gaps/inventory_complete", json!(true)),
        ("/assessment_boundary/status", json!("supported")),
        ("/assessment_boundary/applicability", json!("applicable")),
        (
            "/assessment_boundary/execution_evidence",
            json!(["source is not execution"]),
        ),
        (
            "/assessment_boundary/execution_receipts",
            json!(["invented receipt"]),
        ),
        (
            "/source_units/API/0/data/contract/runtime_proof",
            json!(true),
        ),
        (
            "/source_units/API/0/data/applicability/status",
            json!("applicable"),
        ),
        (
            "/source_units/API/0/data/execution_evidence",
            json!(["invented"]),
        ),
        (
            "/source_units/API/0/data/source/pdf_sha256",
            json!("0".repeat(64)),
        ),
        (
            "/source_units/API/0/data/source/source_text_ref",
            json!("#/missing"),
        ),
        (
            "/source_units/API/0/data/source/release_binding",
            json!("2026r2 runtime verified"),
        ),
        ("/source_documents/manual_pdf/sha256", json!("0".repeat(64))),
        (
            "/source_documents/manual_text/sha256",
            json!("0".repeat(64)),
        ),
        (
            "/remaining_gaps/completed_source_reviews/com_api/release_binding",
            json!("2026r2"),
        ),
        (
            "/semantic_relations/exact_research_ID_enrichments/0/canonical_status_transfer",
            json!(true),
        ),
    ] {
        reject(&mut inventory, &ledger, pointer, wrong);
    }
    for group in ["API", "existing_ID_enrichments", "appendix"] {
        for dimension in [
            "import",
            "interpreter",
            "native_export",
            "ferrule_roundtrip",
            "gui",
            "debug",
            "rust",
            "csharp",
            "vendor_backends/xslt1",
            "vendor_backends/xslt2",
            "vendor_backends/xslt3",
            "vendor_backends/xquery1",
            "vendor_backends/cpp",
            "vendor_backends/java",
            "vendor_backends/csharp",
            "vendor_backends/built_in",
        ] {
            reject(
                &mut inventory,
                &ledger,
                &format!("/source_units/{group}/0/data/source_assessments/{dimension}/status"),
                json!("supported"),
            );
        }
    }
}
