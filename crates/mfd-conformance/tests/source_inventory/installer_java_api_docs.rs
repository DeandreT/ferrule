//! Exact offline documentation facts, not a Java implementation or runtime contract.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::load;

#[path = "installer_java_api_docs/mutations.rs"]
mod mutations;

#[path = "installer_java_api_docs/tensions.rs"]
mod tensions;

const DECLARATIONS: &str = include_str!(
    "../../../../conformance/mfd-2026r2-source-inventory/oracles/installer-java-api-docs.jsonl"
);
const METADATA: &str = include_str!(
    "../../../../conformance/mfd-2026r2-source-inventory/oracles/installer-java-api-docs-metadata.json"
);
const CLAIMS: &str = include_str!(
    "../../../../conformance/mfd-2026r2-source-inventory/oracles/installer-java-api-docs-claims.json"
);

fn require(ok: bool, context: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(context.to_owned()) }
}

fn array(value: &Value) -> Result<&[Value], String> {
    value.as_array().map(Vec::as_slice).ok_or("array".into())
}

fn string(value: &Value) -> Result<&str, String> {
    value.as_str().ok_or("string".into())
}

fn number(value: &Value) -> Result<u64, String> {
    value.as_u64().ok_or("nonnegative integer".into())
}

fn hash(value: &Value) -> Result<(), String> {
    let s = string(value)?;
    require(
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "SHA-256",
    )
}

fn path(value: &Value) -> Result<(), String> {
    let s = string(value)?;
    require(
        !s.is_empty()
            && !s.starts_with('/')
            && !s.contains(['\\', ':'])
            && !s.split('/').any(|part| part == ".." || part == "."),
        "relative archive path",
    )
}

fn anchor(value: &Value, bound: u64) -> Result<(), String> {
    let a = array(value)?;
    require(a.len() == 2, "line-range shape")?;
    require(
        number(&a[0])? > 0 && number(&a[0])? <= number(&a[1])? && number(&a[1])? <= bound,
        "source line range",
    )
}

/// Compare one small record at a time, including shape, nulls and unknown fields.
/// Never format the entire 655-member inventory in a failure or caught panic.
fn exact_rows(actual: &Value, expected: &Value, context: &str) -> Result<(), String> {
    let (actual, expected) = (array(actual)?, array(expected)?);
    require(actual.len() == expected.len(), &format!("{context} length"))?;
    for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
        require(a == e, &format!("{context} row {i}"))?;
    }
    Ok(())
}

fn validate(value: Value, manifest: Value) -> Result<(), String> {
    let expected: Value = serde_json::from_str(METADATA).map_err(|e| e.to_string())?;
    let expected_claims: Value = serde_json::from_str(CLAIMS).map_err(|e| e.to_string())?;
    let declaration_rows: Vec<Value> = DECLARATIONS
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;

    let mut metadata = value.clone();
    let fields = metadata.as_object_mut().ok_or("root object")?;
    for key in [
        "declarations",
        "claims",
        "indices",
        "source_manifest",
        "documentation_tensions",
    ] {
        require(fields.remove(key).is_some(), "required root field")?;
    }
    // The frozen metadata is also the strict schema for the finite editorial
    // profile: unknown fields and missing explicit nulls cannot disappear here.
    require(
        metadata == expected["metadata"],
        "strict metadata/profile oracle",
    )?;
    require(
        value["source_manifest"] == "installer-java-api-docs-sources.json",
        "manifest reference",
    )?;
    let manifest_fields = manifest.as_object().ok_or("manifest object")?;
    require(
        manifest_fields
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            == BTreeSet::from([
                "format",
                "format_version",
                "assessment_ref",
                "archive_sha256",
                "members",
            ])
            && manifest["format"]
                == "mapforce-enterprise-2026r2-installer-java-api-source-manifest"
            && manifest["format_version"] == 1
            && manifest["assessment_ref"] == "installer-java-api-docs.json#/"
            && manifest["archive_sha256"] == value["source"]["sha256"],
        "strict manifest schema",
    )?;
    exact_rows(
        &manifest["members"],
        &expected["manifest"],
        "source manifest",
    )?;
    exact_rows(&value["indices"], &expected["indices"], "index oracle")?;
    exact_rows(&value["claims"], &expected_claims, "authored claim oracle")?;
    exact_rows(
        &value["declarations"],
        &json!(declaration_rows),
        "independent declaration oracle",
    )?;

    let mut sources = BTreeMap::new();
    let mut roles = BTreeMap::new();
    for source in array(&manifest["members"])? {
        let row = array(source)?;
        require(row.len() == 5, "manifest tuple")?;
        path(&row[0])?;
        hash(&row[2])?;
        require(
            sources.insert(string(&row[0])?, row).is_none(),
            "duplicate source",
        )?;
        *roles.entry(string(&row[4])?).or_insert(0) += 1;
    }
    require(
        sources.len() == 141
            && roles.get("documented_type") == Some(&75)
            && roles.get("class_index") == Some(&1)
            && roles.get("package_index") == Some(&2),
        "complete source cohort",
    )?;

    let mut types = BTreeMap::new();
    let mut members = BTreeMap::new();
    let mut kinds = BTreeMap::new();
    let mut counts = BTreeMap::new();
    let mut package_counts: BTreeMap<&str, [u64; 5]> = BTreeMap::new();
    let mut excluded = BTreeMap::<&str, BTreeSet<&str>>::new();
    let mut indices = BTreeMap::<&str, BTreeSet<&str>>::new();
    let mut current = None;
    let mut ordinal = 0;
    let mut constants = 0;
    let mut nested = 0;
    for declaration in array(&value["declarations"])? {
        let row = array(declaration)?;
        let owner = string(&row[1])?;
        if row[0] == "T" {
            require(row.len() == 10, "type tuple")?;
            let page = string(&row[2])?;
            require(
                types.insert(owner, row).is_none()
                    && sources[page][4] == "documented_type"
                    && owner == page.trim_end_matches(".html").replace('/', "."),
                "type source/identity",
            )?;
            if !row[4].is_null() {
                nested += 1;
            }
            *kinds.entry(string(&row[3])?).or_insert(0) += 1;
            let package = if owner.contains(".MapForce.") {
                "com.altova.automation.MapForce"
            } else {
                "com.altova.automation.libs"
            };
            package_counts.entry(package).or_default()[0] += 1;
            let bound = number(&sources[page][3])?;
            anchor(&row[6], bound)?;
            for ranges in [&row[8], &row[9]] {
                for range in array(ranges)? {
                    anchor(range, bound)?;
                }
            }
            current = Some(owner);
            ordinal = 0;
            continue;
        }
        require(row.len() == 22, "member tuple")?;
        require(
            current == Some(owner) && number(&row[2])? == ordinal,
            "direct owner/order closure",
        )?;
        ordinal += 1;
        let ty = types.get(owner).ok_or("unknown member owner")?;
        let source = sources[string(&ty[2])?];
        let bound = number(&source[3])?;
        anchor(&row[8], bound)?;
        require(
            number(&row[7])? > 0 && number(&row[7])? < number(&row[8][0])?,
            "summary/detail reciprocal anchor",
        )?;
        for ranges in [&row[20], &row[21]] {
            for range in array(ranges)? {
                anchor(range, bound)?;
            }
        }
        let fragment = string(&row[5])?;
        require(
            members.insert((owner, fragment), row).is_none(),
            "duplicate direct member/overload",
        )?;
        for (i, parameter) in array(&row[13])?.iter().enumerate() {
            let parameter = array(parameter)?;
            require(
                parameter.len() == 3 && number(&parameter[0])? == i as u64,
                "ordered parameter schema",
            )?;
            string(&parameter[1])?;
            if !parameter[2].is_null() {
                string(&parameter[2])?;
            }
        }
        if row[0] == "X" {
            require(
                ty[3] == "enum"
                    && row[3] == "method"
                    && ["values()", "valueOf(java.lang.String)"].contains(&fragment),
                "standard helper exclusion",
            )?;
            excluded.entry(owner).or_default().insert(fragment);
            continue;
        }
        require(row[0] == "M", "declaration tag")?;
        let kind = string(&row[3])?;
        *counts.entry(kind).or_insert(0) += 1;
        let package = if owner.contains(".MapForce.") {
            "com.altova.automation.MapForce"
        } else {
            "com.altova.automation.libs"
        };
        let offset = match kind {
            "constructor" => 1,
            "method" => 2,
            "field" => 3,
            "enum_name" => 4,
            _ => return Err("member kind".into()),
        };
        package_counts.entry(package).or_default()[offset] += 1;
        require(
            (kind == "constructor") == row[12].is_null(),
            "constructor/return boundary",
        )?;
        match kind {
            "enum_name" => require(
                row[16].is_null() && row[17] == "not_documented" && row[18].is_null(),
                "no inferred enum value",
            )?,
            "field" if row[17] == "documented_literal" => {
                constants += 1;
                string(&row[16])?;
                anchor(&row[18], number(&sources["constant-values.html"][3])?)?;
            }
            "field" => require(
                row[16].is_null() && row[17] == "not_documented" && row[18].is_null(),
                "unstated field initializer",
            )?,
            _ => require(
                row[16].is_null() && row[17] == "not_applicable" && row[18].is_null(),
                "non-value declaration",
            )?,
        }
    }
    require(
        types.len() == 75
            && members.len() == 695
            && nested == 31
            && constants == 28
            && kinds == BTreeMap::from([("class", 48), ("interface", 7), ("enum", 20)])
            && counts
                == BTreeMap::from([
                    ("constructor", 29),
                    ("method", 467),
                    ("field", 29),
                    ("enum_name", 130),
                ])
            && package_counts
                == BTreeMap::from([
                    ("com.altova.automation.libs", [24, 23, 71, 29, 0]),
                    ("com.altova.automation.MapForce", [51, 6, 396, 0, 130]),
                ]),
        "finite documentation partition",
    )?;
    require(excluded.len() == 20, "enum helper owner closure")?;
    for (owner, row) in &types {
        if row[3] == "enum" {
            require(
                excluded.get(owner)
                    == Some(&BTreeSet::from(["values()", "valueOf(java.lang.String)"])),
                "two excluded helpers per enum",
            )?;
        }
        if let Some(enclosing) = row[4].as_str() {
            require(types.contains_key(enclosing), "nested owner closure")?;
        }
    }
    for index in array(&value["indices"])? {
        let row = array(index)?;
        require(row.len() == 3, "index tuple")?;
        let page = string(&row[0])?;
        require(
            number(&row[1])? > 0 && number(&row[1])? <= number(&sources[page][3])?,
            "index line anchor",
        )?;
        require(
            indices.entry(page).or_default().insert(string(&row[2])?),
            "duplicate index target",
        )?;
    }
    let all_pages: BTreeSet<_> = types.values().map(|r| r[2].as_str().unwrap()).collect();
    require(
        indices["allclasses-index.html"] == all_pages
            && indices["com/altova/automation/MapForce/package-summary.html"]
                .union(&indices["com/altova/automation/libs/package-summary.html"])
                .copied()
                .collect::<BTreeSet<_>>()
                == all_pages
            && indices["com/altova/automation/MapForce/package-summary.html"]
                .is_disjoint(&indices["com/altova/automation/libs/package-summary.html"]),
        "class/package/type reciprocal closure",
    )?;
    for claim in array(&value["claims"])? {
        let row = array(claim)?;
        require(row.len() == 5, "claim tuple")?;
        let owner = string(&row[0])?;
        let ty = types.get(owner).ok_or("claim owner")?;
        let declaration = if row[1].is_null() {
            *ty
        } else {
            members
                .get(&(owner, string(&row[1])?))
                .copied()
                .ok_or("claim member")?
        };
        let (notes, labels) = if declaration[0] == "T" {
            (&declaration[8], &declaration[9])
        } else {
            (&declaration[20], &declaration[21])
        };
        let starts: BTreeSet<_> = array(notes)?
            .iter()
            .chain(array(labels)?)
            .map(|r| r[0].as_u64().unwrap())
            .collect();
        for line in array(&row[4])? {
            require(starts.contains(&number(line)?), "claim note/label anchor")?;
        }
    }
    validate_comparisons(&value, &members)?;
    tensions::validate(&value, &types, &members, &sources)?;
    Ok(())
}

fn validate_comparisons(
    value: &Value,
    members: &BTreeMap<(&str, &str), &[Value]>,
) -> Result<(), String> {
    let artifacts = load("artifact-observations.json");
    require(
        value["source"]["archive_sha256"] == artifacts["release"]["archive_sha256"],
        "existing release archive join",
    )?;
    for (i, link) in array(&value["source"]["container_chain"])?
        .iter()
        .enumerate()
    {
        require(
            link[0] == artifacts["release"]["container_chain"][i]["member"]
                && link[1] == artifacts["release"]["container_chain"][i]["sha256"],
            "existing release chain join",
        )?;
    }
    let tlb = load("com-enum-values.json");
    let online = load("com-api.json");
    require(
        online["release_binding"] == "unestablished",
        "online binding",
    )?;
    let mut tlb_names = BTreeSet::new();
    for enumeration in array(&tlb["enumerations"])? {
        for member in array(&enumeration["members"])? {
            tlb_names.insert((
                string(&enumeration["name"])?.to_owned(),
                string(&member["name"])?.to_owned(),
            ));
        }
    }
    let documented_names: BTreeSet<_> = members
        .values()
        .filter(|m| m[3] == "enum_name")
        .map(|m| {
            (
                m[1].as_str()
                    .unwrap()
                    .rsplit('.')
                    .next()
                    .unwrap()
                    .to_owned(),
                m[4].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    require(
        documented_names.len() == 130 && documented_names == tlb_names,
        "unqualified enum name join",
    )?;
    let prior = &artifacts["installed_declarations"][3];
    let discrepancy = &value["comparisons"]["online_count_discrepancy"];
    require(
        discrepancy["online_literal"] == prior["catalog_comparison"]["count_literal"]
            && discrepancy["tlb_literal"]
                .as_str()
                .and_then(|literal| literal.parse::<i64>().ok())
                == prior["members"][2]["value"].as_i64()
            && discrepancy["javadoc_literal"].is_null()
            && discrepancy["resolution"] == "unresolved",
        "preserved online/TLB discrepancy",
    )?;
    Ok(())
}

#[test]
fn installer_java_documentation_has_exact_offline_source_and_declaration_closure() {
    validate(
        load("installer-java-api-docs.json"),
        load("installer-java-api-docs-sources.json"),
    )
    .unwrap();
    for (source, expected) in [
        (
            DECLARATIONS,
            "21b279d2fc880b936b43439432a765addeb72bf3d6129ec9415c871cffc545fb",
        ),
        (
            METADATA,
            "0a6a378fe43d62d0fab37af941dd1a9dc059d62271e8112cde2c49df57d438e6",
        ),
        (
            CLAIMS,
            "d34f6df10a8fb379139ff92aa77220b53fa2bfb7b7d90a8002e2ff6bfecb7cac",
        ),
    ] {
        assert_eq!(format!("{:x}", Sha256::digest(source)), expected);
    }
}
