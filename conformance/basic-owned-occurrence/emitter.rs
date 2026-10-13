use codegen::ArtifactSet;
use engine::{EngineError, ExecutionContext, ExecutionOutputs, OwnedSourceError, SelectedTargetOutput, TargetSelection};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, NamedSource, Node, OwnedPosition, OwnedSourceField, OwnedSourceRef, Project, Scope, ScopeIteration, SourceOwner};
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
fn retain_json(path: &Path, value: &Json) { retain(path, format!("{}\n", serde_json::to_string_pretty(value).unwrap()).as_bytes()); }
fn string(value: &str) -> Instance { Instance::Scalar(Value::String(value.into())) }
fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(fields.into_iter().map(|(name, value)| (name.into(), value)).collect::<Vec<_>>().into())
}
fn row(value: &str) -> Instance { group(vec![("Value", string(value))]) }
fn original(case: &str) -> Instance {
    match case {
        "mixed" => Instance::Repeated(vec![Instance::Scalar(Value::Null), row("kept1"), Instance::Scalar(Value::json_null()), row("kept2")]),
        "empty" => Instance::Repeated(vec![]),
        "singleton" => row("singleton"),
        _ => panic!("unknown frozen case"),
    }
}
fn original_inputs(program: &str, case: &str) -> (Instance, Vec<(String, Instance)>) {
    if program == "primary" { (original(case), vec![]) } else {
        (group(vec![("Value", string("primary.shadow")), ("Ref", row("embedded.shadow"))]), vec![("Ref".into(), original(case))])
    }
}
fn root_schema() -> SchemaNode { SchemaNode::group("Input", vec![SchemaNode::scalar("Value", ScalarType::String)]) }
fn owner(program: &str) -> SourceOwner {
    match program { "primary" => SourceOwner::Primary, "named" => SourceOwner::Named { name: "Ref".into() }, _ => panic!("unknown frozen owner") }
}
fn project(program: &str) -> Project {
    let owned = owner(program);
    let mut graph = Graph::default();
    graph.nodes.insert(1, Node::OwnedSourceField(OwnedSourceField { source: OwnedSourceRef { owner: owned.clone(), path: vec!["Value".into()] }, frame: Some(vec![]) }));
    graph.nodes.insert(2, Node::OwnedPosition(OwnedPosition { owner: owned.clone(), collection: Some(vec![]) }));
    let source = if program == "primary" { root_schema() } else {
        SchemaNode::group("Primary", vec![SchemaNode::scalar("Value", ScalarType::String), SchemaNode::group("Ref", vec![SchemaNode::scalar("Value", ScalarType::String)])])
    };
    let extra_sources = if program == "primary" { vec![] } else { vec![NamedSource { name: "Ref".into(), path: "unused-basic-occurrence-input.json".into(), schema: root_schema(), options: Default::default(), dynamic_path: None }] };
    Project {
        source, target: SchemaNode::group("Output", vec![SchemaNode::scalar("Value", ScalarType::String), SchemaNode::scalar("Position", ScalarType::Int)]),
        source_path: None, target_path: None, source_options: Default::default(), target_options: Default::default(),
        extra_sources, extra_targets: vec![], failure_rules: vec![], user_functions: BTreeMap::new(), graph,
        root: Scope { iteration: ScopeIteration::OwnedSource { source: OwnedSourceRef { owner: owned, path: vec![] }, output_path: None }, bindings: vec![Binding { target_field: "Value".into(), node: 1 }, Binding { target_field: "Position".into(), node: 2 }], ..Scope::default() },
    }
}
fn tree(instance: &Instance) -> Json {
    match instance {
        Instance::Scalar(value) => {
            let kind = match value { Value::Null => "Null", Value::JsonNull(_) => "JsonNull", Value::Bool(_) => "Bool", Value::Int(_) => "Int", Value::Float(_) => "Float", Value::String(_) => "String", Value::XmlNil(_) => "XmlNil" };
            json!({"kind":"Scalar", "type":kind, "value":value})
        }
        Instance::Group(fields) => json!({"kind":"Group", "fields":fields.iter().map(|(name,value)| json!({"name":name,"value":tree(value)})).collect::<Vec<_>>()}),
        Instance::Repeated(items) => json!({"kind":"Repeated", "items":items.iter().map(tree).collect::<Vec<_>>()}),
        Instance::DocumentSet(_) => json!({"kind":"DocumentSet", "complete_serialized_instance":instance, "complete_debug":format!("{instance:#?}")}),
        Instance::MappedSequence(items) => json!({"kind":"MappedSequence", "items":items.iter().map(tree).collect::<Vec<_>>()}),
    }
}
fn error_chain(error: &dyn Error) -> Vec<Json> {
    let mut result = vec![json!({"debug":format!("{error:#?}"),"display":error.to_string()})];
    let mut next = error.source();
    while let Some(cause) = next { result.push(json!({"debug":format!("{cause:#?}"),"display":cause.to_string()})); next = cause.source(); }
    result
}
fn owner_json(value: &SourceOwner) -> Json {
    match value { SourceOwner::Primary => json!({"is_primary":true,"name":null}), SourceOwner::Named { name } => json!({"is_primary":false,"name":name}) }
}
fn error_semantic(error: &EngineError) -> Json {
    match error {
        EngineError::OwnedSourceAccess { source: OwnedSourceError::InactiveSourceFrame { owner, collection: Some(collection) }, .. } => json!({"kind":"Error","semantic_kind":"InactiveSourceFrame","owner":owner_json(owner),"collection":collection}),
        _ => json!({"kind":"UnexpectedError","complete_debug":format!("{error:#?}"),"complete_chain":error_chain(error)}),
    }
}
#[derive(Debug)]
enum NativeReturn { All(Result<ExecutionOutputs, EngineError>), Selected(Result<SelectedTargetOutput, EngineError>) }
fn snapshot(actual: &NativeReturn) -> Json {
    match actual {
        NativeReturn::All(Ok(output)) => json!({"kind":"All","primary":tree(&output.primary),"extras":output.extras.iter().map(|output|json!({"name":output.name,"instance":tree(&output.instance)})).collect::<Vec<_>>()}),
        NativeReturn::Selected(Ok(SelectedTargetOutput::Primary(instance))) => json!({"kind":"SelectedPrimary","instance":tree(instance)}),
        NativeReturn::Selected(Ok(other)) => json!({"kind":"UnexpectedSelection","complete_debug":format!("{other:#?}")}),
        NativeReturn::All(Err(error)) | NativeReturn::Selected(Err(error)) => error_semantic(error),
    }
}
fn error_of(actual: &NativeReturn) -> Option<&EngineError> {
    match actual { NativeReturn::All(Err(error)) | NativeReturn::Selected(Err(error)) => Some(error), _ => None }
}
fn expected_row<'a>(expected: &'a Json, id: &str) -> &'a Json {
    let rows = expected["calls"].as_array().unwrap().iter().filter(|row| row["id"] == id).collect::<Vec<_>>();
    assert_eq!(rows.len(), 1, "exact frozen call inventory"); rows[0]
}
fn compare_native(id: &str, actual: &NativeReturn, expected: &Json, program: &str) {
    let row = expected_row(expected, id); assert_eq!(snapshot(actual), row["expected"], "full typed native return mismatch; originals retained");
    if let Some(error) = error_of(actual) {
        assert_eq!(error, &EngineError::OwnedSourceAccess { node: Some(2), scope: None, source: OwnedSourceError::InactiveSourceFrame { owner: owner(program), collection: Some(vec![]) } });
        let cause = error.source().unwrap().downcast_ref::<OwnedSourceError>().unwrap();
        assert_eq!(cause, &OwnedSourceError::InactiveSourceFrame { owner: owner(program), collection: Some(vec![]) });
    }
}
fn artifact_manifest(out: &Path, label: &str, backend: &str, artifacts: &ArtifactSet) {
    let files = artifacts.files().iter().map(|file| json!({"path":file.path.as_str(),"bytes":file.contents.len(),"sha256":format!("{:x}",Sha256::digest(&file.contents))})).collect::<Vec<_>>();
    retain_json(&out.join(format!("{label}.{backend}.ArtifactSet.manifest.json")), &json!({"program":label,"backend":backend,"files":files,"complete_file_count":artifacts.len(),"all_artifact_contents_hashed":true,"runtime_source_world_copied":false}));
}
fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>(); assert_eq!(args.len(), 4, "fresh output, readonly fixture, Rust runtime package, Runtime DLL");
    let out = PathBuf::from(&args[0]); let fixture = PathBuf::from(&args[1]); let runtime = PathBuf::from(&args[2]); let dll = PathBuf::from(&args[3]);
    assert!([&out,&fixture,&runtime,&dll].iter().all(|path|path.is_absolute())); fs::create_dir(&out).expect("fresh namespace; prior originals preserved");
    let specs: Json = serde_json::from_slice(&fs::read(fixture.join("programs.json")).unwrap()).unwrap();
    let expected: Json = serde_json::from_slice(&fs::read(fixture.join("expected.json")).unwrap()).unwrap();
    assert_eq!(specs["programs"].as_array().unwrap().len(),2); assert_eq!(specs["cases"],json!(["mixed","empty","singleton"])); assert_eq!(specs["selections"],json!(["all","primary"])); assert_eq!(expected["calls"].as_array().unwrap().len(),36);
    assert_eq!(specs["nodes"],json!([{"id":1,"kind":"OwnedSourceField","path":["Value"],"frame":[],"frame_is_present":true},{"id":2,"kind":"OwnedPosition","collection":[],"collection_is_present":true}]));
    assert_eq!(specs["programs"],json!([{"id":"primary","owner":{"is_primary":true,"name":null}},{"id":"named","owner":{"is_primary":false,"name":"Ref"}}]));
    assert_eq!(specs["selected_endpoint"],json!({"kind":"Group","name":"Input","repeating":false,"children":[{"name":"Value","kind":"Scalar","type":"String"}]}));
    assert_eq!(specs["named_primary_collision"],json!({"Value":"primary.shadow","Ref":{"Value":"embedded.shadow"}}));
    assert_eq!(specs["root_scope"],json!({"construction":"Constructed","iteration":"OwnedSource","path":[],"output_path":null,"output":"Repeated","bindings":[{"target_field":"Value","node":1,"type":"String"},{"target_field":"Position","node":2,"type":"Int"}],"filter":null,"sort":null,"windows":[],"grouping":null,"children":[]}));
    assert_eq!(specs["limits"],json!({"max_roots":16,"max_active_frames":8,"max_path_segments":16,"max_metadata_bytes":1048576,"max_walk_contexts":64}));
    for key in ["extra_targets","failure_rules","user_functions"] { assert_eq!(specs[key],json!([])); }
    retain(&out.join("programs.original.json"), &fs::read(fixture.join("programs.json")).unwrap()); retain(&out.join("expected.original.json"), &fs::read(fixture.join("expected.json")).unwrap());
    let context = ExecutionContext::new(Path::new("basic-occurrence.mapping"));
    let mut captured = vec![]; let mut library = String::new(); let mut emission_failed = false;
    for label in ["primary","named"] {
        let project = project(label);
        assert!(!project.source.repeating && !project.target.repeating); assert!(project.extra_targets.is_empty() && project.failure_rules.is_empty() && project.user_functions.is_empty());
        retain_json(&out.join(format!("{label}.Project.json")), &serde_json::to_value(&project).unwrap());
        let validation = engine::validate(&project); retain(&out.join(format!("{label}.validation.debug")),format!("{validation:#?}\n").as_bytes()); assert!(validation.is_empty());
        let lowered = codegen::lower(&project); retain(&out.join(format!("{label}.lower.full-return.debug")),format!("{lowered:#?}\n").as_bytes()); let program = lowered.expect("public lowering; full negative retained");
        retain(&out.join(format!("{label}.Program.debug")),format!("{program:#?}\n").as_bytes());
        for case in ["mixed","empty","singleton"] {
            for selection in ["all","primary"] {
                let id = format!("OCCURRENCE-native-{label}-{case}-{selection}");
                let (primary, supplied) = original_inputs(label, case);
                let before = json!({"id":id,"classification":"before_subject","project":project,"primary":tree(&primary),"supplied":supplied.iter().map(|(name,input)|json!({"name":name,"instance":tree(input)})).collect::<Vec<_>>(),"selection":selection,"execution":{"mapping_path":"basic-occurrence.mapping","host_nodes":false,"loader":false,"parameters":false}});
                retain_json(&out.join(format!("{id}.input.json")), &before); println!("{before}");
                let actual = if selection == "all" { NativeReturn::All(engine::run_outputs_with_sources_and_context(&project,&primary,supplied,&context)) } else { NativeReturn::Selected(engine::run_selected_target_with_sources_and_context(&project,&primary,supplied,&context,TargetSelection::Primary)) };
                let record = json!({"id":id,"classification":"before_comparison","complete_return_debug":format!("{actual:#?}"),"snapshot":snapshot(&actual),"complete_error_chain":error_of(&actual).map(|error|error_chain(error))});
                retain_json(&out.join(format!("{id}.return.json")), &record); println!("{record}"); captured.push((id,label,actual));
            }
        }
        let options = codegen_rust::Options { package_name:"ferrule_basic_occurrence_generated_640".into(), runtime_dependency:codegen_rust::RuntimeDependency::Path(runtime.to_str().unwrap().into()) };
        let rust = codegen_rust::emit_with_basic_owned_sources(&program,&options); let csharp = codegen_csharp::emit_with_basic_owned_sources(&program);
        for (backend,error) in [("Rust",rust.as_ref().err().map(|error|error_chain(error))), ("CSharp",csharp.as_ref().err().map(|error|error_chain(error)))] {
            retain_json(&out.join(format!("{label}.{backend}.emission.full-return.json")), &json!({"program":label,"backend":backend,"success":error.is_none(),"complete_error_chain":error}));
        }
        match rust {
            Ok(files) => {
                artifact_manifest(&out,label,"Rust",&files); for file in files.files() { retain(&out.join("rust-artifacts").join(label).join(file.path.as_str()), &file.contents); }
                let source = files.files().iter().find(|file|file.path.as_str()=="src/lib.rs").unwrap(); library.push_str(&format!("pub mod {label} {{\n{}\n}}\n", std::str::from_utf8(&source.contents).unwrap()));
            }
            Err(_) => emission_failed = true,
        }
        match csharp {
            Ok(files) => {
                artifact_manifest(&out,label,"CSharp",&files); let root = out.join("csharp").join(label);
                for name in ["GeneratedMapping.cs","GeneratedTargetBuilder.cs"] { let file = files.files().iter().find(|file|file.path.as_str()==name).unwrap(); retain(&root.join("generated").join(name),&file.contents); }
                let caller = fs::read_to_string(fixture.join("csharp-caller.template.cs")).unwrap().replace("__PROGRAM__",label); retain(&root.join("consumer/Program.cs"),caller.as_bytes());
                let escaped = dll.to_string_lossy().replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;").replace('"',"&quot;");
                let host = fs::read_to_string(fixture.join("csharp-caller.csproj.template")).unwrap().replace("__RUNTIME_DLL__",&escaped); retain(&root.join("consumer/Host.csproj"),host.as_bytes()); retain(&root.join("expected.json"),&fs::read(fixture.join("expected.json")).unwrap());
            }
            Err(_) => emission_failed = true,
        }
    }
    retain(&out.join("rust-host/src/lib.rs"),library.as_bytes());
    let manifest = fs::read_to_string(fixture.join("rust-caller.Cargo.toml.template")).unwrap().replace("__RUNTIME_PATH__",&serde_json::to_string(runtime.to_str().unwrap()).unwrap()); retain(&out.join("rust-host/Cargo.toml"),manifest.as_bytes());
    retain(&out.join("rust-host/tests/occurrence.rs"),&fs::read(fixture.join("rust-caller.template.rs")).unwrap()); retain(&out.join("rust-host/expected.json"),&fs::read(fixture.join("expected.json")).unwrap());
    // Retain every original native return and emission result before comparisons.
    assert_eq!(captured.len(),12); assert!(!emission_failed,"all original emission negatives retained");
    for (id,label,actual) in &captured { compare_native(id,actual,&expected,label); }
    let classification = json!({"native_calls":12,"programs":2,"public_emissions":4,"native_complete_oracles_passed":true,"generated_calls":"UNRUN","complete_error_provenance_gap":{"native_node":2,"native_scope":null,"Rust_node_field_present":false,"CSharp_node":null},"full_error_parity":false,"total_parity":false});
    retain_json(&out.join("NATIVE_AND_EMISSION_CLASSIFICATION.json"),&classification); println!("{classification}");
}
