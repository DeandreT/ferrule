//! Literal, authored counterexamples to the primary physical boundary contract.
use super::*;

fn children_mut(schema: &mut SchemaNode) -> &mut Vec<SchemaNode> {
    let ir::SchemaKind::Group { children, .. } = &mut schema.kind else {
        panic!("group")
    };
    children
}
fn note_mut(schema: &mut SchemaNode) -> &mut SchemaNode {
    children_mut(schema)
        .iter_mut()
        .find(|child| child.name == "NTE")
        .unwrap()
}
fn dependency(manifest: &mut String, name: &str) {
    manifest.push_str(&format!(
        "{name} = {{ path = {:?} }}\n",
        format!("../{name}")
    ));
}
fn physical(host: &mut String, name: &str, input: &str, expected: &str) {
    host.push_str(&format!("    physical({name}::execute_x12_to_x12, {name}::execute_x12_to_x12_bytes, {name}::execute_x12_to_x12_with_context, {name}::execute_x12_to_x12_bytes_with_context, include_str!({input:?}), include_str!({expected:?}))?;\n"));
}

pub(super) fn add_cases(
    root: &Path,
    manifest: &mut String,
    host: &mut String,
) -> Result<(), Box<dyn Error>> {
    let (values, wire) = fixture(X12EnvelopeProfile::SingleTransaction, "00401");
    // The 22 GiB attacks are deliberately NOT allocated in tests. The runtime's
    // tiny-budget pre-native/pre-writer controls cover occurrence weighting;
    // these small public calls cover the same alias/default semantics.
    let mut alias = envelope(X12EnvelopeProfile::SingleTransaction, "00401");
    children_mut(note_mut(&mut alias))[0].name = "longname".into();
    let program = codegen::lower(&project(alias))?;
    artifacts(
        root,
        "review_alias",
        &program,
        &X12BoundaryPolicy {
            source: Some(Default::default()),
            target: Some(Default::default()),
        },
    )?;
    dependency(manifest, "review_alias");
    physical(
        host,
        "review_alias",
        "../single-00401.x12",
        "../single-00401.x12",
    );

    let mut p = project(envelope(X12EnvelopeProfile::SingleTransaction, "00401"));
    children_mut(note_mut(&mut p.target))[0].fixed = Some("abcdefgh".into());
    p.source = p.target.clone();
    artifacts(
        root,
        "review_defaults",
        &codegen::lower(&p)?,
        &X12BoundaryPolicy {
            source: None,
            target: Some(Default::default()),
        },
    )?;
    dependency(manifest, "review_defaults");
    let mut empty = values.clone();
    empty["NTE"] = json!([{}, {}]);
    fs::write(
        root.join("review-defaults.json"),
        serde_json::to_string_pretty(&empty)?,
    )?;
    fs::write(
        root.join("review-defaults.x12"),
        wire.replace("NTE*one~", "NTE*abcdefgh~")
            .replace("NTE*two~", "NTE*abcdefgh~"),
    )?;
    host.push_str("    assert_eq!(review_defaults::execute_json_to_x12(include_str!(\"../review-defaults.json\"))?,include_str!(\"../review-defaults.x12\"));\n    assert_eq!(review_defaults::execute_json_to_x12_bytes(include_bytes!(\"../review-defaults.json\"))?,include_bytes!(\"../review-defaults.x12\"));\n");

    // Bad present dates must retain a typed native error, never panic, truncate
    // an invalid year prefix, or fall back to the supplied execution clock.
    for (index, date) in ["a€1234", "xx300615", "00000615", "19000229", "2100-02-29"]
        .into_iter()
        .enumerate()
    {
        let mut input = values.clone();
        input["ISA"]["E09"] = json!(date);
        input["GS"]["E05"] = json!("094500");
        fs::write(
            root.join(format!("review-date-{index}.json")),
            serde_json::to_string_pretty(&input)?,
        )?;
        host.push_str(&format!("    let result=single_00401_completion::execute_json_to_x12_bytes_with_context(include_bytes!({:?}),&context());\n    assert!(matches!(result,Err(Failure::Native(EdiFormatError::InvalidEnvelopeElement{{ref element,ref value,..}})) if element==\"ISA09\" && value=={date:?}));\n",format!("../review-date-{index}.json")));
    }
    for (index, date) in ["300615", "20300615", "2030-06-15"].into_iter().enumerate() {
        let mut input = values.clone();
        input["ISA"]["E09"] = json!(date);
        input["GS"]["E05"] = json!("094500");
        fs::write(
            root.join(format!("review-good-date-{index}.json")),
            serde_json::to_string_pretty(&input)?,
        )?;
        host.push_str(&format!("    assert_eq!(single_00401_completion::execute_json_to_x12_bytes_with_context(include_bytes!({:?}),&context())?,include_bytes!(\"../single-00401-completed.x12\"));\n",format!("../review-good-date-{index}.json")));
    }

    // Actual SGn wire segments conflict with the native EDIFACT classification;
    // field/composite names, root names and SG10 containers do not.
    let mut refused = Vec::new();
    let mut fixed = envelope(X12EnvelopeProfile::SingleTransaction, "00401");
    let mut leaf = SchemaNode::scalar("E02", ScalarType::String);
    leaf.fixed = Some(String::new());
    children_mut(note_mut(&mut fixed)).push(leaf);
    refused.push(("fixed_empty", fixed, wire.clone()));
    let mut zero = envelope(X12EnvelopeProfile::SingleTransaction, "00401");
    let note = note_mut(&mut zero).clone();
    *note_mut(&mut zero) = SchemaNode::group("OptionalBody", vec![note]);
    refused.push((
        "zero_width",
        zero,
        wire.replace("NTE*one~\nNTE*two~\n", "")
            .replace("SE*5*", "SE*3*"),
    ));
    let mut sg = envelope(X12EnvelopeProfile::SingleTransaction, "00401");
    note_mut(&mut sg).name = "SG1".into();
    refused.push(("sg_segment", sg, wire.replace("NTE*", "SG1*")));
    let options = Options {
        package_name: "refused".into(),
        runtime_dependency: RuntimeDependency::Path("unused".into()),
    };
    for (name, schema, counterexample) in refused {
        let p = project(schema);
        let program = codegen::lower(&p)?;
        let before = program.clone();
        let policy = X12BoundaryPolicy {
            source: Some(Default::default()),
            target: Some(Default::default()),
        };
        let admitted = codegen::prepare_x12_boundary(&program, &policy)?;
        assert!(matches!(
            emit_with_x12(&program, &options, &policy),
            Err(codegen_rust::X12EmitError::UnsupportedNativeSetting { side: "source", .. })
        ));
        assert_eq!(program, before);
        fs::write(
            root.join(format!("review-{name}.descriptor")),
            &admitted.source_descriptor,
        )?;
        fs::write(
            root.join(format!("review-{name}.project.json")),
            serde_json::to_vec_pretty(&p)?,
        )?;
        fs::write(root.join(format!("review-{name}.x12")), counterexample)?;
        host.push_str(&format!("    assert!(matches!(codegen_runtime::parse_x12(include_str!({0:?}),include_str!({1:?})),Err(Failure::Descriptor(_))));\n    assert!(matches!(codegen_runtime::parse_x12_bytes(include_str!({0:?}),include_bytes!({1:?})),Err(Failure::Descriptor(_))));\n    assert!(matches!(codegen_runtime::serialize_x12(include_str!({0:?}),&Instance::Group(Vec::new().into()),None),Err(Failure::Descriptor(_))));\n",format!("../review-{name}.descriptor"),format!("../review-{name}.x12")));
        let target_only = X12BoundaryPolicy {
            source: None,
            target: Some(Default::default()),
        };
        assert!(matches!(
            emit_with_x12(&program, &options, &target_only),
            Err(codegen_rust::X12EmitError::UnsupportedNativeSetting { side: "target", .. })
        ));
    }

    let mut legal = envelope(X12EnvelopeProfile::SingleTransaction, "00401");
    let mut nonzero = legal.clone();
    let mut note = note_mut(&mut nonzero).clone();
    note.item_count_range = ItemCountRange::new(1, Some(3));
    *note_mut(&mut nonzero) = SchemaNode::group("OptionalBody", vec![note]);
    artifacts(
        root,
        "review_min_occurs",
        &codegen::lower(&project(nonzero))?,
        &X12BoundaryPolicy {
            source: Some(Default::default()),
            target: Some(Default::default()),
        },
    )?;
    dependency(manifest, "review_min_occurs");
    physical(
        host,
        "review_min_occurs",
        "../single-00401.x12",
        "../single-00401.x12",
    );
    legal.name = "SG1".into();
    let mut note = note_mut(&mut legal).clone();
    note.repeating = false;
    note.item_count_range = None;
    *note_mut(&mut legal) = SchemaNode::group("SG10", vec![note]).repeating();
    let reference = children_mut(&mut legal)
        .iter_mut()
        .find(|child| child.name == "REF")
        .unwrap();
    children_mut(reference)[0].name = "SG1".into();
    artifacts(
        root,
        "review_sg_roles",
        &codegen::lower(&project(legal))?,
        &X12BoundaryPolicy {
            source: Some(Default::default()),
            target: Some(Default::default()),
        },
    )?;
    dependency(manifest, "review_sg_roles");
    physical(
        host,
        "review_sg_roles",
        "../single-00401.x12",
        "../single-00401.x12",
    );

    for (name, profile) in [
        (
            "review_single_prefix",
            X12EnvelopeProfile::SingleTransaction,
        ),
        (
            "review_grouped_prefix",
            X12EnvelopeProfile::GroupedTransactions,
        ),
    ] {
        let mut schema = envelope(profile, "00401");
        fn wrap(node: &mut SchemaNode) {
            for child in children_mut(node) {
                if child.name == "NTE" {
                    let mut note = child.clone();
                    note.repeating = false;
                    note.item_count_range = None;
                    *child = SchemaNode::group("ParserErrors_Notes", vec![note]).repeating();
                } else if matches!(child.kind, ir::SchemaKind::Group { .. })
                    && !["ISA", "GS", "ST", "REF", "SE", "GE", "IEA"].contains(&child.name.as_str())
                {
                    wrap(child);
                }
            }
        }
        wrap(&mut schema);
        let options = X12BoundaryOptions {
            envelope_profile: profile,
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
        let path = if profile == X12EnvelopeProfile::SingleTransaction {
            "../single-00401.x12"
        } else {
            "../grouped-00401.x12"
        };
        physical(host, name, path, path);
    }
    // MapForce's actual trailing acknowledgment projection still returns empty
    // collections and refuses physical acknowledgment content after its owner.
    let mut ack = envelope(X12EnvelopeProfile::MapForceInterchanges, "00401");
    let interchange = &mut children_mut(&mut ack)[0];
    let group = &mut children_mut(interchange)[1];
    children_mut(group)
        .push(SchemaNode::group("ParserErrors_Group", vec![fields("AK9", 1)]).repeating());
    let selected = X12BoundaryOptions {
        envelope_profile: X12EnvelopeProfile::MapForceInterchanges,
        interchange_version: Some("00401".into()),
        ..Default::default()
    };
    artifacts(
        root,
        "review_ack",
        &codegen::lower(&project(ack))?,
        &X12BoundaryPolicy {
            source: Some(selected),
            target: None,
        },
    )?;
    dependency(manifest, "review_ack");
    let (mut expected, wire) = fixture(X12EnvelopeProfile::MapForceInterchanges, "00401");
    for interchange in expected["Interchanges"].as_array_mut().unwrap() {
        for group in interchange["Groups"].as_array_mut().unwrap() {
            group["ParserErrors_Group"] = json!([]);
        }
    }
    fs::write(
        root.join("review-ack.json"),
        serde_json::to_string_pretty(&expected)? + "\n",
    )?;
    fs::write(
        root.join("review-ack-content.x12"),
        wire.replacen("GE*2*7~\n", "GE*2*7~\nAK9*A~\n", 1),
    )?;
    host.push_str("    physical(review_ack::execute_x12_to_json,review_ack::execute_x12_to_json_bytes,review_ack::execute_x12_to_json_with_context,review_ack::execute_x12_to_json_bytes_with_context,include_str!(\"../interchanges-00401.x12\"),include_str!(\"../review-ack.json\"))?;\n    assert!(matches!(review_ack::execute_x12_to_json_bytes(include_bytes!(\"../review-ack-content.x12\")),Err(Failure::Envelope{..})));\n");
    Ok(())
}
