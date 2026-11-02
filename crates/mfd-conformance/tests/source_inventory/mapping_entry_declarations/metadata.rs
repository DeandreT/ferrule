use super::{load, require, schema::Inventory};

pub(super) fn validate(i: &Inventory) -> Result<(), String> {
    require(
        i.format == "mapforce-enterprise-2026r2-mapping-entry-declarations"
            && i.format_version == 1,
        "format",
    )?;
    let a = &i.assessment_boundary;
    require(
        !i.inventory_complete
            && !i.runtime_proof
            && i.status == "unassessed"
            && i.applicability == "unknown"
            && i.execution_evidence.is_empty()
            && i.execution_receipts.is_empty()
            && !a.runtime_proof
            && a.status == "unassessed"
            && a.applicability == "unknown"
            && a.execution_evidence.is_empty()
            && a.execution_receipts.is_empty(),
        "source-only assessment",
    )?;
    let s = &i.scope;
    require(
        s.api_shape == "retained_one_input_one_output_profiles_only"
            && s.inherited_members == "excluded"
            && s.implicit_members == "excluded"
            && !s.implementation_copied
            && s.excluded
                == [
                    "inherited TraceProvider APIs",
                    "nested nonpublic implementation types and their members",
                    "business-schema members",
                    "bodies",
                    "private initializers",
                    "comments and examples",
                ]
            && s.unqualified
                == [
                    "universal generated API shape",
                    "close defaults",
                    "ownership and lifetime",
                    "close-on-error and error behavior",
                    "exact Java and CLR versions",
                    "dependency availability",
                    "Authenticode trust",
                    "ABI",
                    "backend applicability",
                    "runtime behavior",
                ],
        "qualification boundary",
    )?;
    let p = &i.provenance;
    let artifact = load("artifact-observations.json");
    let wrapper = load("schema-wrappers.json");
    let io = load("supporting-io-declarations.json");
    require(
        p.observation == "artifact-observations.json"
            && p.wrapper_observation == "schema-wrappers.json"
            && p.generator_version == artifact["release"]["executable_version"]
            && p.generator_sha256 == artifact["matched_members"][0]["sha256"]
            && p.generator_sha256 == wrapper["release"]["generator_sha256"]
            && p.ordinary_manifest_sha256 == io["provenance"]["ordinary_manifest_sha256"]
            && p.wrapper_manifest_sha256 == wrapper["generation"]["private_manifest_sha256"]
            && p.release_comparison_sha256
                == "d67383470c8781e7997f3eb765b6b43bd69aea5925e3c044b5d8847328c53cfe"
            && p.source_binding == "selected_generator_and_four_inputs_only"
            && p.historical_manifest_caveat
                == "earlier_unestablished_provenance_wording_retained_separate_later_selected_release_binding"
            && p.signature_trust == "unverified"
            && !p.profile_byte_identity
            && p.profile_integration_signatures_equal
            && !p.vendor_generation
            && !p.vendor_compilation
            && !p.vendor_execution,
        "selected release provenance",
    )?;
    require(p.inputs.len() == 4, "input coverage")?;
    for (index, (name, role)) in [
        ("ConvertProducts.mfd", "design"),
        ("Products.xsd", "source schema"),
        ("Products.xml", "source instance"),
        ("ProductValuePairs.xsd", "target schema"),
    ]
    .iter()
    .enumerate()
    {
        let input = &p.inputs[index];
        let matched = artifact["matched_members"]
            .as_array()
            .ok_or("installer members")?
            .iter()
            .find(|m| m["name"] == format!("Tutorial/{name}"))
            .ok_or("selected input")?;
        require(
            input.name == *name
                && input.role == *role
                && input.bytes == matched["bytes"].as_u64().ok_or("input bytes")?
                && input.sha256 == matched["sha256"],
            "selected input binding",
        )?;
    }
    for profile in &i.profiles {
        let hash = match profile.id.as_str() {
            "ordinary" => &p.ordinary_manifest_sha256,
            "schema-wrapper" => &p.wrapper_manifest_sha256,
            _ => return Err("unregistered profile".into()),
        };
        require(
            profile.manifest_sha256 == *hash && profile.generator_version == p.generator_version,
            "profile release join",
        )?;
    }
    require(
        i.oracle.path == "oracles/mapping-entry-declarations.jsonl"
            && i.oracle.method
                == "Independent manual transcription frozen before candidate extraction; numbered retained source declarations and separate hash readback, not candidate JSON, lexer output or cached outlines.",
        "oracle independence",
    )?;
    Ok(())
}

pub(super) fn validate_joins(i: &Inventory) -> Result<(), String> {
    let supporting = load("supporting.json");
    let expected = [
        (
            "/source_usage_contracts/0",
            "research_id",
            "enterprise2026r2.supporting.io-substitution-workflow",
            "declarations_only_not_behavior",
            vec![(0, 1), (0, 2), (1, 2), (1, 3)],
        ),
        (
            "/source_usage_contracts/3",
            "research_id",
            "enterprise2026r2.supporting.io-default-close",
            "manual_usage_claim_separate_unqualified",
            vec![(0, 0), (1, 0), (1, 1)],
        ),
        (
            "/unresolved_contracts/4",
            "id",
            "mapping-entry-contract",
            "retained_profiles_not_universal_api_or_error_contract",
            vec![(0, 1), (0, 2), (1, 2), (1, 3)],
        ),
    ];
    require(
        i.contract_enrichments.len() == expected.len(),
        "contract enrichment coverage",
    )?;
    for (c, (pointer, identity_field, identity, relationship, refs)) in
        i.contract_enrichments.iter().zip(expected)
    {
        let target = supporting.pointer(pointer).ok_or("supporting pointer")?;
        let expected_refs = refs
            .iter()
            .map(|(o, m)| format!("#/owners/{o}/members/{m}"))
            .collect::<Vec<_>>();
        require(
            c.file == "supporting.json"
                && c.pointer == pointer
                && c.identity_field == identity_field
                && c.identity == identity
                && target[identity_field] == identity
                && c.relationship == relationship
                && c.member_refs == expected_refs,
            "existing contract identity/pointer join",
        )?;
        for (oi, mi) in refs {
            let member = i
                .owners
                .get(oi)
                .and_then(|o| o.members.get(mi))
                .ok_or("contract member reference")?;
            require(
                if identity == "enterprise2026r2.supporting.io-default-close" {
                    member.name.contains("CloseObjectsAfterRun")
                } else {
                    member.name == "Run" || member.name == "run"
                },
                "contract member role",
            )?;
        }
    }
    let io = load("supporting-io-declarations.json");
    let expected = [
        ("Altova.IO.Input", 0),
        ("Altova.IO.Output", 6),
        ("com.altova.io.Input", 12),
        ("com.altova.io.Output", 13),
    ];
    require(i.io_base_joins.len() == expected.len(), "I/O join coverage")?;
    for (join, (name, index)) in i.io_base_joins.iter().zip(expected) {
        let target = io.pointer(&join.pointer).ok_or("I/O pointer")?;
        require(
            join.file == "supporting-io-declarations.json"
                && join.pointer == format!("/owners/{index}")
                && join.owner == name
                && join.research_id == format!("enterprise2026r2.supporting.{name}")
                && target["name"] == name
                && target["research_id"] == join.research_id
                && join.relationship == "declaration_fact_not_dependency_availability"
                && i.owners
                    .iter()
                    .flat_map(|o| &o.members)
                    .flat_map(|m| &m.parameters)
                    .any(|p| p.r#type.resolved.0.as_deref() == Some(name)),
            "documented I/O declaration join",
        )?;
    }
    Ok(())
}
