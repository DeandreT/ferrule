use codegen_runtime::{field, group, repeated, scalar, BasicNamedSourceInput, Instance, OwnedSourceError, OwnedSourceIdentity, OwnedSourceLimits, RuntimeError, TargetSelection, Value};
use ferrule_basic_occurrence_generated_640::{named, primary};
use serde_json::{json, Value as Json};
use std::error::Error;

fn string(value: &str) -> Instance { scalar(Value::String(value.into())) }
fn fields(items: impl IntoIterator<Item = (&'static str, Instance)>) -> Instance { group(items.into_iter().map(|(name, value)| field(name, value))) }
fn row(value: &str) -> Instance { fields([("Value", string(value))]) }
fn original(case: &str) -> Instance {
    match case {
        "mixed" => repeated([scalar(Value::Null), row("kept1"), scalar(Value::json_null()), row("kept2")]),
        "empty" => repeated([]),
        "singleton" => row("singleton"),
        _ => panic!("unknown frozen case"),
    }
}
fn inputs(program: &str, case: &str) -> (Instance, Instance) {
    if program == "primary" { (original(case), fields([])) } else {
        (fields([("Value", string("primary.shadow")), ("Ref", row("embedded.shadow"))]), original(case))
    }
}
fn tree(instance: &Instance) -> Json {
    match instance {
        Instance::Scalar(value) => {
            let kind = match value { Value::Null => "Null", Value::JsonNull(_) => "JsonNull", Value::Bool(_) => "Bool", Value::Int(_) => "Int", Value::Float(_) => "Float", Value::Decimal(_) => "Decimal", Value::String(_) => "String", Value::XmlNil(_) => "XmlNil" };
            json!({"kind":"Scalar","type":kind,"value":value})
        }
        Instance::Group(items) => json!({"kind":"Group","fields":items.iter().map(|(name,value)|json!({"name":name,"value":tree(value)})).collect::<Vec<_>>()}),
        Instance::Repeated(items) => json!({"kind":"Repeated","items":items.iter().map(tree).collect::<Vec<_>>()}),
        Instance::DocumentSet(_) => json!({"kind":"DocumentSet","complete_serialized_instance":instance,"complete_debug":format!("{instance:#?}")}),
        Instance::MappedSequence(items) => json!({"kind":"MappedSequence","items":items.iter().map(tree).collect::<Vec<_>>()}),
    }
}
fn capture<T: std::fmt::Debug>(id: &str, result: Result<T, RuntimeError>) -> Result<T, RuntimeError> {
    eprintln!("RAW {id} FULL_RETURN={result:#?}");
    if let Err(error) = &result {
        eprintln!("RAW {id} FULL_ERROR={error:#?} DISPLAY={error}");
        let mut next = error.source(); let mut index = 0;
        while let Some(cause) = next { eprintln!("RAW {id} CAUSE_{index}={cause:#?} DISPLAY={cause}"); next = cause.source(); index += 1; }
    }
    result
}
fn owner(program: &str) -> OwnedSourceIdentity { if program == "primary" { OwnedSourceIdentity::Primary } else { OwnedSourceIdentity::Named("Ref".into()) } }
fn semantic(error: &RuntimeError) -> Json {
    match error {
        RuntimeError::OwnedSourceAtNode { source: OwnedSourceError::InactiveSourceFrame { owner, collection: Some(collection) }, .. } => {
            let identity = match owner { OwnedSourceIdentity::Primary => json!({"is_primary":true,"name":null}), OwnedSourceIdentity::Named(name) => json!({"is_primary":false,"name":name}) };
            json!({"kind":"Error","semantic_kind":"InactiveSourceFrame","owner":identity,"collection":collection})
        }
        _ => json!({"kind":"UnexpectedError","complete_debug":format!("{error:#?}"),"display":error.to_string()}),
    }
}
fn expected_row<'a>(expected: &'a Json, id: &str) -> &'a Json {
    let rows = expected["calls"].as_array().unwrap().iter().filter(|row|row["id"] == id).collect::<Vec<_>>(); assert_eq!(rows.len(),1); rows[0]
}
#[test]
fn all_twelve_complete_returns_before_comparison() {
    let expected: Json = serde_json::from_str(include_str!("../expected.json")).unwrap();
    assert_eq!(expected["counts"]["total"],36); assert_eq!(expected["counts"]["calls_each_lane"],12);
    let limits = OwnedSourceLimits { max_roots:16, max_active_frames:8, max_path_segments:16, max_metadata_bytes:1048576, max_walk_contexts:64 };
    let mut captured: Vec<(String, &'static str, Result<Json, RuntimeError>)> = Vec::new();
    macro_rules! program_calls {
        ($module:ident, $label:literal) => {{
            for case in ["mixed","empty","singleton"] {
                for selection in ["all","primary"] {
                    let id = format!("OCCURRENCE-Rust-{}-{case}-{selection}",$label);
                    // Each call receives fresh original inputs; no cloned occurrence tree.
                    let (source, reference) = inputs($label, case);
                    let supplied = if $label == "primary" { Vec::new() } else { vec![BasicNamedSourceInput { name:"Ref", instance:&reference }] };
                    eprintln!("{}",json!({"id":id,"classification":"before_subject","primary":tree(&source),"supplied":supplied.iter().map(|input|json!({"name":input.name,"instance":tree(input.instance)})).collect::<Vec<_>>(),"selection":selection,"execution":null,"limits":{"max_roots":16,"max_active_frames":8,"max_path_segments":16,"max_metadata_bytes":1048576,"max_walk_contexts":64}}));
                    let actual = if selection == "all" {
                        capture(&id, $module::execute_with_basic_owned_sources(&source,&supplied,None,limits)).map(|output|json!({"kind":"All","primary":tree(&output.primary),"extras":output.extras.iter().map(|named|json!({"name":named.name,"instance":tree(&named.instance)})).collect::<Vec<_>>()}))
                    } else {
                        capture(&id, $module::execute_selected_with_basic_owned_sources(&source,&supplied,None,limits,TargetSelection::Primary)).map(|output|match output {
                            $module::SelectedTargetOutput::Primary(instance) => json!({"kind":"SelectedPrimary","instance":tree(&instance)}),
                            other => json!({"kind":"UnexpectedSelection","complete_debug":format!("{other:#?}")}),
                        })
                    };
                    eprintln!("{}",json!({"id":id,"classification":"before_comparison","complete_snapshot":match &actual { Ok(output)=>output.clone(),Err(error)=>semantic(error) },"actual_error_node_field_present":matches!(&actual, Err(RuntimeError::OwnedSourceAtNode { .. } | RuntimeError::BasicSourceBridgeAtNode { .. })),"actual_error_scope_field_present":false}));
                    captured.push((id,$label,actual));
                }
            }
        }};
    }
    program_calls!(primary,"primary"); program_calls!(named,"named");
    // All twelve complete original returns and error chains precede any comparison.
    assert_eq!(captured.len(),12);
    for (id,program,actual) in &captured {
        let row = expected_row(&expected,id);
        match actual {
            Ok(output) => assert_eq!(output,&row["expected"],"full typed generated return mismatch; originals retained"),
            Err(error) => {
                assert_eq!(error,&RuntimeError::OwnedSourceAtNode { node:2, source:OwnedSourceError::InactiveSourceFrame { owner:owner(program), collection:Some(vec![]) } });
                assert_eq!(semantic(error),row["expected"]);
                let direct = error.source().unwrap().downcast_ref::<OwnedSourceError>().unwrap();
                assert_eq!(direct,&OwnedSourceError::InactiveSourceFrame { owner:owner(program), collection:Some(vec![]) });
                // Preserve the frozen historical descriptor; current node2 wrapping is asserted above.
                assert_eq!(row["expected_error_provenance"],json!({"node_field_present":false,"native_semantic_node":2}));
            }
        }
    }
    eprintln!("{}",json!({"classification":"fixture_protocol_qualified","lane":"Rust","calls":12,"complete_success_oracles":8,"complete_inactive_oracles":4,"semantic_inactive_contract_qualified":true,"full_error_parity":false,"current_owned_node_contract":{"expected_node":2,"Rust_node_field_present":true,"Rust_scope_field_present":false},"historical_fixture_provenance_descriptor_preserved":true,"total_parity":false}));
}
