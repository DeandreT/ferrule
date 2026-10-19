//! #443 source fixture facade. UNRUN; not a preparation/execution entry point.
//! Mount beside the existing #442 harness. Integration is specified separately.
use super::*;
use serde_json::{Value as Json,json};
#[path="../../../../conformance/basic-owned-group-starting-with/projects.rs"]
mod projects;
#[path="../../../../conformance/basic-owned-group-starting-with/cases.rs"]
mod cases;
#[path="../../../../conformance/basic-owned-group-starting-with/catalogues.rs"]
mod catalogues;
#[path="../../../../conformance/basic-owned-group-starting-with/closed.rs"]
mod closed;
#[path="../../../../conformance/basic-owned-group-starting-with/closed_specialized.rs"]
mod closed_specialized;
#[path="../../../../conformance/basic-owned-group-starting-with/closed_matrix.rs"]
mod closed_matrix;
#[path="../../../../conformance/basic-owned-group-starting-with/old_profiles.rs"]
mod old_profiles;
use super::basic_owned_sequence_item_at::codec;
struct Case{
 id:&'static str,profile:&'static str,source:Instance,
 inputs:Vec<(String,Instance)>,calls:Vec<Json>,native:bool,
}
const MODELS:usize=105;
const CASES:usize=109;
const CORE_CALLS:usize=381;
const LEGACY_CALLS:usize=2;
const CLOSED_PROJECTS:usize=65;
const DIRECT_PROGRAMS:usize=61;
/// Materialization is deliberately not invoked in this source-authoring lane.
/// Expectations are expanded from source literals, never from lowered Programs.
fn corpus()->(Json,Vec<Project>,Vec<String>){
 let mut profiles=Vec::<String>::new();let mut projects=Vec::new();let mut records=Vec::new();let mut calls=0;
 for case in cases::cases(){
  calls+=case.calls.len();
  let model=if let Some(i)=profiles.iter().position(|p|p==case.profile){i}else{profiles.push(case.profile.into());projects.push(projects::project(case.profile));profiles.len()-1};
  records.push(json!({"id":case.id,"profile":case.profile,"model":model,"native":case.native,
   "source":codec::encode(&case.source),"inputs":case.inputs.iter().map(|(name,tree)|json!([name,codec::encode(tree)])).collect::<Vec<_>>(),"calls":case.calls}));
 }
 assert_eq!(profiles.len(),MODELS);assert_eq!(projects::PROFILES.len(),MODELS);assert_eq!(records.len(),CASES);assert_eq!(calls,CORE_CALLS);
 (json!({"version":1,"status":"UNRUN","models":profiles,"cases":records,"case_count":CASES,"core_calls_per_engine":CORE_CALLS,"legacy_calls_per_engine":LEGACY_CALLS}),projects,profiles)
}
fn closed_projects()->Vec<(String,Project,Vec<codegen::Diagnostic>)>{
 let controls=closed::projects().into_iter().chain(closed_specialized::projects()).map(|(id,p,e)|(id.into(),p,e)).chain(closed_matrix::projects()).collect::<Vec<_>>();assert_eq!(controls.len(),CLOSED_PROJECTS);controls
}
/// Both bases must be independently retained by preparation. The specialized
/// base is projects::refusal_base(), not the functional named-target model.
fn closed_programs(base:&codegen::Program,specialized_base:&codegen::Program)->Vec<(String,codegen::Program,codegen::ProgramValidationError)>{
 let controls=closed::programs(base).into_iter().chain(closed_specialized::programs(specialized_base)).map(|(id,p,e)|(id.into(),p,e)).chain(closed_matrix::programs(base)).collect::<Vec<_>>();assert_eq!(controls.len(),DIRECT_PROGRAMS);controls
}
