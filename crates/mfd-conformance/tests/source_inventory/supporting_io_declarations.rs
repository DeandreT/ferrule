//! Offline public declaration checks; neither vendor source nor runtime is used.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::load;

#[path = "supporting_io_declarations/schema.rs"]
mod schema;
use schema::{Inventory, Member, Type};

#[path = "supporting_io_declarations/mutations.rs"]
mod mutations;

const ORACLE: &str = include_str!(
    "../../../../conformance/mfd-2026r2-source-inventory/oracles/supporting-io-declarations.jsonl"
);

fn type_tuple(ty: &Type) -> Value {
    json!([ty.spelling, ty.resolved])
}

fn declaration_rows(inventory: &Inventory) -> Vec<Value> {
    let mut rows = Vec::new();
    for source in &inventory.sources {
        rows.push(json!([
            "S",
            source.index,
            source.language,
            source.path,
            source.bytes,
            source.sha256,
            source.lf_lines,
            source
                .imports
                .iter()
                .map(|i| json!([i.name, i.declaration_lines]))
                .collect::<Vec<_>>(),
            source.conditions
        ]));
    }
    for owner in &inventory.owners {
        rows.push(json!([
            "O",
            owner.name,
            owner.language,
            owner.source_index,
            owner.source_type_index,
            owner.declaration_lines,
            owner.modifiers,
            owner.extends.iter().map(type_tuple).collect::<Vec<_>>(),
            owner.implements.iter().map(type_tuple).collect::<Vec<_>>(),
            owner.kind
        ]));
        for nested in &owner.nested_types {
            rows.push(json!([
                "N",
                owner.name,
                nested.name,
                nested.declaration_lines,
                nested.modifiers,
                nested.kind,
                nested.underlying_type.0.as_ref().map(type_tuple),
                nested
                    .constants
                    .iter()
                    .map(|c| json!([c.name, c.value, c.declaration_lines]))
                    .collect::<Vec<_>>(),
                nested.conditions
            ]));
        }
        for member in &owner.members {
            rows.push(json!([
                "M",
                owner.name,
                member.index,
                member.declaration_lines,
                member.kind,
                member.name,
                member.r#type.0.as_ref().map(type_tuple),
                member.modifiers,
                member
                    .parameters
                    .iter()
                    .map(|p| json!([
                        p.index,
                        p.name,
                        type_tuple(&p.r#type),
                        p.direction,
                        p.default.0
                    ]))
                    .collect::<Vec<_>>(),
                member.throws.iter().map(type_tuple).collect::<Vec<_>>(),
                member
                    .accessors
                    .iter()
                    .map(|a| json!([a.name, a.visibility, a.declaration_lines]))
                    .collect::<Vec<_>>(),
                member.annotations,
                member.constant_value.0,
                member.directly_declared,
                member.conditions,
                member.value_status
            ]));
        }
    }
    rows
}

fn anchor(range: [u32; 2], bound: u32) -> Result<(), String> {
    require(
        range[0] > 0 && range[0] <= range[1] && range[1] <= bound,
        "source anchor",
    )
}

fn require(ok: bool, context: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(context.to_owned()) }
}

fn overload_key(member: &Member) -> String {
    format!(
        "{}:{}({})",
        member.kind,
        member.name,
        member
            .parameters
            .iter()
            .map(|p| format!("{}:{}", p.direction, p.r#type.resolved))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn validate(value: Value) -> Result<Inventory, String> {
    let inventory: Inventory = serde_json::from_value(value).map_err(|e| e.to_string())?;
    require(
        inventory.format == "mapforce-enterprise-2026r2-supporting-io-declarations"
            && inventory.format_version == 1,
        "format",
    )?;
    require(
        !inventory.inventory_complete
            && !inventory.runtime_proof
            && inventory.status == "unassessed"
            && inventory.applicability == "unknown"
            && inventory.execution_evidence.is_empty()
            && inventory.execution_receipts.is_empty(),
        "root assessment",
    )?;
    let assessment = &inventory.assessment_boundary;
    require(
        assessment.status == "unassessed"
            && assessment.applicability == "unknown"
            && !assessment.runtime_proof
            && assessment.execution_evidence.is_empty()
            && assessment.execution_receipts.is_empty(),
        "cohort assessment",
    )?;
    let scope = &inventory.scope;
    require(
        scope.owner_count == 24
            && scope.source_count == 13
            && scope.direct_member_count == 99
            && scope.nested_type_count == 2
            && scope.enum_constant_count == 6
            && scope.inherited_members == "excluded"
            && scope.implicit_members == "excluded"
            && !scope.implementation_copied,
        "finite scope",
    )?;
    require(
        scope.excluded
            == [
                "UrlInput",
                "Web",
                "mapping-entry APIs",
                "close-setting APIs",
                "exception owners",
                "schema-generated types",
                "nonpublic members",
                "implementation bodies",
            ],
        "exclusions",
    )?;
    require(
        scope.unqualified
            == [
                "ownership",
                "lifetime",
                "encoding and buffering",
                "null and error behavior",
                "runtime defaults",
                "exact Java version",
                "dependency availability",
                "Authenticode trust",
                "ABI",
                "backend applicability",
            ],
        "qualification boundary",
    )?;
    validate_provenance(&inventory)?;
    let supporting = load("supporting.json");
    let documented = supporting["io_classes"]
        .as_array()
        .ok_or("documented owners")?;
    require(
        inventory.owners.len() == documented.len() && inventory.sources.len() == 13,
        "source/owner coverage",
    )?;
    let mut identities = BTreeSet::new();
    let mut counts = BTreeMap::new();
    let mut nested_count = 0;
    let mut constant_count = 0;
    let mut prior_owner = None;
    for (i, source) in inventory.sources.iter().enumerate() {
        require(
            source.index == i
                && source.bytes > 0
                && source.lf_lines > 1
                && source.wrapper_byte_identical
                && source.sha256 == source.wrapper_sha256
                && source.conditions.is_empty(),
            "source identity",
        )?;
        require(
            source.sha256.len() == 64
                && source.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                && !source.path.starts_with('/')
                && !source.path.split('/').any(|p| p == ".."),
            "source hash/path",
        )?;
        for import in &source.imports {
            anchor(import.declaration_lines, source.lf_lines)?;
        }
    }
    for owner in &inventory.owners {
        require(identities.insert(&owner.research_id), "duplicate owner")?;
        let doc = documented
            .iter()
            .find(|d| d["research_id"] == owner.research_id)
            .ok_or("unregistered owner")?;
        require(
            doc["name"] == owner.name && doc["language"] == owner.language,
            "documented owner join",
        )?;
        let source = inventory
            .sources
            .get(owner.source_index)
            .ok_or("source index")?;
        require(
            owner.kind == "class"
                && owner.language == source.language
                && owner.assessment_ref == "#/assessment_boundary"
                && owner.conditions.is_empty(),
            "owner assessment/source",
        )?;
        anchor(owner.declaration_lines, source.lf_lines)?;
        let position = (owner.source_index, owner.declaration_lines[0]);
        require(
            prior_owner.is_none_or(|prior| prior < position),
            "owner source order",
        )?;
        prior_owner = Some(position);
        let mut signatures = BTreeSet::new();
        let mut prior_line = owner.declaration_lines[1];
        for (index, member) in owner.members.iter().enumerate() {
            require(
                member.index == index
                    && member.directly_declared
                    && member.conditions.is_empty()
                    && member.modifiers.iter().any(|m| m == "public"),
                "direct public membership",
            )?;
            anchor(member.declaration_lines, source.lf_lines)?;
            require(
                member.declaration_lines[0] > prior_line,
                "member declaration order",
            )?;
            prior_line = member.declaration_lines[1];
            require(
                signatures.insert(overload_key(member)),
                "duplicate overload",
            )?;
            *counts.entry(member.kind.as_str()).or_insert(0) += 1;
            require(
                member.kind == "constructor" || member.r#type.0.is_some(),
                "member type",
            )?;
            require(
                (member.kind == "constructor") == member.r#type.0.is_none(),
                "constructor type",
            )?;
            for (pi, parameter) in member.parameters.iter().enumerate() {
                require(
                    parameter.index == pi
                        && parameter.direction == "value"
                        && parameter.default.0.is_none(),
                    "ordered parameter declaration",
                )?;
            }
            require(
                (member.kind == "property") != member.accessors.is_empty(),
                "property accessors",
            )?;
            for accessor in &member.accessors {
                anchor(accessor.declaration_lines, source.lf_lines)?;
                require(
                    accessor.visibility == "public"
                        && accessor.name == "get"
                        && accessor.declaration_lines[0] >= member.declaration_lines[0],
                    "accessor declaration",
                )?;
            }
            require(
                (member.kind == "constant") == member.constant_value.0.is_some(),
                "constant declaration",
            )?;
            let value_status = match member.kind.as_str() {
                "constant" => "literal_constant",
                "field" => "not_established",
                _ => "not_applicable",
            };
            require(
                member.value_status == value_status,
                "field/constant value boundary",
            )?;
        }
        for nested in &owner.nested_types {
            nested_count += 1;
            constant_count += nested.constants.len();
            anchor(nested.declaration_lines, source.lf_lines)?;
            require(
                owner.language == "C#"
                    && nested.kind == "enum"
                    && nested.modifiers == ["public"]
                    && nested.underlying_type.0.is_none()
                    && nested.conditions.is_empty(),
                "nested declaration",
            )?;
            for constant in &nested.constants {
                anchor(constant.declaration_lines, source.lf_lines)?;
            }
        }
    }
    require(nested_count == 2 && constant_count == 6, "nested coverage")?;
    let c = &scope.member_counts;
    require(
        counts
            == BTreeMap::from([
                ("constructor", 26),
                ("method", 35),
                ("property", 31),
                ("field", 1),
                ("constant", 6),
            ])
            && (c.constructor, c.method, c.property, c.field, c.constant) == (26, 35, 31, 1, 6),
        "member counts",
    )?;
    let expected: Vec<Value> = ORACLE
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let actual = declaration_rows(&inventory);
    if actual != expected {
        let mismatch = actual.iter().zip(&expected).position(|(a, e)| a != e);
        return Err(format!(
            "independent source oracle mismatch at {mismatch:?}"
        ));
    }
    validate_comparisons(&inventory)?;
    Ok(inventory)
}

fn validate_provenance(inventory: &Inventory) -> Result<(), String> {
    let p = &inventory.provenance;
    let artifacts = load("artifact-observations.json");
    let wrappers = load("schema-wrappers.json");
    require(
        p.observation == "artifact-observations.json"
            && p.wrapper_observation == "schema-wrappers.json"
            && p.generator_version == artifacts["release"]["executable_version"]
            && p.generator_sha256 == artifacts["matched_members"][0]["sha256"]
            && p.generator_sha256 == wrappers["release"]["generator_sha256"]
            && p.wrapper_manifest_sha256 == wrappers["generation"]["private_manifest_sha256"]
            && p.ordinary_manifest_sha256
                == "db01c723a234bf515ddbeb2930f9ddb00277960eea80ade4fff28a6407d4da04"
            && p.release_comparison_sha256
                == "d67383470c8781e7997f3eb765b6b43bd69aea5925e3c044b5d8847328c53cfe"
            && !p.vendor_generation
            && !p.vendor_compilation
            && !p.vendor_execution,
        "release/generation provenance",
    )?;
    require(p.inputs.len() == 4, "input coverage")?;
    for (index, name, role) in [
        (0, "ConvertProducts.mfd", "design"),
        (1, "Products.xsd", "source schema"),
        (2, "Products.xml", "source instance"),
        (3, "ProductValuePairs.xsd", "target schema"),
    ] {
        let input = &p.inputs[index];
        let matched = artifacts["matched_members"]
            .as_array()
            .ok_or("installer members")?
            .iter()
            .find(|m| m["name"] == format!("Tutorial/{name}"))
            .ok_or("input member")?;
        require(
            input.name == name
                && input.role == role
                && input.bytes == matched["bytes"].as_u64().ok_or("input byte count")?
                && input.sha256 == matched["sha256"],
            "input binding",
        )?;
    }
    require(
        inventory.oracle.path == "oracles/supporting-io-declarations.jsonl"
            && inventory.oracle.method
                == "Independent manual transcription from numbered retained source reads and separate hash readback; no candidate JSON, cached discovery metadata or candidate lexer used to create expected rows.",
        "oracle independence",
    )?;
    Ok(())
}

fn constructor_types<'a>(inventory: &'a Inventory, owner: &str) -> Vec<Vec<&'a str>> {
    inventory
        .owners
        .iter()
        .find(|o| o.name == owner)
        .unwrap()
        .members
        .iter()
        .filter(|m| m.kind == "constructor")
        .map(|m| {
            m.parameters
                .iter()
                .map(|p| p.r#type.resolved.as_str())
                .collect()
        })
        .collect()
}

fn validate_comparisons(inventory: &Inventory) -> Result<(), String> {
    let artifacts = load("artifact-observations.json");
    for observation in artifacts["generated_source"]["public_declaration_observations"]
        .as_array()
        .ok_or("prior declarations")?
    {
        let owner = observation["type"].as_str().ok_or("prior owner")?;
        require(
            json!(constructor_types(inventory, owner))
                == observation["constructor_parameter_types"],
            "prior constructor comparison",
        )?;
        for (field, method) in [
            ("getStream_return_type", "getStream"),
            ("getString_return_type", "getString"),
        ] {
            if let Some(expected) = observation.get(field) {
                let member = inventory
                    .owners
                    .iter()
                    .find(|o| o.name == owner)
                    .unwrap()
                    .members
                    .iter()
                    .find(|m| m.name == method)
                    .ok_or("prior method")?;
                require(
                    member.r#type.0.as_ref().ok_or("method type")?.resolved == *expected,
                    "prior return comparison",
                )?;
            }
        }
    }
    let c = &inventory.manual_comparison;
    let supporting = load("supporting.json");
    let manual = supporting["io_classes"]
        .as_array()
        .ok_or("manual owners")?
        .iter()
        .find(|o| o["research_id"] == c.research_id)
        .ok_or("manual owner")?;
    require(
        c.research_id == "enterprise2026r2.supporting.com.altova.io.StreamOutput"
            && c.manual_signature == manual["literal_signature"]
            && c.manual_signature == "com.altova.io.StreamOutput(String filename)"
            && json!(c.manual_text_lines) == manual["text_lines"]
            && c.generated_public_constructor_parameter_types
                == constructor_types(inventory, "com.altova.io.StreamOutput")
            && c.relationship
                == "Separate source claims, not an automatic contradiction or a correction of the manual. Protected constructors are outside this public cohort; behavior remains unqualified.",
        "separate manual/generated declarations",
    )?;
    Ok(())
}

#[test]
fn supporting_io_public_surface_matches_independent_source_transcription() {
    let inventory = validate(load("supporting-io-declarations.json")).unwrap();
    assert_eq!(
        inventory
            .owners
            .iter()
            .filter(|o| o.language == "C#")
            .count(),
        12
    );
    assert_eq!(
        inventory
            .owners
            .iter()
            .filter(|o| o.language == "Java")
            .count(),
        12
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(ORACLE)),
        "4d87d8a5bf35cc2b0873b25bc65e339ac74955e40314aebc4df305c9701fa0d8"
    );
}
