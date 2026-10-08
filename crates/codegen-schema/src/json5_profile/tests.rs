use std::error::Error;

use ir::{
    GroupAlternative, GroupAlternativeMode, JsonFormatAnnotations, ScalarType, ScalarTypeSet,
    SchemaKind, SchemaNode, ValueGeneration, XmlAlternativeKind, XmlWildcardProcessContents,
};
use serde_json::json;

use super::*;

fn scalar(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}
fn object() -> SchemaNode {
    SchemaNode::group("S", vec![scalar("n")])
}
fn require(schema: &mut SchemaNode, names: Vec<String>) {
    let SchemaKind::Group {
        children: _,
        alternatives: _,
        required,
        xml_restricted_alternatives: _,
        dynamic: _,
    } = &mut schema.kind
    else {
        panic!("fixture is a group")
    };
    *required = names;
}
fn unsupported(schema: &SchemaNode, field: &'static str) {
    let result = validate_schema(schema);
    eprintln!("original profile result={result:?}");
    assert!(
        matches!(result, Err(Json5ProfileError::UnsupportedMetadata { field: actual }) if actual == field)
    );
}

#[test]
fn nested_nullable_required_and_four_single_scalar_types_round_trip() {
    let mut nullable = SchemaNode::scalar("Optional", ScalarType::String);
    nullable.nullable = true;
    let mut nested = SchemaNode::group(
        "Details",
        vec![nullable, SchemaNode::scalar("Count", ScalarType::Int)],
    );
    require(&mut nested, vec!["Count".into()]);
    let mut schema = SchemaNode::group(
        "Source",
        vec![
            nested,
            SchemaNode::scalar("Ratio", ScalarType::Float),
            SchemaNode::scalar("Ready", ScalarType::Bool),
        ],
    );
    require(&mut schema, vec!["Details".into()]);
    let summary = validate_schema(&schema);
    eprintln!("original summary={summary:?}");
    assert_eq!(
        summary.unwrap(),
        Json5ProfileSummary {
            nodes: 6,
            logical_levels: 3,
            name_bytes: 48
        }
    );
    let encoded = encode_schema(&schema);
    eprintln!("original encoded={encoded:?}");
    let encoded = encoded.unwrap();
    let decoded = decode_schema(&encoded);
    eprintln!("original decoded={decoded:?}");
    assert_eq!(decoded.unwrap(), schema);
    assert_eq!(
        encoded,
        crate::encode(&schema, MAX_DESCRIPTOR_BYTES).unwrap()
    );
    let simple = encode_schema(&object());
    eprintln!("original literal descriptor={simple:?}");
    assert_eq!(
        simple.unwrap(),
        r#"{"name":"S","repeating":false,"kind":{"kind":"group","children":[{"name":"n","repeating":false,"kind":{"kind":"scalar","ty":"string"}}]}}"#
    );
}

#[test]
fn root_and_nested_unsupported_typed_shapes_refuse_without_changing_codec() {
    let result = validate_schema(&scalar("Root"));
    eprintln!("original root scalar={result:?}");
    assert!(matches!(result, Err(Json5ProfileError::RootObjectRequired)));
    let mut schema = object();
    schema.nullable = true;
    unsupported(&schema, "nullable_group");
    let mut schema = object();
    schema.repeating = true;
    unsupported(&schema, "repeating");
    let union = ScalarTypeSet::new([ScalarType::Int, ScalarType::String]).unwrap();
    let schema = SchemaNode::group("S", vec![SchemaNode::scalar_union("n", union)]);
    unsupported(&schema, "kind.scalar_union");
    let mut schema = object();
    let SchemaKind::Group {
        children,
        alternatives: _,
        required: _,
        xml_restricted_alternatives: _,
        dynamic,
    } = &mut schema.kind
    else {
        panic!("fixture group")
    };
    children[0].nullable = true;
    *dynamic = Some(Box::new(scalar("value")));
    unsupported(&schema, "kind.dynamic");
    let ordinary = crate::encode(&schema, MAX_DESCRIPTOR_BYTES);
    eprintln!("original unchanged ordinary open descriptor={ordinary:?}");
    assert!(ordinary.is_ok());
    let mut schema = object();
    let SchemaKind::Group {
        children: _,
        alternatives,
        required: _,
        xml_restricted_alternatives: _,
        dynamic: _,
    } = &mut schema.kind
    else {
        panic!("fixture group")
    };
    alternatives.push(GroupAlternative {
        name: "T".into(),
        members: vec!["n".into()],
        required: Vec::new(),
        constraints: Vec::new(),
    });
    unsupported(&schema, "kind.alternatives");
    let mut schema = object();
    let SchemaKind::Group {
        children: _,
        alternatives: _,
        required: _,
        xml_restricted_alternatives,
        dynamic: _,
    } = &mut schema.kind
    else {
        panic!("fixture group")
    };
    xml_restricted_alternatives.push("T".into());
    unsupported(&schema, "kind.xml_restricted_alternatives");
}

#[test]
fn all_nondefault_descriptor_metadata_families_and_nested_defaults_are_explicit() {
    let base = json!({"name":"S","repeating":false,"kind":{"kind":"group","children":[{"name":"n","repeating":false,"kind":{"kind":"scalar","ty":"string"}}]}});
    let rejected = [
        ("xml_namespace", json!("urn:test")),
        ("xml_name_alternatives", json!(["urn:test"])),
        ("xml_wildcard_namespace", json!({})),
        ("xml_wildcard_process_contents", json!("strict")),
        ("repeating", json!(true)),
        ("recursive_ref", json!("S")),
        ("attribute", json!(true)),
        ("text", json!(true)),
        ("nillable", json!(true)),
        ("xml_optional", json!(true)),
        ("xml_attribute_required", json!(true)),
        ("container_nullable", json!(true)),
        ("json_any", json!(true)),
        ("fixed", json!("x")),
        ("json_allowed_values", json!({})),
        ("numeric_range", json!({})),
        ("json_multiple_of", json!({})),
        ("item_count_range", json!({})),
        ("json_contains", json!({})),
        ("json_dependent_schemas", json!({})),
        ("property_count_range", json!({})),
        ("json_property_dependencies", json!({})),
        ("json_pattern_property_names", json!({})),
        ("json_property_names", json!({})),
        ("json_unique_items", json!(true)),
        ("string_length_range", json!({})),
        ("json_patterns", json!({})),
        ("json_formats", json!(["email"])),
        ("default", json!("x")),
        ("value_generation", json!("max_number")),
        ("alternative_mode", json!("inclusive")),
        ("xml_alternative_kind", json!("substitution_group")),
        ("xml_type_alternatives", json!(true)),
        ("xml_default_type", json!("T")),
        ("xml_repeating_sequences", json!([{}])),
        ("xml_repeating_choices", json!([{}])),
        ("database_relation", json!({})),
    ];
    assert_eq!(rejected.len(), 37);
    for (field, value) in rejected {
        for nested in [false, true] {
            let mut raw = base.clone();
            if nested {
                raw["kind"]["children"][0][field] = value.clone();
            } else {
                raw[field] = value.clone();
            }
            let source = raw.to_string();
            let result = decode_schema(&source);
            eprintln!("original nondefault field={field} nested={nested} result={result:?}");
            assert!(
                matches!(result, Err(Json5ProfileError::UnsupportedDescriptorMetadata { field: actual }) if actual == field)
            );
        }
    }
    // These are raw metadata refusals, not claims that every deliberately tiny
    // constraint payload above is a valid general-schema constraint.
    let mut leaf = scalar("n");
    leaf.default = Some("x".into());
    unsupported(&SchemaNode::group("S", vec![leaf]), "default");
    let mut leaf = scalar("n");
    leaf.value_generation = Some(ValueGeneration::MaxNumber);
    unsupported(&SchemaNode::group("S", vec![leaf]), "value_generation");
    let mut leaf = scalar("n");
    leaf.json_formats = JsonFormatAnnotations::new(["email".into()]).unwrap();
    unsupported(&SchemaNode::group("S", vec![leaf]), "json_formats");
    let mut schema = object();
    schema.alternative_mode = GroupAlternativeMode::Inclusive;
    unsupported(&schema, "alternative_mode");
    let mut schema = object();
    schema.xml_alternative_kind = XmlAlternativeKind::SubstitutionGroup;
    unsupported(&schema, "xml_alternative_kind");
    let mut schema = object();
    schema.xml_wildcard_process_contents = XmlWildcardProcessContents::Strict;
    unsupported(&schema, "xml_wildcard_process_contents");
    let mut raw = base;
    raw["fixed"] = json!(null);
    raw["json_formats"] = json!([]);
    raw["alternative_mode"] = json!("exclusive");
    raw["xml_alternative_kind"] = json!("xsi_type");
    raw["xml_wildcard_process_contents"] = json!("skip");
    let result = decode_schema(&raw.to_string());
    eprintln!("original explicit-default raw result={result:?}");
    assert_eq!(result.unwrap(), object());
}

#[test]
fn sibling_and_required_names_are_independent_presence_controls() {
    let duplicate = SchemaNode::group("S", vec![scalar("n"), scalar("n")]);
    let observed = validate_schema(&duplicate);
    eprintln!("original duplicate child={observed:?}");
    assert!(matches!(observed, Err(Json5ProfileError::DuplicateChildName { name }) if name == "n"));
    for (names, expected) in [
        (vec!["n", "n"], Json5RequiredKind::DuplicateName),
        (vec!["missing"], Json5RequiredKind::UndeclaredName),
        (vec![""], Json5RequiredKind::EmptyName),
    ] {
        let mut schema = SchemaNode::group("S", vec![scalar("n"), scalar("m")]);
        require(&mut schema, names.into_iter().map(str::to_owned).collect());
        let observed = validate_schema(&schema);
        eprintln!("original required={observed:?}");
        assert!(
            matches!(observed, Err(Json5ProfileError::InvalidRequiredName { kind, name: Some(_) }) if kind == expected)
        );
    }
    let mut schema = object();
    require(&mut schema, vec!["n".into(); MAX_SCHEMA_NODES + 1]);
    let observed = validate_schema(&schema);
    eprintln!("original overfull required={observed:?}");
    assert!(matches!(
        observed,
        Err(Json5ProfileError::InvalidRequiredName {
            kind: Json5RequiredKind::TooManyNames,
            name: None
        })
    ));
}

#[test]
fn node_depth_and_utf8_name_exact_plus_one_are_separate_from_codec_admission() {
    let mut schema = SchemaNode::group(
        "",
        (0..4095)
            .map(|index| scalar(&format!("n{index}")))
            .collect(),
    );
    let observed = validate_schema(&schema);
    eprintln!("original nodes4096={observed:?}");
    assert_eq!(observed.unwrap().nodes, 4096);
    let encoded = encode_schema(&schema);
    eprintln!(
        "original nodes4096 descriptor status={:?}",
        encoded.as_ref().map(String::len)
    );
    assert!(encoded.is_ok());
    let SchemaKind::Group {
        children,
        alternatives: _,
        required: _,
        xml_restricted_alternatives: _,
        dynamic: _,
    } = &mut schema.kind
    else {
        panic!("fixture group")
    };
    children.push(scalar("extra"));
    let observed = validate_schema(&schema);
    eprintln!("original nodes4097={observed:?}");
    assert!(matches!(
        observed,
        Err(Json5ProfileError::Limit {
            resource: Json5ProfileResource::SchemaNodes,
            requested: 4097,
            max: 4096
        })
    ));
    let chain = |levels: usize| {
        let mut node = scalar("v");
        for _ in 1..levels {
            node = SchemaNode::group("g", vec![node]);
        }
        node
    };
    let deep = chain(64);
    let observed = validate_schema(&deep);
    eprintln!("original logical64={observed:?}");
    assert_eq!(observed.unwrap().logical_levels, 64);
    let encoded = encode_schema(&deep);
    eprintln!("original logical64 actual codec={encoded:?}");
    assert!(matches!(encoded, Err(Json5ProfileError::Codec(_))));
    let observed = validate_schema(&chain(65));
    eprintln!("original logical65={observed:?}");
    assert!(matches!(
        observed,
        Err(Json5ProfileError::Limit {
            resource: Json5ProfileResource::LogicalLevels,
            requested: 65,
            max: 64
        })
    ));
    let name = format!("{}x", "雪".repeat(1365));
    assert_eq!(name.len(), 4096);
    let mut schema = SchemaNode::group("", vec![scalar(&name)]);
    let observed = encode_schema(&schema);
    eprintln!(
        "original name4096 descriptor status={:?}",
        observed.as_ref().map(String::len)
    );
    assert!(observed.is_ok());
    let SchemaKind::Group {
        children,
        alternatives: _,
        required: _,
        xml_restricted_alternatives: _,
        dynamic: _,
    } = &mut schema.kind
    else {
        panic!("fixture group")
    };
    children[0].name.push('y');
    let observed = validate_schema(&schema);
    eprintln!("original name4097={observed:?}");
    assert!(matches!(
        observed,
        Err(Json5ProfileError::Limit {
            resource: Json5ProfileResource::NameLength,
            requested: 4097,
            max: 4096
        })
    ));
}

#[test]
fn cumulative_names_count_required_occurrences_and_actual_descriptor_bytes() {
    let mut schema = SchemaNode::group(
        "",
        (0..128)
            .map(|i| scalar(&format!("{i:04}{}", "n".repeat(4092))))
            .collect(),
    );
    let SchemaKind::Group {
        children,
        alternatives: _,
        required,
        xml_restricted_alternatives: _,
        dynamic: _,
    } = &mut schema.kind
    else {
        panic!("fixture group")
    };
    *required = children.iter().map(|child| child.name.clone()).collect();
    let observed = validate_schema(&schema);
    eprintln!("original cumulative exact including required={observed:?}");
    assert_eq!(observed.unwrap().name_bytes, 1_048_576);
    schema.name.push('x');
    let observed = validate_schema(&schema);
    eprintln!("original cumulative one-over={observed:?}");
    assert!(matches!(
        observed,
        Err(Json5ProfileError::Limit {
            resource: Json5ProfileResource::NameBytes,
            requested: 1_048_577,
            max: 1_048_576
        })
    ));
    let mut schema = SchemaNode::group("", (0..256).map(|i| scalar(&format!("{i:04}"))).collect());
    let baseline = crate::encode(&schema, MAX_DESCRIPTOR_BYTES).unwrap();
    let mut remaining = MAX_DESCRIPTOR_BYTES - baseline.len();
    let SchemaKind::Group {
        children,
        alternatives: _,
        required: _,
        xml_restricted_alternatives: _,
        dynamic: _,
    } = &mut schema.kind
    else {
        panic!("fixture group")
    };
    for child in children.iter_mut() {
        let added = remaining.min(MAX_NAME_BYTES - child.name.len());
        child.name.push_str(&"n".repeat(added));
        remaining -= added;
    }
    assert_eq!(remaining, 0);
    let encoded = encode_schema(&schema);
    eprintln!(
        "original encoded exact status={:?}",
        encoded.as_ref().map(String::len)
    );
    let encoded = encoded.unwrap();
    assert_eq!(encoded.len(), MAX_DESCRIPTOR_BYTES);
    let decoded = decode_schema(&encoded);
    eprintln!(
        "original exact decode status={:?}",
        decoded.as_ref().map(|_| "complete schema retained")
    );
    assert_eq!(decoded.unwrap(), schema);
    let SchemaKind::Group {
        children,
        alternatives: _,
        required: _,
        xml_restricted_alternatives: _,
        dynamic: _,
    } = &mut schema.kind
    else {
        panic!("fixture group")
    };
    children
        .iter_mut()
        .find(|child| child.name.len() < MAX_NAME_BYTES)
        .unwrap()
        .name
        .push('n');
    let observed = encode_schema(&schema);
    eprintln!("original encoded one-over={observed:?}");
    assert!(matches!(
        observed,
        Err(Json5ProfileError::Codec(crate::CodecError::TooLarge {
            bytes: 1_048_577,
            max: 1_048_576
        }))
    ));
    let padded = format!(
        "{}{}",
        crate::encode(&object(), MAX_DESCRIPTOR_BYTES).unwrap(),
        " ".repeat(MAX_DESCRIPTOR_BYTES - baseline_object_len())
    );
    assert_eq!(padded.len(), MAX_DESCRIPTOR_BYTES);
    let observed = decode_schema(&padded);
    eprintln!("original raw bytes exact={observed:?}");
    assert_eq!(observed.unwrap(), object());
    let observed = decode_schema(&(padded + " "));
    eprintln!("original raw bytes one-over={observed:?}");
    assert!(matches!(
        observed,
        Err(Json5ProfileError::Limit {
            resource: Json5ProfileResource::DescriptorBytes,
            requested: 1_048_577,
            max: 1_048_576
        })
    ));
}
fn baseline_object_len() -> usize {
    crate::encode(&object(), MAX_DESCRIPTOR_BYTES)
        .unwrap()
        .len()
}

#[test]
fn unknown_wire_fields_refuse_before_legacy_ignored_metadata_is_erased() {
    for location in [0, 1, 2] {
        let mut raw = serde_json::to_value(object()).unwrap();
        match location {
            0 => raw["future_profile"] = json!(true),
            1 => raw["kind"]["children"][0]["future_profile"] = json!(true),
            2 => raw["kind"]["children"][0]["kind"]["future_profile"] = json!(true),
            _ => unreachable!(),
        }
        let source = raw.to_string();
        let ordinary = crate::decode(&source, MAX_DESCRIPTOR_BYTES);
        let selected = decode_schema(&source);
        eprintln!("original ordinary={ordinary:?} selected={selected:?}");
        assert_eq!(ordinary.unwrap(), object());
        assert!(
            matches!(selected, Err(Json5ProfileError::UnknownDescriptorField { field }) if field == "future_profile")
        );
    }
    for nested in [false, true] {
        let mut raw = serde_json::to_value(object()).unwrap();
        let unknown = "x".repeat(MAX_NAME_BYTES + 1);
        if nested {
            raw["kind"]["children"][0]["kind"][unknown.as_str()] = json!(true);
        } else {
            raw[unknown.as_str()] = json!(true);
        }
        let observed = decode_schema(&raw.to_string());
        eprintln!("original oversized unknown key nested={nested}: {observed:?}");
        assert!(matches!(
            observed,
            Err(Json5ProfileError::Limit {
                resource: Json5ProfileResource::NameLength,
                requested: 4097,
                max: 4096
            })
        ));
    }
    let plain = crate::encode(&object(), MAX_DESCRIPTOR_BYTES).unwrap();
    let versioned = format!("{}{}", crate::V2_PREFIX, plain);
    let observed = decode_schema(&versioned);
    eprintln!("original existing v2 syntax={observed:?}");
    assert_eq!(observed.unwrap(), object());
    let observed = decode_schema("FERRULE-EMBEDDED-SCHEMA/3\n{}");
    eprintln!("original unknown version={observed:?}");
    assert!(matches!(
        observed,
        Err(Json5ProfileError::Codec(
            crate::CodecError::UnsupportedVersion
        ))
    ));
    let observed = decode_schema("{");
    eprintln!("original malformed descriptor={observed:?}");
    assert!(observed.as_ref().unwrap_err().source().is_some());
    assert!(matches!(
        observed,
        Err(Json5ProfileError::DescriptorSyntax(_))
    ));
}
