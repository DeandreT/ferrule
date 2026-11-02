//! Finite source declarations, not compiled types, exception behavior or support.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::{load, source_only};

#[path = "supporting_exception_declarations/mutations.rs"]
mod mutations;

const PREFIX: &str = "enterprise2026r2.supporting.";
const FILE: &str = "supporting-exception-declarations.json";

fn keys(value: &Value, expected: &[&str]) {
    let actual: BTreeSet<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(actual, expected.iter().copied().collect(), "schema keys");
}

fn array(value: &Value) -> &[Value] {
    value.as_array().unwrap()
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap()
}

fn count(value: &Value) -> u64 {
    value.as_u64().unwrap()
}

fn nullable_text(value: &Value) {
    assert!(value.is_null() || value.as_str().is_some());
}

fn empty(value: &Value) {
    assert!(array(value).is_empty());
}

fn lines(value: &Value) {
    let range = array(value);
    assert_eq!(range.len(), 2);
    assert!(count(&range[0]) > 0 && count(&range[0]) <= count(&range[1]));
}

fn anchored_literals(value: &Value) {
    for row in array(value) {
        keys(row, &["literal", "line"]);
        assert!(!text(&row["literal"]).is_empty());
        assert!(count(&row["line"]) > 0);
    }
}

fn hash(value: &Value) {
    let s = text(value);
    assert_eq!(s.len(), 64);
    assert!(
        s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
}

/// Shape and internal coherence are deliberately checked before the frozen oracle.
/// A coherently changed source fact can satisfy this layer and still be wrong.
fn structure(v: &Value) {
    keys(
        v,
        &[
            "format",
            "format_version",
            "inventory_complete",
            "runtime_proof",
            "status",
            "applicability",
            "execution_evidence",
            "execution_receipts",
            "purpose",
            "provenance",
            "scope",
            "sources",
            "owners",
            "aliases",
            "manual_claim_refs",
            "qualification",
            "method",
        ],
    );
    assert_eq!(
        v["format"],
        "mapforce-enterprise-2026r2-supporting-exception-declarations"
    );
    assert_eq!(v["format_version"], 1);
    assert_eq!(v["inventory_complete"], false);
    assert_eq!(v["runtime_proof"], false);
    assert_eq!(v["status"], "unassessed");
    assert_eq!(v["applicability"], "unknown");
    empty(&v["execution_evidence"]);
    empty(&v["execution_receipts"]);
    source_only(v);
    let p = &v["provenance"];
    keys(
        p,
        &[
            "release_ref",
            "wrapper_ref",
            "generation_manifest",
            "generation_manifest_sha256",
            "generator_version",
            "generator_sha256",
            "inputs",
            "source_binding",
        ],
    );
    for input in array(&p["inputs"]) {
        keys(input, &["name", "bytes", "sha256", "artifact_name"]);
        hash(&input["sha256"]);
        assert!(count(&input["bytes"]) > 0);
        assert_eq!(
            input["artifact_name"],
            format!("Tutorial/{}", text(&input["name"]))
        );
    }
    hash(&p["generation_manifest_sha256"]);
    hash(&p["generator_sha256"]);
    let mut source_ids = BTreeSet::new();
    for source in array(&v["sources"]) {
        keys(
            source,
            &[
                "id",
                "profiles",
                "path",
                "bytes",
                "sha256",
                "installed_template",
                "imports",
                "inclusion_directives",
                "includes",
            ],
        );
        assert!(source_ids.insert(text(&source["id"])));
        hash(&source["sha256"]);
        assert!(count(&source["bytes"]) > 0);
        let path = text(&source["path"]);
        assert!(!path.starts_with('/') && !path.split('/').any(|part| part == ".."));
        assert!(!array(&source["profiles"]).is_empty());
        for profile in array(&source["profiles"]) {
            text(profile);
        }
        let installed = &source["installed_template"];
        if !installed.is_null() {
            keys(installed, &["cab_member", "sha256"]);
            hash(&installed["sha256"]);
            text(&installed["cab_member"]);
        }
        for field in ["imports", "inclusion_directives", "includes"] {
            anchored_literals(&source[field]);
        }
    }
    let mut owners = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut declarations = 0;
    let mut references = 0;
    let mut parameters = 0;
    let mut defaults = 0;
    let mut kinds = [0_u64; 3];
    let mut languages = [0_u64; 3];
    for owner in array(&v["owners"]) {
        keys(
            owner,
            &[
                "research_id",
                "documented_name",
                "language",
                "source_id",
                "namespace_literal",
                "source_name",
                "exact_name_match",
                "relationship",
                "class_fact",
                "members",
                "fields",
                "properties",
                "accessors",
                "nested_types",
                "member_conditions",
            ],
        );
        assert!(owners.insert(text(&owner["research_id"])));
        assert!(names.insert(text(&owner["source_name"])));
        assert!(text(&owner["research_id"]).starts_with(PREFIX));
        assert!(source_ids.contains(text(&owner["source_id"])));
        let language = text(&owner["language"]);
        languages[match language {
            "C++" => 0,
            "C#" => 1,
            "Java" => 2,
            _ => panic!("language"),
        }] += 1;
        let name = text(&owner["source_name"])
            .rsplit(if language == "C++" { "::" } else { "." })
            .next()
            .unwrap();
        assert_eq!(
            owner["source_name"],
            format!(
                "{}{}{name}",
                text(&owner["namespace_literal"]),
                if language == "C++" { "::" } else { "." }
            )
        );
        let exact = owner["documented_name"] == owner["source_name"];
        assert_eq!(owner["exact_name_match"], exact);
        assert_eq!(
            owner["relationship"],
            if exact {
                "exact_name_match"
            } else {
                "source_documentation_correspondence_not_alias"
            }
        );
        let class = &owner["class_fact"];
        keys(
            class,
            &[
                "source_lines",
                "declaration",
                "base_literal",
                "base_access",
                "modifiers",
                "attributes",
                "observation_ref",
            ],
        );
        lines(&class["source_lines"]);
        for field in [
            "declaration",
            "base_literal",
            "base_access",
            "observation_ref",
        ] {
            nullable_text(&class[field]);
        }
        anchored_literals(&class["attributes"]);
        for modifier in array(&class["modifiers"]) {
            text(modifier);
        }
        if class["observation_ref"].is_null() {
            assert!(!text(&class["declaration"]).contains(['{', '}', ';', '=']));
            assert!(!text(&class["base_literal"]).is_empty());
        } else {
            assert!(
                class["declaration"].is_null()
                    && class["base_literal"].is_null()
                    && class["base_access"].is_null()
            );
            empty(&class["modifiers"]);
            empty(&class["attributes"]);
        }
        for field in [
            "fields",
            "properties",
            "accessors",
            "nested_types",
            "member_conditions",
        ] {
            empty(&owner[field]);
        }
        for (ordinal, member) in array(&owner["members"]).iter().enumerate() {
            assert_eq!(count(&member["ordinal"]), ordinal as u64);
            lines(&member["source_lines"]);
            if member.get("observation_ref").is_some() {
                keys(
                    member,
                    &["ordinal", "name", "source_lines", "observation_ref"],
                );
                text(&member["observation_ref"]);
                text(&member["name"]);
                references += 1;
                kinds[2] += 1;
                continue;
            }
            keys(
                member,
                &[
                    "ordinal",
                    "kind",
                    "name",
                    "access",
                    "declaration",
                    "return_type_literal",
                    "modifiers",
                    "qualifiers",
                    "parameters",
                    "java_throws",
                    "attributes",
                    "conditions",
                    "source_lines",
                ],
            );
            assert_eq!(member["access"], "public");
            nullable_text(&member["return_type_literal"]);
            for field in ["java_throws", "attributes", "conditions"] {
                empty(&member[field]);
            }
            for field in ["modifiers", "qualifiers"] {
                for item in array(&member[field]) {
                    text(item);
                }
            }
            let kind = text(&member["kind"]);
            kinds[match kind {
                "constructor" => 0,
                "destructor" => 1,
                "method" => 2,
                _ => panic!("kind"),
            }] += 1;
            if kind == "constructor" {
                assert_eq!(member["name"], name);
            }
            if kind == "destructor" {
                assert_eq!(member["name"], format!("~{name}"));
            }
            assert_eq!(member["return_type_literal"].is_null(), kind != "method");
            let mut parameter_names = BTreeSet::new();
            let mut params = Vec::new();
            for (index, param) in array(&member["parameters"]).iter().enumerate() {
                keys(
                    param,
                    &[
                        "index",
                        "name",
                        "type_literal",
                        "default_literal",
                        "modifiers",
                        "attributes",
                    ],
                );
                assert_eq!(count(&param["index"]), index as u64);
                assert!(parameter_names.insert(text(&param["name"])));
                nullable_text(&param["default_literal"]);
                empty(&param["modifiers"]);
                empty(&param["attributes"]);
                parameters += 1;
                defaults += u64::from(!param["default_literal"].is_null());
                params.push(format!(
                    "{} {}{}",
                    text(&param["type_literal"]),
                    text(&param["name"]),
                    param["default_literal"]
                        .as_str()
                        .map(|s| format!(" = {s}"))
                        .unwrap_or_default()
                ));
            }
            // Reconstruct declarations from independently checked parts. This
            // prevents extra implementation text hiding in a redundant string.
            let prefix = array(&member["modifiers"])
                .iter()
                .chain(
                    member["return_type_literal"]
                        .as_str()
                        .map(|_| &member["return_type_literal"]),
                )
                .map(text)
                .collect::<Vec<_>>()
                .join(" ");
            let suffix = array(&member["qualifiers"])
                .iter()
                .map(text)
                .collect::<Vec<_>>()
                .join(" ");
            let expected = format!(
                "{}{}({}){}",
                if prefix.is_empty() {
                    String::new()
                } else {
                    prefix + " "
                },
                text(&member["name"]),
                params.join(", "),
                if suffix.is_empty() {
                    suffix
                } else {
                    " ".to_owned() + &suffix
                }
            );
            assert_eq!(member["declaration"], expected);
            assert!(!expected.contains(['{', '}', ';']));
            declarations += 1;
        }
    }
    let s = &v["scope"];
    keys(
        s,
        &[
            "owner_count",
            "language_owner_counts",
            "direct_public_callables",
            "new_declarations",
            "referenced_declarations",
            "constructors",
            "destructors",
            "methods",
            "parameters",
            "default_arguments",
            "excluded_platform_references",
            "excluded_other_public_owners",
            "selection",
        ],
    );
    assert_eq!(count(&s["owner_count"]), owners.len() as u64);
    assert_eq!(count(&s["new_declarations"]), declarations);
    assert_eq!(count(&s["referenced_declarations"]), references);
    assert_eq!(
        count(&s["direct_public_callables"]),
        declarations + references
    );
    for (field, actual) in ["constructors", "destructors", "methods"]
        .into_iter()
        .zip(kinds)
    {
        assert_eq!(count(&s[field]), actual);
    }
    assert_eq!(count(&s["parameters"]), parameters);
    assert_eq!(count(&s["default_arguments"]), defaults);
    keys(&s["language_owner_counts"], &["C++", "C#", "Java"]);
    for (field, actual) in ["C++", "C#", "Java"].into_iter().zip(languages) {
        assert_eq!(count(&s["language_owner_counts"][field]), actual);
    }
    for alias in array(&v["aliases"]) {
        keys(
            alias,
            &[
                "source_id",
                "name",
                "alternatives",
                "endif_line",
                "inclusion_guard",
                "scope",
            ],
        );
        assert!(source_ids.contains(text(&alias["source_id"])));
        for alt in array(&alias["alternatives"]) {
            keys(
                alt,
                &[
                    "condition_literal",
                    "condition_line",
                    "type_literal",
                    "line",
                ],
            );
            assert!(count(&alt["condition_line"]) < count(&alt["line"]));
            assert!(count(&alt["line"]) < count(&alias["endif_line"]));
        }
    }
    keys(
        &v["qualification"],
        &[
            "binding",
            "abi",
            "ownership",
            "lifetime",
            "returned_message",
            "inner_ownership",
            "runtime_errors",
            "defaults",
            "dependencies",
            "language_versions",
            "authenticode_trust",
            "rule",
        ],
    );
    for (key, value) in v["qualification"].as_object().unwrap() {
        if key != "rule" {
            assert_eq!(value, "unknown");
        }
    }
    keys(&v["method"], &["oracle", "normalization", "execution"]);
}

fn tuples(v: &Value) -> Vec<Value> {
    let anchored = |rows: &Value| {
        array(rows)
            .iter()
            .map(|r| json!([r["literal"], r["line"]]))
            .collect::<Vec<_>>()
    };
    let mut result = Vec::new();
    for s in array(&v["sources"]) {
        result.push(json!([
            "S",
            s["id"],
            s["profiles"],
            s["path"],
            s["bytes"],
            s["sha256"],
            s["installed_template"]["cab_member"],
            s["installed_template"]["sha256"],
            anchored(&s["imports"]),
            anchored(&s["inclusion_directives"]),
            anchored(&s["includes"])
        ]));
    }
    for o in array(&v["owners"]) {
        let id = text(&o["research_id"]).strip_prefix(PREFIX).unwrap();
        let c = &o["class_fact"];
        result.push(json!([
            "O",
            id,
            o["documented_name"],
            o["language"],
            o["source_id"],
            o["namespace_literal"],
            o["source_name"],
            o["exact_name_match"],
            c["source_lines"],
            c["declaration"],
            c["base_literal"],
            c["base_access"],
            c["modifiers"],
            anchored(&c["attributes"]),
            c["observation_ref"],
            o["fields"],
            o["properties"],
            o["accessors"],
            o["nested_types"],
            o["member_conditions"]
        ]));
        for m in array(&o["members"]) {
            if m.get("observation_ref").is_some() {
                result.push(json!([
                    "R",
                    id,
                    m["ordinal"],
                    m["name"],
                    m["source_lines"],
                    m["observation_ref"]
                ]));
            } else {
                let params: Vec<_> = array(&m["parameters"])
                    .iter()
                    .map(|p| json!([p["name"], p["type_literal"], p["default_literal"]]))
                    .collect();
                result.push(json!([
                    "M",
                    id,
                    m["ordinal"],
                    m["kind"],
                    m["name"],
                    m["return_type_literal"],
                    m["modifiers"],
                    m["qualifiers"],
                    params,
                    m["source_lines"],
                    m["declaration"]
                ]));
            }
        }
    }
    for a in array(&v["aliases"]) {
        let alternatives: Vec<_> = array(&a["alternatives"])
            .iter()
            .map(|r| {
                json!([
                    r["condition_literal"],
                    r["condition_line"],
                    r["type_literal"],
                    r["line"]
                ])
            })
            .collect();
        result.push(json!([
            "A",
            a["source_id"],
            a["name"],
            alternatives,
            a["endif_line"],
            a["inclusion_guard"]
        ]));
    }
    result
}

fn frozen(v: &Value) {
    let expected: Vec<Value> = include_str!("oracles/supporting-exception-declarations.jsonl")
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    let actual = tuples(v);
    assert_eq!(actual.len(), expected.len(), "exact finite source cohort");
    for (i, (actual, expected)) in actual.iter().zip(&expected).enumerate() {
        assert_eq!(actual, expected, "independent source tuple {i}");
    }
}

fn joins(v: &Value, supporting: &Value, artifacts: &Value, wrappers: &Value) {
    let docs = array(&supporting["vendor_exception_classes"]);
    assert_eq!(array(&v["owners"]).len(), docs.len());
    for (owner, doc) in array(&v["owners"]).iter().zip(docs) {
        assert_eq!(owner["research_id"], doc["research_id"]);
        assert_eq!(owner["documented_name"], doc["name"]);
        assert_eq!(owner["language"], doc["language"]);
        let source = array(&v["sources"])
            .iter()
            .find(|s| s["id"] == owner["source_id"])
            .unwrap();
        for fact in std::iter::once(&owner["class_fact"]).chain(array(&owner["members"])) {
            if let Some(id) = fact["observation_ref"].as_str() {
                let observation = array(&artifacts["installed_declarations"])
                    .iter()
                    .find(|r| r["id"] == id)
                    .unwrap();
                assert_eq!(
                    observation["cab_member"],
                    source["installed_template"]["cab_member"]
                );
                assert_eq!(
                    observation["sha256"],
                    source["installed_template"]["sha256"]
                );
                assert_eq!(observation["language"], owner["language"]);
                match id {
                    "installed.cpp.exception-inner" => {
                        assert!(array(&observation["types"]).contains(&owner["source_name"]));
                        assert_eq!(observation["public_member"], "inner");
                        assert_eq!(observation["return_type"], "const std::exception*");
                        assert_eq!(observation["const_member"], true);
                        assert_eq!(fact["name"], "inner");
                    }
                    "installed.java.source-instance-unavailable" => {
                        assert_eq!(observation["public_type"], owner["source_name"]);
                        assert_eq!(observation["declared_base"], "RuntimeException");
                    }
                    _ => panic!("unadopted source reference"),
                }
            }
        }
    }
    let p = &v["provenance"];
    assert_eq!(p["release_ref"], "artifact-observations.json#/release");
    assert_eq!(p["wrapper_ref"], "schema-wrappers.json");
    assert_eq!(
        p["generation_manifest"],
        "schema-wrapper-v1/generation-manifest.json"
    );
    assert_eq!(
        p["generation_manifest_sha256"],
        "1101c782bb8056df2a82cbba777af11de0aeebc813c889deb49ad25687d5f69d"
    );
    assert_eq!(
        p["generation_manifest_sha256"],
        wrappers["generation"]["private_manifest_sha256"]
    );
    assert_eq!(p["generator_version"], "28.2.0.0");
    assert_eq!(
        p["generator_version"],
        artifacts["release"]["executable_version"]
    );
    assert_eq!(
        p["generator_sha256"],
        artifacts["matched_members"][0]["sha256"]
    );
    assert_eq!(
        p["generator_sha256"],
        wrappers["release"]["generator_sha256"]
    );
    assert_eq!(array(&p["inputs"]).len(), 4);
    let input_names: Vec<_> = array(&p["inputs"])
        .iter()
        .map(|r| text(&r["name"]))
        .collect();
    assert_eq!(
        input_names,
        [
            "ConvertProducts.mfd",
            "Products.xsd",
            "Products.xml",
            "ProductValuePairs.xsd"
        ]
    );
    for input in array(&p["inputs"]) {
        let member = array(&artifacts["matched_members"])
            .iter()
            .find(|r| r["name"] == input["artifact_name"])
            .unwrap();
        assert_eq!(input["sha256"], member["sha256"]);
        assert_eq!(input["bytes"], member["bytes"]);
    }
    let platform: Vec<_> = array(&supporting["platform_exception_references"])
        .iter()
        .map(|r| r["research_id"].clone())
        .collect();
    assert_eq!(v["scope"]["excluded_platform_references"], json!(platform));
    assert_eq!(
        v["scope"]["excluded_other_public_owners"],
        json!([
            "altova::TargetUpdateFailureException",
            "altova::UserException",
            "Altova.Types.TargetUpdateFailureException"
        ])
    );
    assert_eq!(
        v["manual_claim_refs"],
        json!([
            "supporting.json#/vendor_exception_classes",
            "supporting.json#/shared_cpp_exception_methods"
        ])
    );
    // Source-level differences never rewrite the manual's literal claims.
    assert_eq!(
        supporting["shared_cpp_exception_methods"][0]["literal_signature"],
        "string_type message()"
    );
    assert_eq!(
        supporting["shared_cpp_exception_methods"][1]["literal_signature"],
        "std::exception inner()"
    );
    for (index, manual, source) in [
        (12, "System.Exception", "Exception"),
        (13, "System.Exception", "Exception"),
        (14, "Exception", "RuntimeException"),
        (15, "Exception", "RuntimeException"),
    ] {
        assert_eq!(docs[index]["base_class_literal"], manual);
        let class = &v["owners"][index]["class_fact"];
        let source_base = if let Some(id) = class["observation_ref"].as_str() {
            &array(&artifacts["installed_declarations"])
                .iter()
                .find(|row| row["id"] == id)
                .unwrap()["declared_base"]
        } else {
            &class["base_literal"]
        };
        assert_eq!(source_base, source);
        assert_ne!(source_base, &docs[index]["base_class_literal"]);
    }
}

fn boundaries(v: &Value) {
    assert_eq!(
        v["purpose"],
        "Direct-public source declarations for the 16 documented vendor exception owners; enrichment of existing research IDs, not capabilities or execution qualification."
    );
    assert_eq!(
        v["provenance"]["source_binding"],
        "HTTPS acquisition and byte identity only; selected generated sources match the retained release-bound manifest and installed templates. Authenticode integrity, timestamp and trust are unverified."
    );
    assert_eq!(
        v["scope"]["selection"],
        "Only the documented vendor-owner cohort. Other public owners are outside this finite scope, not nonexistent or private. Inherited and compiler-implicit members, private members, implementation bodies, constructor initializers and comment examples are excluded."
    );
    assert_eq!(
        v["aliases"][0]["scope"],
        "Global typedef declaration alternatives only; no compiled configuration selected."
    );
    assert_eq!(
        v["qualification"]["rule"],
        "Literal bases, imports, throw(), const, empty Java throws and default expressions are source facts only. No name binding, platform guarantee, runtime error absence, returned-message behavior or compatibility alias is established; manual claims remain separate."
    );
    assert_eq!(
        v["method"]["oracle"],
        "Manually transcribed from actual retained source files before the separate candidate extractor; not derived from readiness outlines or candidate JSON."
    );
    assert_eq!(
        v["method"]["normalization"],
        "Whitespace only: collapse whitespace, attach pointer/reference punctuation and parentheses, retain source token spelling and declaration order. Bodies, initializer lists, comments and examples are omitted. One-based LF source lines refer to the exact hashed generated bytes."
    );
    assert_eq!(
        v["method"]["execution"],
        "No vendor compilation, generation, program execution, COM activation or network access is performed by this inventory or its tests."
    );
}

fn validate(v: &Value) {
    structure(v);
    frozen(v);
    joins(
        v,
        &load("supporting.json"),
        &load("artifact-observations.json"),
        &load("schema-wrappers.json"),
    );
    boundaries(v);
}

#[test]
fn all_documented_exception_owners_join_independent_source_declarations() {
    let v = load(FILE);
    validate(&v);
    for (field, expected) in [
        ("owner_count", 16),
        ("direct_public_callables", 30),
        ("new_declarations", 28),
        ("referenced_declarations", 2),
        ("constructors", 24),
        ("destructors", 2),
        ("methods", 4),
        ("parameters", 43),
        ("default_arguments", 9),
    ] {
        assert_eq!(v["scope"][field], expected);
    }
    assert_eq!(
        array(&v["owners"])
            .iter()
            .filter(|o| o["exact_name_match"] == false)
            .count(),
        6
    );
}
