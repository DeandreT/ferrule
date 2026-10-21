use codegen::ArtifactSet;
use engine::{EngineError, ExecutionContext, ExecutionOutputs, SelectedTargetOutput, TargetSelection};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, NamedSource, NamedTarget, Node, OwnedPosition, OwnedSourceField, OwnedSourceRef, Project, Scope, ScopeIteration, SourceOwner};
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

fn retain(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() { fs::create_dir_all(parent).unwrap(); }
    let mut file = OpenOptions::new().write(true).create_new(true).open(path).unwrap();
    file.write_all(bytes).unwrap(); file.sync_all().unwrap();
}
fn retain_json(path: &Path, value: &Json) {
    retain(path, format!("{}\n", serde_json::to_string_pretty(value).unwrap()).as_bytes());
}
fn strings(value: &Json) -> Vec<String> {
    value.as_array().unwrap().iter().map(|item| item.as_str().unwrap().to_owned()).collect()
}
fn optional_strings(value: &Json) -> Option<Vec<String>> {
    if value.is_null() { None } else { Some(strings(value)) }
}
fn scalar_type(value: &str) -> ScalarType {
    match value { "Int" => ScalarType::Int, "Bool" => ScalarType::Bool, "String" => ScalarType::String, _ => panic!("unsupported frozen scalar type") }
}
fn value(input: &Json) -> Value {
    assert_eq!(input["kind"], "Scalar");
    match input["type"].as_str().unwrap() {
        "Null" => { assert!(input["value"].is_null()); Value::Null },
        "JsonNull" => { assert!(input["value"].is_null()); Value::json_null() },
        "Int" => Value::Int(input["value"].as_i64().unwrap()),
        "Bool" => Value::Bool(input["value"].as_bool().unwrap()),
        "String" => Value::String(input["value"].as_str().unwrap().to_owned()),
        _ => panic!("unsupported frozen input value"),
    }
}
fn instance(input: &Json) -> Instance {
    match input["kind"].as_str().unwrap() {
        "Scalar" => Instance::Scalar(value(input)),
        "Group" => Instance::Group(input["fields"].as_array().unwrap().iter().map(|field| {
            (field["name"].as_str().unwrap().to_owned(), instance(&field["value"]))
        }).collect::<Vec<_>>().into()),
        "Repeated" => Instance::Repeated(input["items"].as_array().unwrap().iter().map(instance).collect()),
        _ => panic!("unsupported frozen instance domain"),
    }
}
fn schema(input: &Json) -> SchemaNode {
    let name = input["name"].as_str().unwrap();
    let node = match input["kind"].as_str().unwrap() {
        "Scalar" => SchemaNode::scalar(name, scalar_type(input["type"].as_str().unwrap())),
        "Group" => SchemaNode::group(name, input["children"].as_array().unwrap().iter().map(schema).collect::<Vec<_>>()),
        _ => panic!("unsupported frozen schema domain"),
    };
    if input["repeating"].as_bool().unwrap() { node.repeating() } else { node }
}
fn owner(input: &Json) -> SourceOwner {
    match input["kind"].as_str().unwrap() {
        "Primary" => SourceOwner::Primary,
        "Named" => SourceOwner::Named { name: input["name"].as_str().unwrap().to_owned() },
        _ => panic!("unsupported frozen owner"),
    }
}
fn id(input: &Json) -> u32 { u32::try_from(input.as_u64().unwrap()).unwrap() }
fn optional_id(input: &Json) -> Option<u32> { if input.is_null() { None } else { Some(id(input)) } }
fn node(input: &Json) -> Node {
    match input["kind"].as_str().unwrap() {
        "OwnedSourceField" => Node::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef { owner: owner(&input["owner"]), path: strings(&input["path"]) },
            frame: optional_strings(&input["frame"]),
        }),
        "OwnedPosition" => Node::OwnedPosition(OwnedPosition { owner: owner(&input["owner"]), collection: optional_strings(&input["collection"]) }),
        "SourceField" => Node::SourceField { path: strings(&input["path"]), frame: optional_strings(&input["frame"]) },
        "Position" => Node::Position { collection: strings(&input["collection"]) },
        "Const" => Node::Const { value: value(&input["value"]) },
        "Call" => Node::Call { function: input["function"].as_str().unwrap().to_owned(), args: input["args"].as_array().unwrap().iter().map(id).collect() },
        "If" => Node::If { condition: id(&input["condition"]), then: id(&input["then"]), else_: id(&input["else"]) },
        "Raise" => Node::Raise { message: optional_id(&input["message"]) },
        _ => panic!("unsupported frozen graph node"),
    }
}
fn scope(input: &Json) -> Scope {
    assert_eq!(input["construction"], "Constructed");
    assert_eq!(input["iteration_output"], "Repeated");
    assert_eq!(input["iteration"]["kind"], "OwnedSource");
    assert!(input["iteration"]["output_path"].is_null());
    assert!(input["sort"].is_null() && input["grouping"].is_null());
    assert_eq!(input["windows"], json!([]));
    Scope {
        target_field: input["target_field"].as_str().unwrap().to_owned(),
        iteration: ScopeIteration::OwnedSource { source: OwnedSourceRef {
            owner: owner(&input["iteration"]["owner"]), path: strings(&input["iteration"]["path"]),
        }, output_path: None },
        filter: optional_id(&input["filter"]),
        bindings: input["bindings"].as_array().unwrap().iter().map(|binding| Binding {
            target_field: binding["target_field"].as_str().unwrap().to_owned(), node: id(&binding["node"]),
        }).collect(),
        children: input["children"].as_array().unwrap().iter().map(scope).collect(),
        ..Scope::default()
    }
}
fn project(input: &Json) -> Project {
    assert_eq!(input["user_functions"], json!([]));
    assert_eq!(input["options"], json!({"source_path":null,"target_path":null,"source_format_options":"Default","target_format_options":"Default"}));
    let mut graph = Graph::default();
    for entry in input["nodes"].as_array().unwrap() { assert!(graph.nodes.insert(id(&entry["id"]), node(entry)).is_none()); }
    Project {
        source: schema(&input["source_schema"]), target: schema(&input["target_schema"]),
        source_path: None, target_path: None, source_options: Default::default(), target_options: Default::default(),
        extra_sources: input["named_sources"].as_array().unwrap().iter().map(|source| NamedSource {
            name: source["name"].as_str().unwrap().to_owned(), path: "unused-filter-host-input.json".into(),
            schema: schema(&source["schema"]), options: Default::default(), dynamic_path: None,
        }).collect(),
        extra_targets: input["extra_targets"].as_array().unwrap().iter().map(|target| NamedTarget {
            name: target["name"].as_str().unwrap().to_owned(), path: None, schema: schema(&target["schema"]),
            options: Default::default(), root: scope(&target["root_scope"]),
        }).collect(),
        failure_rules: input["failure_rules"].as_array().unwrap().iter().map(|rule| mapping::FailureRule {
            iteration: mapping::FailureIteration::Source { collection: strings(&rule["collection"]) },
            selection: mapping::FailureSelection::WhenTrue { predicate: id(&rule["predicate"]) },
            message: optional_id(&rule["message"]),
        }).collect(),
        user_functions: BTreeMap::new(), graph, root: scope(&input["root_scope"]),
    }
}
fn tree(instance: &Instance) -> Json {
    match instance {
        Instance::Scalar(value) => {
            let kind = match value { Value::Null => "Null", Value::JsonNull(_) => "JsonNull", Value::Bool(_) => "Bool", Value::Int(_) => "Int", Value::Float(_) => "Float", Value::Decimal(_) => "Decimal", Value::String(_) => "String", Value::XmlNil(_) => "XmlNil" };
            let payload = match value { Value::Null | Value::JsonNull(_) | Value::XmlNil(_) => Json::Null, _ => serde_json::to_value(value).unwrap() };
            json!({"kind":"Scalar","type":kind,"value":payload})
        }
        Instance::Group(fields) => json!({"kind":"Group","fields":fields.iter().map(|(name,value)|json!({"name":name,"value":tree(value)})).collect::<Vec<_>>()}),
        Instance::Repeated(items) => json!({"kind":"Repeated","items":items.iter().map(tree).collect::<Vec<_>>()}),
        other => json!({"kind":"UnexpectedDomain","complete_serialized_instance":other,"complete_debug":format!("{other:#?}")}),
    }
}
fn error_chain(error: &dyn Error) -> Vec<Json> {
    let mut result = Vec::new(); let mut next = Some(error);
    while let Some(cause) = next { result.push(json!({"debug":format!("{cause:#?}"),"display":cause.to_string()})); next = cause.source(); }
    result
}
fn error_snapshot(error: &EngineError) -> Json {
    match error {
        EngineError::NotABool { node, found } => json!({"kind":"Error","category":"NotABool","node":node,"found":found}),
        EngineError::MappingException { node, message } => json!({"kind":"Error","category":"MappingException","node":node,"mapping_exception_message":message}),
        EngineError::MappingFailure { rule, message } => json!({"kind":"Error","category":"MappingFailure","rule":rule,"mapping_failure_message":message}),
        _ => json!({"kind":"UnexpectedError","complete_debug":format!("{error:#?}"),"complete_chain":error_chain(error)}),
    }
}
#[derive(Debug)]
enum NativeReturn { All(Result<ExecutionOutputs, EngineError>), Selected(Result<SelectedTargetOutput, EngineError>) }
fn snapshot(actual: &NativeReturn) -> Json {
    match actual {
        NativeReturn::All(Ok(output)) => json!({"kind":"All","primary":tree(&output.primary),"extras":output.extras.iter().map(|output|json!({"name":output.name,"instance":tree(&output.instance)})).collect::<Vec<_>>()}),
        NativeReturn::Selected(Ok(SelectedTargetOutput::Primary(output))) => json!({"kind":"SelectedPrimary","instance":tree(output)}),
        NativeReturn::Selected(Ok(SelectedTargetOutput::Named(output))) => json!({"kind":"SelectedNamed","name":output.name,"instance":tree(&output.instance)}),
        NativeReturn::All(Err(error)) | NativeReturn::Selected(Err(error)) => error_snapshot(error),
    }
}
fn error_of(actual: &NativeReturn) -> Option<&EngineError> {
    match actual { NativeReturn::All(Err(error)) | NativeReturn::Selected(Err(error)) => Some(error), _ => None }
}
fn expected_row<'a>(expected: &'a Json, id: &str) -> &'a Json {
    let matches = expected["calls"].as_array().unwrap().iter().filter(|row|row["id"] == id).collect::<Vec<_>>();
    assert_eq!(matches.len(), 1); matches[0]
}
fn compare(id: &str, actual: &NativeReturn, expected: &Json) {
    let row = expected_row(expected, id); assert_eq!(snapshot(actual), row["expected"], "complete native return differs; originals retained");
    if let Some(error) = error_of(actual) {
        let typed = &row["typed_error"];
        let expected_error = match typed["variant"].as_str().unwrap() {
            "NotABool" => EngineError::NotABool { node: id_from(typed, "node"), found: "int" },
            "MappingException" => EngineError::MappingException { node: id_from(typed, "node"), message: Some(typed["mapping_exception_message"].as_str().unwrap().to_owned()) },
            "MappingFailure" => EngineError::MappingFailure { rule: usize::try_from(typed["rule"].as_u64().unwrap()).unwrap(), message: Some(typed["mapping_failure_message"].as_str().unwrap().to_owned()) },
            _ => panic!("unsupported frozen typed native error"),
        };
        assert_eq!(error, &expected_error); assert_eq!(error.to_string(), typed["display"].as_str().unwrap());
        assert!(error.source().is_none()); assert_eq!(typed["source_chain_length"], 0); assert_eq!(typed["typed_inner_null"], true);
    } else { assert!(row["typed_error"].is_null()); }
}
fn id_from(input: &Json, name: &str) -> u32 { id(&input[name]) }
fn artifact_manifest(out: &Path, model: &str, backend: &str, project_digest: &str, artifacts: &ArtifactSet) {
    retain_json(&out.join(format!("{model}.{backend}.ArtifactSet.manifest.json")), &json!({
        "model":model,"backend":backend,"source_Project_sha256":project_digest,"complete_file_count":artifacts.len(),
        "files":artifacts.files().iter().map(|file|json!({"path":file.path.as_str(),"bytes":file.contents.len(),"sha256":format!("{:x}",Sha256::digest(&file.contents))})).collect::<Vec<_>>(),
        "all_artifact_contents_hashed":true,"runtime_source_world_copied":false,
    }));
}
fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>(); assert_eq!(args.len(), 4, "fresh output, readonly fixture, Rust runtime package, Runtime DLL");
    let out = PathBuf::from(&args[0]); let fixture = PathBuf::from(&args[1]); let runtime = PathBuf::from(&args[2]); let dll = PathBuf::from(&args[3]);
    assert!([&out,&fixture,&runtime,&dll].iter().all(|path|path.is_absolute())); fs::create_dir(&out).expect("fresh namespace; failures retained");
    let specs_body = fs::read(fixture.join("programs.json")).unwrap(); let expected_body = fs::read(fixture.join("expected.json")).unwrap();
    let specs: Json = serde_json::from_slice(&specs_body).unwrap(); let expected: Json = serde_json::from_slice(&expected_body).unwrap();
    assert_eq!(specs["fixture"], "basic-owned-nested-alias-v1"); assert_eq!(specs["issue"], 688);
    assert_eq!(specs["exact_counts"], json!({"models":6,"calls_per_backend":6,"total_calls":18}));
    assert_eq!(specs["models"].as_array().unwrap().len(),6); assert_eq!(specs["calls"].as_array().unwrap().len(),6); assert_eq!(expected["calls"].as_array().unwrap().len(),18);
    assert_eq!(specs["execution"], json!({"mapping_path":"basic-owned-nested-alias.mapping","runtime_parameters":null,"loader":false,"tracing":false,"debugging":false}));
    assert_eq!(specs["generated_limits"],json!({"max_roots":16,"max_active_frames":8,"max_path_segments":16,"max_metadata_bytes":1048576,"max_walk_contexts":64}));
    retain(&out.join("programs.original.json"), &specs_body); retain(&out.join("expected.original.json"), &expected_body);
    let execution = ExecutionContext::new(Path::new("basic-owned-nested-alias.mapping"));
    let mut captured = Vec::new(); let mut admissions = Vec::new(); let mut library = String::new(); let mut emission_failed = false;
    for model in specs["models"].as_array().unwrap() {
        let label = model["id"].as_str().unwrap(); let project = project(model);
        let project_body = serde_json::to_vec_pretty(&project).unwrap(); let project_digest = format!("{:x}",Sha256::digest(&project_body));
        retain(&out.join(format!("{label}.Project.json")), &project_body);
        let validation = engine::validate(&project); retain(&out.join(format!("{label}.validation.debug")),format!("{validation:#?}\n").as_bytes());
        for call in specs["calls"].as_array().unwrap().iter().filter(|call|call["model"] == label) {
            let id = format!("NEST-native-{}",call["case"].as_str().unwrap());
            let primary = instance(&call["primary"]); let supplied = call["supplied"].as_array().unwrap().iter().map(|input|(input["name"].as_str().unwrap().to_owned(),instance(&input["instance"]))).collect::<Vec<_>>();
            let selection = match call["selection"]["kind"].as_str().unwrap() { "All"=>None,"Primary"=>Some(TargetSelection::Primary),"Named"=>Some(TargetSelection::Named(call["selection"]["name"].as_str().unwrap())),_=>panic!("unknown selection") };
            let admission = engine::check_source_ownership_capability(&project, selection);
            let before = json!({"id":id,"classification":"before_subject","source_Project_sha256":project_digest,"project":project,"primary":tree(&primary),"supplied":supplied.iter().map(|(name,input)|json!({"name":name,"instance":tree(input)})).collect::<Vec<_>>(),"selection":call["selection"],"execution":specs["execution"],"native_limits":specs["native_limits"],"complete_public_admission":format!("{admission:#?}")});
            retain_json(&out.join(format!("{id}.input.json")),&before); println!("{before}");
            let actual = match selection { None=>NativeReturn::All(engine::run_outputs_with_sources_and_context(&project,&primary,supplied,&execution)),Some(selection)=>NativeReturn::Selected(engine::run_selected_target_with_sources_and_context(&project,&primary,supplied,&execution,selection)) };
            let record = json!({"id":id,"classification":"before_comparison","complete_return_debug":format!("{actual:#?}"),"snapshot":snapshot(&actual),"complete_error_chain":error_of(&actual).map(|error|error_chain(error))});
            retain_json(&out.join(format!("{id}.return.json")),&record); println!("{record}");
            admissions.push((id.clone(), admission)); captured.push((id, actual));
        }
        let lowered = codegen::lower_with_basic_owned_filters(&project); retain(&out.join(format!("{label}.lower.full-return.debug")),format!("{lowered:#?}\n").as_bytes());
        let program = lowered.expect("complete real-Project lowering result retained before assertion"); retain(&out.join(format!("{label}.Program.debug")),format!("{program:#?}\n").as_bytes());
        assert!(validation.is_empty(), "full validation negative retained");
        let options = codegen_rust::Options { package_name:"ferrule_basic_nested_alias_generated_688".into(),runtime_dependency:codegen_rust::RuntimeDependency::Path(runtime.to_str().unwrap().into()) };
        retain(&out.join(format!("{label}.Rust.Options.debug")),format!("{options:#?}\n").as_bytes());
        retain_json(&out.join(format!("{label}.CSharp.Options.json")),&json!({"public_API":"codegen_csharp::emit_with_basic_owned_filters","options":null}));
        let rust = codegen_rust::emit_with_basic_owned_filters(&program,&options); let csharp = codegen_csharp::emit_with_basic_owned_filters(&program);
        for (backend,error) in [("Rust",rust.as_ref().err().map(|error|error_chain(error))),("CSharp",csharp.as_ref().err().map(|error|error_chain(error)))] {
            retain_json(&out.join(format!("{label}.{backend}.emission.full-return.json")),&json!({"model":label,"backend":backend,"source_Project_sha256":project_digest,"success":error.is_none(),"complete_error_chain":error}));
        }
        match rust {
            Ok(files)=>{ artifact_manifest(&out,label,"Rust",&project_digest,&files); for file in files.files() { retain(&out.join("rust-artifacts").join(label).join(file.path.as_str()),&file.contents); }
                let source = files.files().iter().find(|file|file.path.as_str()=="src/lib.rs").unwrap(); library.push_str(&format!("pub mod {label} {{\n{}\n}}\n",std::str::from_utf8(&source.contents).unwrap())); },
            Err(_)=>emission_failed=true,
        }
        match csharp {
            Ok(files)=>{ artifact_manifest(&out,label,"CSharp",&project_digest,&files); let root=out.join("csharp").join(label);
                for name in ["GeneratedMapping.cs","GeneratedTargetBuilder.cs"] { let file=files.files().iter().find(|file|file.path.as_str()==name).unwrap();retain(&root.join("generated").join(name),&file.contents); }
                let caller=fs::read_to_string(fixture.join("csharp-caller.template.cs")).unwrap().replace("__PROGRAM__",label).replace("__EXPECTED_SHA256__",&format!("{:x}",Sha256::digest(&expected_body))).replace("__PROGRAMS_SHA256__",&format!("{:x}",Sha256::digest(&specs_body)));retain(&root.join("consumer/Program.cs"),caller.as_bytes());
                let escaped=dll.to_string_lossy().replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;").replace('"',"&quot;");
                let host=fs::read_to_string(fixture.join("csharp-caller.csproj.template")).unwrap().replace("__RUNTIME_DLL__",&escaped);retain(&root.join("consumer/Host.csproj"),host.as_bytes()); },
            Err(_)=>emission_failed=true,
        }
    }
    retain(&out.join("rust-host/src/lib.rs"),library.as_bytes());
    let manifest=fs::read_to_string(fixture.join("rust-caller.Cargo.toml.template")).unwrap().replace("__RUNTIME_PATH__",&serde_json::to_string(runtime.to_str().unwrap()).unwrap());retain(&out.join("rust-host/Cargo.toml"),manifest.as_bytes());
    retain(&out.join("rust-host/tests/aliases.rs"),&fs::read(fixture.join("rust-caller.template.rs")).unwrap());retain(&out.join("rust-host/programs.json"),&specs_body);retain(&out.join("rust-host/expected.json"),&expected_body);
    assert_eq!(captured.len(),6);assert_eq!(admissions.len(),6);assert!(!emission_failed,"all reached emission negatives retained");
    for (id,admission) in &admissions { assert!(admission.is_ok(),"complete public admission captured for {id}"); }
    if std::env::var_os("FERRULE_CONFORMANCE_CAPTURE_ONLY").is_some_and(|value| value == "1") {
        for (id, _) in &captured { println!("{id} capturedreturns"); }
        let classification = json!({"classification":"fixture_interpreter_and_emission_captured","interpreter":"Ferrule public engine","native_calls":6,"real_Project_models":6,"public_filter_emissions":12,"typed_failure_calls":0,"typed_failure_credit":false,"native_complete_oracles_passed":false,"native_complete_returns_captured":true,"oracle_comparisons":0,"generated_calls":"UNRUN","full_filter_parity":false,"full_error_parity":false,"total_parity":false});
        retain_json(&out.join("NATIVE_AND_EMISSION_CLASSIFICATION.json"), &classification); println!("{classification}");
        return;
    }
    for (id,actual) in &captured { compare(id,actual,&expected); println!("{id} matchedoracles"); }
    let classification=json!({"classification":"fixture_interpreter_and_emission_qualified","interpreter":"Ferrule public engine","native_calls":6,"real_Project_models":6,"public_filter_emissions":12,"typed_failure_calls":0,"typed_failure_credit":false,"native_complete_oracles_passed":true,"generated_calls":"UNRUN","full_filter_parity":false,"full_error_parity":false,"total_parity":false});
    retain_json(&out.join("NATIVE_AND_EMISSION_CLASSIFICATION.json"),&classification);println!("{classification}");
}
