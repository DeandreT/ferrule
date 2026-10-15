//! Physical roles, root/field aliases and completion are independently tested
//! through generated document APIs. Labels remain declared metadata.
use super::*;

fn children_mut(node: &mut SchemaNode) -> &mut Vec<SchemaNode> {
    let ir::SchemaKind::Group { children, .. } = &mut node.kind else {
        panic!("group")
    };
    children
}
fn find_mut<'a>(node: &'a mut SchemaNode, name: &str) -> &'a mut SchemaNode {
    for child in children_mut(node) {
        if child.name == name {
            return child;
        }
        if matches!(child.kind, ir::SchemaKind::Group { .. }) {
            fn contains(node: &SchemaNode, name: &str) -> bool {
                node.name == name
                    || match &node.kind {
                        ir::SchemaKind::Group { children, .. } => {
                            children.iter().any(|child| contains(child, name))
                        }
                        _ => false,
                    }
            }
            if contains(child, name) {
                return find_mut(child, name);
            }
        }
    }
    panic!("missing node {name}")
}
fn dependency(manifest: &mut String, name: &str) {
    manifest.push_str(&format!(
        "{name} = {{ path = {:?} }}\n",
        format!("../{name}")
    ));
}
fn physical(host: &mut String, name: &str, input: &str) {
    host.push_str(&format!("    physical({name}::execute_x12_to_x12,{name}::execute_x12_to_x12_bytes,{name}::execute_x12_to_x12_with_context,{name}::execute_x12_to_x12_bytes_with_context,include_str!({input:?}),include_str!({input:?}))?;\n"));
}

pub(super) fn add_cases(
    root: &Path,
    manifest: &mut String,
    host: &mut String,
) -> Result<(), Box<dyn Error>> {
    for (name, label, profile, version) in [
        (
            "roles_root_isa",
            "ISA",
            X12EnvelopeProfile::SingleTransaction,
            "00401",
        ),
        (
            "roles_root_st",
            "ST",
            X12EnvelopeProfile::MapForceInterchanges,
            "00501",
        ),
        (
            "roles_scalar_st",
            "Envelope",
            X12EnvelopeProfile::SingleTransaction,
            "00401",
        ),
    ] {
        let mut schema = envelope(profile, version);
        schema.name = label.into();
        if name == "roles_scalar_st" {
            children_mut(find_mut(&mut schema, "ISA"))[0].name = "ST".into();
        }
        let options = X12BoundaryOptions {
            envelope_profile: profile,
            interchange_version: Some(version.into()),
            ..Default::default()
        };
        artifacts(
            root,
            name,
            &codegen::lower(&project(schema))?,
            &X12BoundaryPolicy {
                source: Some(options.clone()),
                target: Some(options),
            },
        )?;
        dependency(manifest, name);
        let input = if profile == X12EnvelopeProfile::SingleTransaction {
            "../single-00401.x12"
        } else {
            "../interchanges-00501.x12"
        };
        physical(host, name, input);
        host.push_str(&format!("    malformed({name}::execute_x12_to_x12,{name}::execute_x12_to_x12_bytes,include_str!({input:?}))?;\n"));
        host.push_str(&format!("    let bad=include_str!({input:?}).replacen(\"ST*850*0007~\",\"ST*850*0007*extra~\",1);\n    assert!(matches!({name}::execute_x12_to_x12(&bad),Err(Failure::Envelope{{..}})));\n    assert!(matches!({name}::execute_x12_to_x12_bytes(bad.as_bytes()),Err(Failure::Envelope{{..}})));\n    assert!(matches!({name}::execute_x12_to_x12_with_context(&bad,&context()),Err(Failure::Envelope{{..}})));\n    assert!(matches!({name}::execute_x12_to_x12_bytes_with_context(bad.as_bytes(),&context()),Err(Failure::Envelope{{..}})));\n"));
    }

    // Every alias below REF is positional: GE is a repeated composite, IEA a
    // repeated integer, ISA a repeated floating-point element. None is a header.
    let mut schema = envelope(X12EnvelopeProfile::MapForceInterchanges, "00501");
    let reference = find_mut(&mut schema, "REF");
    for (field, name) in children_mut(reference).iter_mut().zip(["GE", "IEA", "ISA"]) {
        field.name = name.into();
        field.repeating = true;
    }
    let options = X12BoundaryOptions {
        envelope_profile: X12EnvelopeProfile::MapForceInterchanges,
        interchange_version: Some("00501".into()),
        ..Default::default()
    };
    artifacts(
        root,
        "roles_body_aliases",
        &codegen::lower(&project(schema))?,
        &X12BoundaryPolicy {
            source: Some(options.clone()),
            target: Some(options),
        },
    )?;
    dependency(manifest, "roles_body_aliases");
    let (_, wire) = fixture(X12EnvelopeProfile::MapForceInterchanges, "00501");
    fs::write(
        root.join("roles-body-aliases.x12"),
        wire.replace("REF*AA:BB*3*1.25~", "REF*AA:BB^CC:DD*3^4*1.25^2.5~"),
    )?;
    physical(host, "roles_body_aliases", "../roles-body-aliases.x12");
    host.push_str("    let bad=include_str!(\"../roles-body-aliases.x12\").replacen(\"1.25^2.5\",\"1.25^NaN\",1);\n    assert!(matches!(roles_body_aliases::execute_x12_to_x12_bytes(bad.as_bytes()),Err(Failure::Native(EdiFormatError::ElementParse{segment,element:3,..})) if segment==\"REF\"));\n");

    // Invalid output reaches native document lookup; its error must name the
    // original declared physical field, not a synthetic renamed root/schema.
    let mut schema = envelope(X12EnvelopeProfile::SingleTransaction, "00401");
    schema.name = "ISA".into();
    children_mut(find_mut(&mut schema, "ISA"))[10].name = "ProtocolMarker".into();
    artifacts(
        root,
        "roles_native_error",
        &codegen::lower(&project(schema))?,
        &X12BoundaryPolicy {
            source: None,
            target: Some(Default::default()),
        },
    )?;
    dependency(manifest, "roles_native_error");
    let (mut values, _) = fixture(X12EnvelopeProfile::SingleTransaction, "00401");
    values["ISA"].as_object_mut().unwrap().remove("E11");
    values["ISA"]["ProtocolMarker"] = json!("!");
    fs::write(
        root.join("roles-native-error.json"),
        serde_json::to_string_pretty(&values)?,
    )?;
    host.push_str("    for result in [roles_native_error::execute_json_to_x12(include_str!(\"../roles-native-error.json\")),roles_native_error::execute_json_to_x12_with_context(include_str!(\"../roles-native-error.json\"),&context())] {assert!(matches!(result,Err(Failure::Native(EdiFormatError::InvalidEnvelopeElement{element,value,..})) if element==\"ProtocolMarker\" && value==\"!\"));}\n    assert!(matches!(roles_native_error::execute_json_to_x12_bytes_with_context(include_bytes!(\"../roles-native-error.json\"),&context()),Err(Failure::Native(EdiFormatError::InvalidEnvelopeElement{element,value,..})) if element==\"ProtocolMarker\" && value==\"!\"));\n");
    host.push_str("    assert!(matches!(roles_native_error::execute_json_to_x12_bytes(include_bytes!(\"../roles-native-error.json\")),Err(Failure::Native(EdiFormatError::InvalidEnvelopeElement{element,value,..})) if element==\"ProtocolMarker\" && value==\"!\"));\n");

    let mut schema = envelope(X12EnvelopeProfile::MapForceInterchanges, "00401");
    schema.name = "ST".into();
    let selected = X12BoundaryOptions {
        envelope_profile: X12EnvelopeProfile::MapForceInterchanges,
        interchange_version: Some("00401".into()),
        autocomplete: Some(mapping::X12Autocomplete {
            request_acknowledgement: false,
            transaction_set: Some("850".into()),
        }),
        ..Default::default()
    };
    artifacts(
        root,
        "roles_created_trailers",
        &codegen::lower(&project(schema))?,
        &X12BoundaryPolicy {
            source: None,
            target: Some(selected),
        },
    )?;
    dependency(manifest, "roles_created_trailers");
    let (mut values, _) = fixture(X12EnvelopeProfile::MapForceInterchanges, "00401");
    for interchange in values["Interchanges"].as_array_mut().unwrap() {
        interchange["IEA"] = json!([]);
        for group in interchange["Groups"].as_array_mut().unwrap() {
            group["GE"] = json!([]);
            group["GS"][0]["E05"] = json!("094500");
            for message in group["Messages"].as_array_mut().unwrap() {
                message["SE"] = json!([]);
            }
        }
    }
    fs::write(
        root.join("roles-created-trailers.json"),
        serde_json::to_string_pretty(&values)?,
    )?;
    host.push_str("    completion(roles_created_trailers::execute_json_to_x12,roles_created_trailers::execute_json_to_x12_bytes,roles_created_trailers::execute_json_to_x12_with_context,roles_created_trailers::execute_json_to_x12_bytes_with_context,include_str!(\"../roles-created-trailers.json\"),include_str!(\"../interchanges-00401-completed.x12\"))?;\n");
    Ok(())
}
