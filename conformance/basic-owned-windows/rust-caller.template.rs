use codegen_runtime::{BasicNamedSourceInput, ExecutionContext, Instance, OwnedSourceLimits, RuntimeError, TargetSelection, Value, field, group, repeated, scalar};
use ferrule_basic_windows_generated_704::{operators, filter_then_sort, sort_then_filter, nested_parent_bounds, empty_bounds_eager, zero_window_non_bool, invalid_bound, fts_predicate_priority, stf_key_priority, first_predicate_priority};
use serde_json::{Value as Json, json};
use std::error::Error;
use std::path::Path;

fn value(input: &Json) -> Value {
    assert_eq!(input["kind"], "Scalar");
    match input["type"].as_str().unwrap() {
        "Null" => { assert!(input["value"].is_null()); Value::Null },
        "JsonNull" => { assert!(input["value"].is_null()); Value::json_null() },
        "Float" => Value::Float(f64::from_bits(u64::from_str_radix(input["value"].as_str().unwrap(), 16).unwrap())),
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
            let payload=match value { Value::Null | Value::JsonNull(_) | Value::XmlNil(_) => Json::Null,Value::Float(value)=>Json::String(format!("{:016x}",value.to_bits())),_=>serde_json::to_value(value).unwrap() };
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
        RuntimeError::NotAnItemCount{node,found}=>json!({"kind":"Error","category":"NotAnItemCount","node":node,"found":found}),
        RuntimeError::NotABool{node,found}=>json!({"kind":"Error","category":"NotABool","node":node,"found":found}),
        RuntimeError::MappingException{node,message}=>json!({"kind":"Error","category":"MappingException","node":node,"mapping_exception_message":message}),
        RuntimeError::MappingFailure{rule,message}=>json!({"kind":"Error","category":"MappingFailure","rule":rule,"mapping_failure_message":message}),
        RuntimeError::OwnedSourceAtNode{node,source:codegen_runtime::OwnedSourceError::SourceUnavailable{name}}=>json!({"kind":"Error","category":"OwnedSourceUnavailable","node":node,"owner":{"kind":"Named","name":name},"scope":null,"scope_metadata_available":false}),
        _=>json!({"kind":"UnexpectedError","complete_debug":format!("{error:#?}"),"display":error.to_string()}),
    }
}
fn runtime_chain(error: &RuntimeError) -> Vec<Json> { let mut chain=Vec::new();let mut next=error.source();while let Some(cause)=next {chain.push(json!({"debug":format!("{cause:#?}"),"display":cause.to_string()}));next=cause.source();}chain }
fn typed_error(error: &RuntimeError) -> Json {
    match error {
        RuntimeError::NotABool{node,found} | RuntimeError::NotAnItemCount{node,found}=>json!({"variant":if matches!(error,RuntimeError::NotABool{..}) {"NotABool"} else {"NotAnItemCount"},"node":node,"found":found,"display":error.to_string(),"source_chain_length":runtime_chain(error).len(),"typed_inner_null":error.source().is_none()}),
        RuntimeError::MappingException{node,message}=>json!({"variant":"MappingException","node":node,"mapping_exception_message":message,"display":error.to_string(),"source_chain_length":runtime_chain(error).len(),"typed_inner_null":error.source().is_none()}),
        RuntimeError::OwnedSourceAtNode{node,source:source @ codegen_runtime::OwnedSourceError::SourceUnavailable{name}}=>json!({"variant":"OwnedSourceAtNode","node":node,"scope":null,"scope_metadata_available":false,"source":{"variant":"SourceUnavailable","owner":{"kind":"Named","name":name}},"display":error.to_string(),"source_chain_length":runtime_chain(error).len(),"typed_inner_null":false,"original_source_is_typed_cause":error.source().is_some_and(|actual|std::ptr::eq(actual as *const dyn Error as *const (),source as *const _ as *const ()))}),
        _=>json!({"variant":"UnexpectedError","complete_debug":format!("{error:#?}"),"complete_chain":runtime_chain(error)}),
    }
}
fn retain_capture(id: &str, actual: &Result<Json,RuntimeError>, call: &Json) {
    let record=json!({"id":id,"backend":"rust","classification":"raw_return_before_any_classification","complete_call_spec":call,"snapshot":match actual {Ok(value)=>value.clone(),Err(error)=>error_snapshot(error)},"typed_error":actual.as_ref().err().map(typed_error),"complete_error_debug":actual.as_ref().err().map(|error|format!("{error:#?}")),"complete_error_chain":actual.as_ref().err().map(runtime_chain)});
    eprintln!("{record}");
    if let Some(directory)=std::env::var_os("FERRULE_CONFORMANCE_CAPTURE_DIR") { use std::io::Write;let path=std::path::PathBuf::from(directory).join(format!("{id}.capture.json"));let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(path).unwrap();file.write_all(format!("{}\n",serde_json::to_string_pretty(&record).unwrap()).as_bytes()).unwrap();file.sync_all().unwrap(); }
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
    assert_eq!(specs["fixture"],"basic-owned-windows-v1");assert_eq!(specs["exact_counts"],json!({"models":10,"calls_per_backend":12,"total_calls":36}));
    assert_eq!(specs["models"].as_array().unwrap().len(),10);assert_eq!(specs["calls"].as_array().unwrap().len(),12);assert_eq!(expected["calls"].as_array().unwrap().len(),36);
    let execution=ExecutionContext::new(Path::new(specs["execution"]["mapping_path"].as_str().unwrap()));
    let policy=limits(&specs);let mut captured:Vec<(String,Result<Json,RuntimeError>)>=Vec::new();
    macro_rules! model_calls {
        ($module:ident,$label:literal)=>{{
            for call in specs["calls"].as_array().unwrap().iter().filter(|call|call["model"]==$label) {
                let id=format!("WINDOW-rust-{}",call["case"].as_str().unwrap());
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
                retain_capture(&id,&actual,call);captured.push((id,actual));
            }
        }};
    }
    model_calls!(operators,"operators");
    model_calls!(filter_then_sort,"filter_then_sort");
    model_calls!(sort_then_filter,"sort_then_filter");
    model_calls!(nested_parent_bounds,"nested_parent_bounds");
    model_calls!(empty_bounds_eager,"empty_bounds_eager");
    model_calls!(zero_window_non_bool,"zero_window_non_bool");
    model_calls!(invalid_bound,"invalid_bound");
    model_calls!(fts_predicate_priority,"fts_predicate_priority");
    model_calls!(stf_key_priority,"stf_key_priority");
    model_calls!(first_predicate_priority,"first_predicate_priority");
    assert_eq!(captured.len(),12);
    eprintln!("{}",json!({"classification":"raw_Rust12_complete","calls":12,"expected_results_compared":false,"full_window_parity":false,"full_error_parity":false,"total_parity":false}));
}
