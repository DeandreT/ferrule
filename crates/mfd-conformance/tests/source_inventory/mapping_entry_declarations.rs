//! Finite mapping-entry source declarations; tests require repository metadata only.

use std::collections::BTreeSet;

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::load;

#[path = "mapping_entry_declarations/metadata.rs"]
mod metadata;
#[path = "mapping_entry_declarations/mutations.rs"]
mod mutations;
#[path = "mapping_entry_declarations/oracle.rs"]
mod oracle;
#[path = "mapping_entry_declarations/schema.rs"]
mod schema;

use schema::{Inventory, Type};

fn require(ok: bool, context: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(context.to_owned()) }
}

fn anchor(range: [u32; 2], bound: u32) -> Result<(), String> {
    require(
        range[0] > 0 && range[0] <= range[1] && range[1] <= bound,
        "source anchor",
    )
}

fn validate_type(ty: &Type, language: &str) -> Result<(), String> {
    let valid = match ty.binding.as_str() {
        "explicit_qualified" => {
            ty.spelling.contains('.') && ty.resolved.0.as_ref() == Some(&ty.spelling)
        }
        "unresolved_unqualified" => !ty.spelling.contains('.') && ty.resolved.0.is_none(),
        "language_keyword" => {
            let expected = match (language, ty.spelling.as_str()) {
                ("C#", "bool") => "System.Boolean",
                ("C#", "string") => "System.String",
                ("C#", "void") => "System.Void",
                ("Java", "boolean") => "boolean",
                ("Java", "void") => "void",
                _ => return Err("language type binding".into()),
            };
            ty.resolved.0.as_deref() == Some(expected)
        }
        _ => false,
    };
    require(valid && !ty.spelling.is_empty(), "source type binding")
}

fn validate(value: Value) -> Result<Inventory, String> {
    let inventory: Inventory = serde_json::from_value(value).map_err(|e| e.to_string())?;
    metadata::validate(&inventory)?;
    let mut source_ids = BTreeSet::new();
    for (index, source) in inventory.sources.iter().enumerate() {
        require(
            source.index == index
                && source.bytes > 0
                && source.sha256.len() == 64
                && source.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                && source.conditions.is_empty()
                && !source.path.starts_with('/')
                && !source.path.split('/').any(|p| p == "..")
                && inventory.profiles.iter().any(|p| p.id == source.profile)
                && source_ids.insert((&source.profile, &source.path)),
            "source identity/profile",
        )?;
        anchor(source.namespace_lines, source.lf_lines)?;
        for import in &source.imports {
            anchor(import.declaration_lines, source.lf_lines)?;
        }
    }
    let mut owner_names = BTreeSet::new();
    let mut used_sources = BTreeSet::new();
    let (mut members, mut parameters, mut accessors, mut throws) = (0, 0, 0, 0);
    for owner in &inventory.owners {
        require(
            owner_names.insert(&owner.name)
                && owner.kind == "class"
                && owner.modifiers == ["public"]
                && owner.explicit_constructors.is_empty()
                && owner.conditions.is_empty()
                && owner.assessment_ref == "#/assessment_boundary",
            "direct public owner boundary",
        )?;
        for ty in owner.extends.iter().chain(&owner.implements) {
            validate_type(ty, &owner.language)?;
        }
        let mut profiles = BTreeSet::new();
        for a in &owner.anchors {
            let s = inventory
                .sources
                .get(a.source_index)
                .ok_or("owner source")?;
            anchor(a.declaration_lines, s.lf_lines)?;
            require(
                owner.language == s.language
                    && owner.name == format!("{}.MappingMapToProductValuePairs", s.namespace)
                    && a.declaration_lines[0] > s.namespace_lines[1]
                    && a.closing_line > a.declaration_lines[1]
                    && a.closing_line < s.lf_lines
                    && profiles.insert(&s.profile)
                    && used_sources.insert(a.source_index),
                "owner header/closure/profile",
            )?;
        }
        require(
            profiles.len() == inventory.profiles.len(),
            "owner profile coverage",
        )?;
        let mut signatures = BTreeSet::new();
        for (index, member) in owner.members.iter().enumerate() {
            members += 1;
            parameters += member.parameters.len();
            accessors += member.accessors.len();
            throws += usize::from(!member.throws.is_empty());
            require(
                member.index == index
                    && member.directly_declared
                    && member.modifiers == ["public"]
                    && member.conditions.is_empty()
                    && matches!(member.kind.as_str(), "method" | "property"),
                "direct public member",
            )?;
            validate_type(&member.r#type, &owner.language)?;
            for ty in &member.throws {
                validate_type(ty, &owner.language)?;
            }
            let key = format!(
                "{}:{}({})",
                member.kind,
                member.name,
                member
                    .parameters
                    .iter()
                    .map(|p| p.r#type.spelling.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            );
            require(signatures.insert(key), "duplicate overload")?;
            for (pi, p) in member.parameters.iter().enumerate() {
                require(
                    p.index == pi && p.direction == "value" && p.default.0.is_none(),
                    "ordered parameter/default",
                )?;
                validate_type(&p.r#type, &owner.language)?;
            }
            require(
                member.anchors.len() == owner.anchors.len(),
                "member profile coverage",
            )?;
            for (a, oa) in member.anchors.iter().zip(&owner.anchors) {
                anchor(a.declaration_lines, oa.closing_line)?;
                let prior = if index == 0 {
                    oa.declaration_lines[1]
                } else {
                    owner.members[index - 1]
                        .anchors
                        .iter()
                        .find(|p| p.source_index == a.source_index)
                        .ok_or("prior member anchor")?
                        .declaration_lines[1]
                };
                require(
                    a.source_index == oa.source_index
                        && a.declaration_lines[0] > prior
                        && a.declaration_lines[1] < oa.closing_line,
                    "direct member anchor/order",
                )?;
            }
            require(
                (member.kind == "property") != member.accessors.is_empty(),
                "property accessor boundary",
            )?;
            let mut accessor_names = BTreeSet::new();
            for a in &member.accessors {
                require(
                    owner.language == "C#"
                        && matches!(a.name.as_str(), "get" | "set")
                        && accessor_names.insert(&a.name)
                        && matches!(
                            a.visibility.as_str(),
                            "public" | "private" | "protected" | "internal"
                        )
                        && a.explicit_visibility
                            .0
                            .as_ref()
                            .is_none_or(|v| v == &a.visibility)
                        && a.anchors.len() == member.anchors.len(),
                    "accessor declaration",
                )?;
                for (aa, ma) in a.anchors.iter().zip(&member.anchors) {
                    require(
                        aa.source_index == ma.source_index
                            && aa.declaration_lines == ma.declaration_lines,
                        "accessor token anchor",
                    )?;
                }
            }
        }
    }
    let s = &inventory.scope;
    require(
        (
            s.profile_count,
            s.source_count,
            s.owner_count,
            s.direct_member_count,
            s.parameter_count,
            s.accessor_count,
            s.throws_clause_count,
            s.explicit_constructor_count,
        ) == (
            inventory.profiles.len(),
            inventory.sources.len(),
            inventory.owners.len(),
            members,
            parameters,
            accessors,
            throws,
            0,
        ) && used_sources.len() == inventory.sources.len(),
        "derived cohort counts",
    )?;
    oracle::validate(&inventory)?;
    metadata::validate_joins(&inventory)?;
    Ok(inventory)
}

#[test]
fn retained_mapping_entry_surface_matches_frozen_source_oracle() {
    let inventory = validate(load("mapping-entry-declarations.json")).unwrap();
    assert_eq!((inventory.owners.len(), inventory.sources.len()), (2, 4));
    assert_eq!(
        (
            inventory.scope.direct_member_count,
            inventory.scope.parameter_count
        ),
        (7, 9)
    );
    assert_eq!(
        (
            inventory.scope.accessor_count,
            inventory.scope.throws_clause_count
        ),
        (2, 2)
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(oracle::ORACLE)),
        "95ecfd4f4ab7d5f1ea75b77a1a1ab07ec716127e6be67351c2a16122a486e3a5"
    );
}
