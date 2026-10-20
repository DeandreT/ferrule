//! Finite phase/sibling expansion of literal refusal recipes, all UNRUN.
//! Cloning an authored recipe is not lowering it or observing its error.
use super::*;
use codegen::{Expression as E,GeneratedSequence as G,ProgramValidationError as Error};
fn expected_operator(id:&str)->Error{
 closed::expression_error(991,if id.contains("aggregate")&&!id.contains("sequence")||id.contains("lookup")||id.contains("find"){
  codegen::OwnedSourceCapabilityReason::AggregateLookup
 }else{codegen::OwnedSourceCapabilityReason::GeneratedItem})
}
fn item_project(p:&mut Project){
 p.graph.nodes.insert(992,Node::SourceField{path:vec![],frame:None});
 p.graph.nodes.insert(993,Node::SequenceItemAt{sequence:mapping::SequenceExpr::Generate{from:None,to:912,item:992},index:5});
}
fn put(p:&mut codegen::Program,id:u32,expression:E){p.expressions.retain(|n|n.id!=id);p.expressions.push(codegen::ExpressionNode{id,expression});}
fn item_program(p:&mut codegen::Program){
 put(p,992,E::SourceField{path:vec![],frame:None});
 put(p,993,E::SequenceItemAt{sequence:G::Range{from:None,to:912,item:992},index:5});
}
fn failure_error(predicate:bool)->Error{
 use codegen::{OwnedSourceDiagnostic as D,OwnedSourceDiagnosticLocation as L,OwnedSourceDiagnosticRequest as Q,OwnedSourceDiagnosticOwner as O,OwnedSourceNamespace as N,OwnedSourceCapabilityOrigin as Origin,OwnedSourceCapabilityReason as R};
 Error::OwnedSource{diagnostic:Box::new(D::Capability{
  location:L::Expression{namespace:N::Main,node:if predicate{5}else{3},scope:None},
  request:Some(if predicate{Q::Position{owner:O::Primary,collection:Some(projects::path(&["Rows"]))}}else{Q::Field{owner:O::Primary,path:projects::path(&["Rows","Keep"]),frame:Some(projects::path(&["Rows"]))}}),
  reason:R::FailureRule,origin:Origin::Failure{index:0}
 })}
}
fn dynamic_error()->Error{
 use codegen::{OwnedSourceDiagnostic as D,OwnedSourceDiagnosticLocation as L,OwnedSourceDiagnosticRequest as Q,OwnedSourceDiagnosticOwner as O,OwnedSourceNamespace as N,OwnedSourceCapabilityOrigin as Origin,OwnedSourceCapabilityReason as R};
 Error::OwnedSource{diagnostic:Box::new(D::Capability{
  location:L::Expression{namespace:N::Main,node:1,scope:Some(closed::route())},
  request:Some(Q::Field{owner:O::Named{name:"unavailableDynamic".into()},path:projects::path(&["Rows","Start"]),frame:Some(projects::path(&["Rows"]))}),
  reason:R::DynamicOwner,origin:Origin::DynamicSource{index:3,name:"unavailableDynamic".into()}
 })}
}
pub fn projects()->Vec<(String,Project,Vec<codegen::Diagnostic>)>{
 let mut out=Vec::new();
 for(id,p,_)in closed::projects().into_iter().filter(|(id,_,_)|id.starts_with("Q04-project-")){
  for phase in["boundary","post","under-ItemAt","mixed-sibling"]{
   let mut p=p.clone();p.root.children[0].bindings[0].node=2;
   match phase{
    "boundary"=>p.root.children[0].group_starting_with=Some(991),
    "post"=>p.root.children[0].post_group_filter=Some(991),
    "under-ItemAt"=>{item_project(&mut p);let Node::SequenceItemAt{index,..}=p.graph.nodes.get_mut(&993).unwrap()else{unreachable!()};*index=991;p.root.children[0].bindings[0].node=993;},
    _=>{item_project(&mut p);projects::call(&mut p,994,"equal",&[993,991]);p.root.children[0].bindings[0].node=994;}
   }
   out.push((format!("{id}-{phase}"),p,vec![closed::diagnostic(expected_operator(id))]));
  }
 }
 let mut p=projects::base();p.graph.nodes.insert(1,projects::field("unavailableDynamic",&["Rows","Start"],Some(&["Rows"])));
 out.push(("Q05-project-dynamic-owner".into(),p,vec![closed::diagnostic(dynamic_error())]));
 let mut p=projects::base();p.root.children[0].children[0].iteration=ScopeIteration::Sequence(mapping::SequenceExpr::Generate{from:None,to:912,item:990});p.graph.nodes.insert(990,Node::SourceField{path:vec![],frame:None});
 // This exact complete iteration diagnostic is independent of any invocation.
 let scope=closed::route().descendant(mapping::SourceScopeStep::Child(0));
 let e=Error::OwnedSource{diagnostic:Box::new(codegen::OwnedSourceDiagnostic::Capability{
  location:codegen::OwnedSourceDiagnosticLocation::Iteration{scope:scope.clone()},
  request:Some(codegen::OwnedSourceDiagnosticRequest::Iteration{owner:codegen::OwnedSourceDiagnosticOwner::Primary,path:projects::path(&["Rows"])}),
  reason:codegen::OwnedSourceCapabilityReason::GeneratedItem,origin:codegen::OwnedSourceCapabilityOrigin::Scope{scope}
 })};out.push(("Q05-project-generated-member".into(),p,vec![closed::diagnostic(e)]));
  let mut fm=projects::base();for id in[990,994]{fm.graph.nodes.insert(id,Node::SourceField{path:vec![],frame:None});}
  for(id,ty,node)in[(1,ScalarType::Bool,Node::Const{value:Value::Bool(true)}),(2,ScalarType::Int,Node::FunctionParameter{parameter:mapping::FunctionParameterId::new(1)})]{
   fm.user_functions.insert(mapping::FunctionId::new(id),mapping::UserFunction{library:"corpus443".into(),name:format!("stage{id}"),description:None,parameters:vec![mapping::FunctionParameter{id:mapping::FunctionParameterId::new(1),name:"value".into(),ty:ScalarType::Int},mapping::FunctionParameter{id:mapping::FunctionParameterId::new(2),name:"position".into(),ty:ScalarType::Int}],output_name:"value".into(),output_type:ty,body:Graph{nodes:BTreeMap::from([(0,node)])},output:0});
  }
  fm.graph.nodes.insert(991,Node::SequenceItemAt{sequence:mapping::SequenceExpr::FilterMapV1(mapping::FilterMapV1{source:Box::new(mapping::SequenceExpr::Generate{from:None,to:912,item:990}),item:994,predicate:mapping::FunctionId::new(1),mapper:mapping::FunctionId::new(2),output_type:ScalarType::Int,captures:vec![]}),index:5});fm.root.children[0].bindings[0].node=991;
  out.push(("Q05-project-FilterMap-ItemAt".into(),fm,vec![closed::diagnostic(closed::expression_error(991,codegen::OwnedSourceCapabilityReason::GeneratedItem))]));
  for(id,predicate)in[("Q05-project-failure-predicate",true),("Q05-project-failure-message",false)]{
  let mut p=projects::base();p.graph.nodes.insert(990,Node::SourceField{path:vec![],frame:None});p.graph.nodes.insert(991,Node::SequenceItemAt{sequence:mapping::SequenceExpr::Generate{from:Some(911),to:913,item:990},index:5});
  p.failure_rules.push(mapping::FailureRule{iteration:mapping::FailureIteration::Source{collection:projects::path(&["Rows"])},selection:mapping::FailureSelection::WhenTrue{predicate:if predicate{991}else{3}},message:(!predicate).then_some(991)});
  out.push((id.into(),p,vec![closed::diagnostic(failure_error(predicate))]));
 }
 out
}
pub fn programs(base:&codegen::Program)->Vec<(String,codegen::Program,Error)>{
 let mut out=Vec::new();
 for(id,name)in[("E21-direct-unknown-owner","Unknown"),("E21-direct-trimmed-owner","Røws💠"),("E21-direct-case-owner","rows")]{
  let mut p=base.clone();put(&mut p,1,E::OwnedSourceField(mapping::OwnedSourceField{source:mapping::OwnedSourceRef{owner:projects::owner(name),path:projects::path(&["Rows","Start"])},frame:Some(projects::path(&["Rows"]))}));
  let expected=Error::OwnedSource{diagnostic:Box::new(codegen::OwnedSourceDiagnostic::Structure{location:codegen::OwnedSourceDiagnosticLocation::Expression{namespace:codegen::OwnedSourceNamespace::Main,node:1,scope:None},request:codegen::OwnedSourceDiagnosticRequest::Field{owner:codegen::OwnedSourceDiagnosticOwner::Named{name:name.into()},path:projects::path(&["Rows","Start"]),frame:Some(projects::path(&["Rows"]))},reason:codegen::OwnedSourceStructureReason::UnknownOwner})};out.push((id.into(),p,expected));
 }
 for(id,p,_)in closed::programs(base).into_iter().filter(|(id,_,_)|id.starts_with("Q04-")&&!id.ends_with("under-ItemAt")){
  for phase in["boundary","post","mixed-sibling"]{
   let mut p=p.clone();p.root.children[0].bindings[0].expression=2;
   match phase{
    "boundary"=>p.root.children[0].iteration=Some(codegen::IterationPlan::owned_source(mapping::OwnedSourceRef{owner:mapping::SourceOwner::Primary,path:projects::path(&["Rows"])}).with_grouping(codegen::GroupingPlan::StartingWith{predicate:991})),
    "post"=>p.root.children[0].iteration=Some(codegen::IterationPlan::owned_source(mapping::OwnedSourceRef{owner:mapping::SourceOwner::Primary,path:projects::path(&["Rows"])}).with_filtered_grouping(codegen::GroupingPlan::StartingWith{predicate:1},991)),
    _=>{item_program(&mut p);put(&mut p,994,E::Call{function:codegen::ScalarFunction::Equal,args:vec![993,991]});p.root.children[0].bindings[0].expression=994;}
   }
   out.push((format!("{id}-{phase}"),p,expected_operator(id)));
  }
 }
 out
}
