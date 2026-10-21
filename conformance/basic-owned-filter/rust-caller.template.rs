use codegen_runtime::{BasicNamedSourceInput, ExecutionContext, Instance, OwnedSourceLimits, RuntimeError, TargetSelection, Value, field, group, repeated, scalar};
use ferrule_basic_filter_generated_662::{global_rules, lazy_false, nested_identity, non_bool, order, phases};
use serde_json::{Value as Json, json};
use std::error::Error;
use std::path::Path;

fn value(input: &Json) -> Value {
    assert_eq!(input["kind"], "Scalar");
    match input["type"].as_str().unwrap() {
        "Null" => { assert!(input["value"].is_null()); Value::Null },
        "JsonNull" => { assert!(input["value"].is_null()); Value::json_null() },
        "Int" => Value::Int(input["value"].as_i64().unwrap()),
        "Bool" => Value::Bool(input["value"].as_bool().unwrap()),
        "String" => Value::String(input["value"].as_str().unwrap().to_owned()),
        _ => panic!("unsupported frozen scalar"),
    }
}
fn instance(input: &Json) -> Instance {
    match input["kind"].as_str().unwrap() {
        "Scalar" => scalar(value(input)),
        "Group" => group(input["fields"].as_array().unwrap().iter().map(|entry|field(entry["name"].as_str().unwrap(),instance(&entry["value"])))),
        "Repeated" => repeated(input["items"].as_array().unwrap().iter().map(instance)),
        _ => panic!("unsupported frozen instance domain"),
    }
}
fn tree(input: &Instance) -> Json {
    match input {
        Instance::Scalar(value) => {
            let kind=match value { Value::Null=>"Null",Value::JsonNull(_)=>"JsonNull",Value::Bool(_)=>"Bool",Value::Int(_)=>"Int",Value::Float(_)=>"Float",Value::Decimal(_)=>"Decimal",Value::String(_)=>"String",Value::XmlNil(_)=>"XmlNil" };
            let payload=match value { Value::Null | Value::JsonNull(_) | Value::XmlNil(_) => Json::Null,_=>serde_json::to_value(value).unwrap() };
            json!({"kind":"Scalar","type":kind,"value":payload})
        }
        Instance::Group(fields)=>json!({"kind":"Group","fields":fields.iter().map(|(name,value)|json!({"name":name,"value":tree(value)})).collect::<Vec<_>>()}),
        Instance::Repeated(items)=>json!({"kind":"Repeated","items":items.iter().map(tree).collect::<Vec<_>>()}),
        other=>json!({"kind":"UnexpectedDomain","complete_debug":format!("{other:#?}"),"complete_serialized_instance":other}),
    }
}
fn capture<T: std::fmt::Debug>(id: &str, actual: Result<T,RuntimeError>) -> Result<T,RuntimeError> {
    eprintln!("RAW {id} FULL_RETURN={actual:#?}");
    if let Err(error)=&actual {
        eprintln!("RAW {id} FULL_ERROR={error:#?} DISPLAY={error}");
        let mut next=error.source();let mut index=0;
        while let Some(cause)=next { eprintln!("RAW {id} CAUSE_{index}={cause:#?} DISPLAY={cause}");next=cause.source();index+=1; }
    }
    actual
}
fn error_snapshot(error: &RuntimeError) -> Json {
    match error {
        RuntimeError::NotABool{node,found}=>json!({"kind":"Error","category":"NotABool","node":node,"found":found}),
        RuntimeError::MappingException{node,message}=>json!({"kind":"Error","category":"MappingException","node":node,"mapping_exception_message":message}),
        RuntimeError::MappingFailure{rule,message}=>json!({"kind":"Error","category":"MappingFailure","rule":rule,"mapping_failure_message":message}),
        _=>json!({"kind":"UnexpectedError","complete_debug":format!("{error:#?}"),"display":error.to_string()}),
    }
}
fn expected_row<'a>(expected: &'a Json,id: &str)->&'a Json {
    let rows=expected["calls"].as_array().unwrap().iter().filter(|row|row["id"]==id).collect::<Vec<_>>();assert_eq!(rows.len(),1);rows[0]
}
fn limits(spec: &Json)->OwnedSourceLimits {
    let x=&spec["generated_limits"];
    let n=|key:&str|usize::try_from(x[key].as_u64().unwrap()).unwrap();
    OwnedSourceLimits{max_roots:n("max_roots"),max_active_frames:n("max_active_frames"),max_path_segments:n("max_path_segments"),max_metadata_bytes:n("max_metadata_bytes"),max_walk_contexts:n("max_walk_contexts")}
}
#[test]
fn all_twelve_complete_returns_before_comparison() {
    let specs:Json=serde_json::from_str(include_str!("../programs.json")).unwrap();
    let expected:Json=serde_json::from_str(include_str!("../expected.json")).unwrap();
    assert_eq!(specs["fixture"],"basic-owned-filter-v1");assert_eq!(specs["exact_counts"],json!({"models":6,"calls_per_backend":12,"total_calls":36}));
    assert_eq!(specs["models"].as_array().unwrap().len(),6);assert_eq!(specs["calls"].as_array().unwrap().len(),12);assert_eq!(expected["calls"].as_array().unwrap().len(),36);
    let execution=ExecutionContext::new(Path::new(specs["execution"]["mapping_path"].as_str().unwrap()));
    let policy=limits(&specs);let mut captured:Vec<(String,Result<Json,RuntimeError>)>=Vec::new();
    macro_rules! model_calls {
        ($module:ident,$label:literal)=>{{
            for call in specs["calls"].as_array().unwrap().iter().filter(|call|call["model"]==$label) {
                let id=format!("FILTER-rust-{}",call["case"].as_str().unwrap());
                // Fresh decoded owning inputs per call; generated evaluation borrows them.
                let source=instance(&call["primary"]);
                let records=call["supplied"].as_array().unwrap().iter().map(|record|(record["name"].as_str().unwrap().to_owned(),instance(&record["instance"]))).collect::<Vec<_>>();
                let supplied=records.iter().map(|(name,input)|BasicNamedSourceInput{name,instance:input}).collect::<Vec<_>>();
                eprintln!("{}",json!({"id":id,"classification":"before_subject","model":$label,"primary":tree(&source),"supplied":records.iter().map(|(name,input)|json!({"name":name,"instance":tree(input)})).collect::<Vec<_>>(),"selection":call["selection"],"execution":specs["execution"],"limits":specs["generated_limits"]}));
                let actual=match call["selection"]["kind"].as_str().unwrap() {
                    "All"=>capture(&id,$module::execute_with_basic_owned_sources(&source,&supplied,Some(&execution),policy)).map(|output|json!({"kind":"All","primary":tree(&output.primary),"extras":output.extras.iter().map(|output|json!({"name":output.name,"instance":tree(&output.instance)})).collect::<Vec<_>>() })),
                    kind=>{
                        let selected=match kind { "Primary"=>TargetSelection::Primary,"Named"=>TargetSelection::Named(call["selection"]["name"].as_str().unwrap()),_=>panic!("unknown frozen selection") };
                        capture(&id,$module::execute_selected_with_basic_owned_sources(&source,&supplied,Some(&execution),policy,selected)).map(|output|match output {
                            $module::SelectedTargetOutput::Primary(instance)=>json!({"kind":"SelectedPrimary","instance":tree(&instance)}),
                            $module::SelectedTargetOutput::Named(output)=>json!({"kind":"SelectedNamed","name":output.name,"instance":tree(&output.instance)}),
                        })
                    }
                };
                eprintln!("{}",json!({"id":id,"classification":"before_comparison","snapshot":match &actual { Ok(output)=>output.clone(),Err(error)=>error_snapshot(error) },"complete_error_debug":actual.as_ref().err().map(|error|format!("{error:#?}"))}));
                captured.push((id,actual));
            }
        }};
    }
    model_calls!(phases,"phases");model_calls!(nested_identity,"nested_identity");model_calls!(lazy_false,"lazy_false");
    model_calls!(non_bool,"non_bool");model_calls!(order,"order");model_calls!(global_rules,"global_rules");
    assert_eq!(captured.len(),12);
    for (id,actual) in &captured {
        let row=expected_row(&expected,id);
        match actual {
            Ok(output)=>{assert_eq!(output,&row["expected"]);assert!(row["typed_error"].is_null());},
            Err(error)=>{
                let typed=&row["typed_error"];
                let node=||u32::try_from(typed["node"].as_u64().unwrap()).unwrap();
                let expected_error=match typed["variant"].as_str().unwrap() {
                    "NotABool"=>RuntimeError::NotABool{node:node(),found:"int"},
                    "MappingException"=>RuntimeError::MappingException{node:node(),message:Some(typed["mapping_exception_message"].as_str().unwrap().to_owned())},
                    "MappingFailure"=>RuntimeError::MappingFailure{rule:usize::try_from(typed["rule"].as_u64().unwrap()).unwrap(),message:Some(typed["mapping_failure_message"].as_str().unwrap().to_owned())},
                    _=>panic!("unexpected frozen RuntimeError variant"),
                };
                assert_eq!(error,&expected_error);assert_eq!(error_snapshot(error),row["expected"]);assert_eq!(error.to_string(),typed["display"].as_str().unwrap());
                assert!(error.source().is_none());assert_eq!(typed["source_chain_length"],0);assert_eq!(typed["typed_inner_null"],true);
            }
        }
        eprintln!("{id} matchedoracles");
    }
    eprintln!("{}",json!({"classification":"fixture_protocol_qualified","backend":"rust","calls":12,"full_filter_parity":false,"full_error_parity":false,"total_parity":false}));
}
