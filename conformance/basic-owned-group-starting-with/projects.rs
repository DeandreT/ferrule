//! Genuine public Project definitions. Authored source only: all UNRUN.
use super::*;
use mapping::{NamedSource,NamedTarget,OwnedPosition,OwnedSourceField,OwnedSourceRef,SourceOwner,SequenceExpr,SequenceWindow as W,SortFilterOrder};
pub const EXACT:&str=" Røws💠 ";
pub const AUDIT:&str="audit";
pub const UNICODE_TARGET:&str="unicode";
pub const PROFILES:&[&str]=&[
 "S01","S02","S03","S04","S05","S06","S07","S08","S09","S10","S11","S12","S13","S14","S15","S16","S17","S18","S19","S20","S21","S22","S23-boundary","S23-post","S23-ordinary","S24",
 "X01","X02","X03","X04","X05","X06","X07-P","X07-N","X08-P","X08-N","X08-Alias","X09-P-true","X09-P-false","X09-N-true","X09-N-false","X09-Alias-true","X09-Alias-false","X10-ordinary","X11","X12-inactive","X12-bypass","X12-strict","X13","X14-scalar","X14-members","X15-by","X15-adjacent","X16","X17",
 "E01","E02","E03","E04","E05","E06","E07-zero","E07-first","E08","E09","E10-string","E10-null","E10-json-null","E10-int","E10-float","E10-xml-nil","E11","E12","E13","E14","E15","E16","E17","E18-window","E18-post","E18-reached","E19-position","E19-pin","E19-unpinned","E20-row","E20-root","E20-ordinary","E23-first","E23-last",
 "I01","I02-group","I02-member","I03","I04","I05","I06","I07-producer","I07-empty","I07-null","I08","C02","C03-global","C03-row","C05","Q02-islands"
];
pub fn path(parts:&[&str])->Vec<String>{parts.iter().map(|s|(*s).into()).collect()}
pub fn owner(name:&str)->SourceOwner{if name=="Primary"{SourceOwner::Primary}else{SourceOwner::Named{name:name.into()}}}
pub fn field(name:&str,parts:&[&str],pin:Option<&[&str]>)->Node{Node::OwnedSourceField(OwnedSourceField{source:OwnedSourceRef{owner:owner(name),path:path(parts)},frame:pin.map(path)})}
pub fn position(name:&str,parts:&[&str])->Node{Node::OwnedPosition(OwnedPosition{owner:owner(name),collection:Some(path(parts))})}
pub fn iteration(name:&str,parts:&[&str])->ScopeIteration{ScopeIteration::OwnedSource{source:OwnedSourceRef{owner:owner(name),path:path(parts)},output_path:None}}
pub fn bindings(fields:&[(&str,u32)])->Vec<Binding>{fields.iter().map(|(name,id)|Binding{target_field:(*name).into(),node:*id}).collect()}
pub fn mixed(name:&str)->SchemaNode{SchemaNode::scalar_union(name,ir::ScalarTypeSet::new([ScalarType::String,ScalarType::Bool,ScalarType::Int,ScalarType::Float]).unwrap())}
pub fn fields(schema:&mut SchemaNode)->&mut Vec<SchemaNode>{let ir::SchemaKind::Group{children,..}=&mut schema.kind else{panic!("authored group")};children}
pub fn row_schema()->SchemaNode{SchemaNode::group("Rows",vec![bool_("Start"),string("Label"),bool_("Keep"),int("Rank"),int("Limit"),string("Key")]).repeating()}
pub fn endpoint()->SchemaNode{SchemaNode::group("Endpoint",vec![string("Marker"),int("Limit"),row_schema()])}
fn member_schema()->SchemaNode{SchemaNode::group("Members",vec![string("Label"),int("MemberPosition"),int("LegacyPosition")]).repeating()}
pub fn groups_schema()->SchemaNode{SchemaNode::group("Groups",vec![string("First"),int("GroupPosition"),string("RootMarker"),member_schema()]).repeating()}
pub fn grouped(name:&str,offset:u32)->Scope{Scope{target_field:"Groups".into(),iteration:iteration(name,&["Rows"]),group_starting_with:Some(offset+1),bindings:bindings(&[("First",offset+2),("GroupPosition",offset+5),("RootMarker",offset+6)]),children:vec![Scope{target_field:"Members".into(),iteration:iteration(name,&["Rows"]),bindings:bindings(&[("Label",offset+2),("MemberPosition",offset+5),("LegacyPosition",600)]),..Default::default()}],..Default::default()}}
pub fn constant(p:&mut Project,id:u32,value:Value){p.graph.nodes.insert(id,Node::Const{value});}
pub fn call(p:&mut Project,id:u32,function:&str,args:&[u32]){p.graph.nodes.insert(id,Node::Call{function:function.into(),args:args.to_vec()});}
pub fn base()->Project{
 let mut nodes=BTreeMap::new();
 for (name,offset) in [("Primary",0),("Rows",200),(EXACT,300)]{
  for(id,leaf)in[(1,"Start"),(2,"Label"),(3,"Keep"),(4,"Rank"),(8,"Limit"),(9,"Key")]{nodes.insert(offset+id,field(name,&["Rows",leaf],Some(&["Rows"])));}
  nodes.insert(offset+5,position(name,&["Rows"]));nodes.insert(offset+6,field(name,&["Marker"],Some(&[])));nodes.insert(offset+7,field(name,&["Limit"],Some(&[])));
 }
 nodes.insert(600,Node::Position{collection:vec![]});
 let mut p=Project{source:endpoint(),target:SchemaNode::group("Output",vec![groups_schema()]),source_path:None,target_path:None,source_options:Default::default(),target_options:Default::default(),
  extra_sources:["unusedStatic","Rows",EXACT,"unavailableDynamic"].into_iter().map(|name|NamedSource{name:name.into(),path:format!("unused/{name}.json"),schema:endpoint(),options:Default::default(),dynamic_path:(name=="unavailableDynamic").then_some(mapping::DynamicSourcePath::legacy(900, vec![]))}).collect(),
  extra_targets:[(AUDIT,"Rows",200),(UNICODE_TARGET,EXACT,300)].into_iter().map(|(name,own,offset)|NamedTarget{name:name.into(),path:None,schema:SchemaNode::group("Output",vec![groups_schema()]),options:Default::default(),root:Scope{children:vec![grouped(own,offset)],..Default::default()}}).collect(),
  graph:Graph{nodes},root:Scope{children:vec![grouped("Primary",0)],..Default::default()},failure_rules:vec![],user_functions:Default::default()};
 for(id,value)in[(900,Value::String("unused.json".into())),(901,Value::Bool(true)),(902,Value::Bool(false)),(910,Value::Int(0)),(911,Value::Int(1)),(912,Value::Int(2)),(913,Value::Int(3)),(914,Value::Int(4)),(917,Value::Int(7)),(920,Value::String("A".into())),(921,Value::String("p2".into())),(923,Value::String("audit".into()))]{constant(&mut p,id,value);}
 for(id,fun,args)in[(30,"greater_than",vec![5,912]),(31,"less_or_equal",vec![5,913]),(32,"equal",vec![5,914]),(34,"equal",vec![5,912]),(35,"equal",vec![2,921])]{call(&mut p,id,fun,&args);}
 p.graph.nodes.insert(33,Node::If{condition:1,then:902,else_:901});
 for(id,msg)in[(101,"boundary"),(102,"filter"),(103,"sort"),(104,"bound"),(105,"post"),(106,"target"),(107,"last-bound")]{constant(&mut p,id+1000,Value::String(msg.into()));p.graph.nodes.insert(id,Node::Raise{message:Some(id+1000)});}
 p
}
/// Specialized refusal constructors require an independent scalar audit route.
/// It is not a counted functional model and never supplies mapping expectations.
pub fn refusal_base()->Project{let mut p=base();for(t,node)in p.extra_targets.iter_mut().zip([206,306]){t.schema=SchemaNode::group("Audit",vec![string("Marker")]);t.root=Scope{bindings:bindings(&[("Marker",node)]),..Default::default()};}p}
fn first(p:&mut Project){p.root.children[0].iteration_output=mapping::IterationOutput::First;fields(&mut p.target)[0].repeating=false;}
fn nested(p:&mut Project,outer_name:&str,outer:u32,inner_name:&str,inner:u32,window:W){
 let mut scope=grouped(outer_name,outer);scope.target_field="Outer".into();scope.windows=vec![window];scope.bindings=bindings(&[("OuterFirst",outer+2),("OuterPosition",outer+5)]);scope.children=vec![Scope{target_field:"Inner".into(),children:vec![grouped(inner_name,inner)],..Default::default()}];
 p.root.children=vec![scope];p.target=SchemaNode::group("Output",vec![SchemaNode::group("Outer",vec![string("OuterFirst"),int("OuterPosition"),SchemaNode::group("Inner",vec![groups_schema()])]).repeating()]);
}
fn s23(p:&mut Project,variant:&str){
 p.extra_targets.clear();p.graph.nodes.clear();constant(p,900,Value::String("unused.json".into()));
 for(id,leaf)in[(601,"Start"),(602,"Label")]{p.graph.nodes.insert(id,Node::SourceField{path:path(&[leaf]),frame:Some(path(&["Rows"]))});}
 p.graph.nodes.insert(600,Node::Position{collection:path(&["Rows"])});
 let boundary=if variant=="S23-boundary"{p.graph.nodes.insert(1,field("Primary",&["Rows","Start"],Some(&["Rows"])));1}else{601};
 let post=if variant=="S23-post"{p.graph.nodes.insert(3,field("Primary",&["Rows","Keep"],Some(&["Rows"])));call(p,605,"equal",&[3,3]);Some(605)}else{None};
 p.target=SchemaNode::group("Output",vec![SchemaNode::group("Groups",vec![string("First"),int("GroupPosition"),SchemaNode::group("Members",vec![string("Label"),int("MemberPosition")]).repeating()]).repeating()]);
 p.root=Scope{children:vec![Scope{target_field:"Groups".into(),iteration:ScopeIteration::Source(path(&["Rows"])),group_starting_with:Some(boundary),post_group_filter:post,bindings:bindings(&[("First",602),("GroupPosition",600)]),children:vec![Scope{target_field:"Members".into(),iteration:ScopeIteration::Source(vec![]),bindings:bindings(&[("Label",602),("MemberPosition",600)]),..Default::default()}],..Default::default()}],..Default::default()};
}
fn roots(profile:&str)->Project{
 let mut p=base();p.extra_targets.clear();p.graph.nodes.clear();constant(&mut p,900,Value::String("unused.json".into()));
 let alias=profile.contains("Alias");let named=alias||profile.contains("-N");let own=if alias{"Alias"}else if named{"Rows"}else{"Primary"};
 let mut root=row_schema();root.name="Root".into();root.repeating=!profile.starts_with("X07")&&!profile.ends_with("false");
 if named{p.source=SchemaNode::group("Primary",vec![]);p.extra_sources[1].name=own.into();p.extra_sources[1].schema=root;}else{p.source=root;}
 p.graph.nodes.insert(1,field(own,&["Start"],None));p.graph.nodes.insert(2,field(own,&["Label"],None));p.graph.nodes.insert(11,field(own,&["Label"],Some(&[])));p.graph.nodes.insert(5,position(own,&[]));p.graph.nodes.insert(600,Node::Position{collection:if alias{path(&["Alias"])}else{vec![]}});
 let route=if alias{ScopeIteration::Source(path(&["Alias"]))}else{iteration(own,&[])};
 p.root=Scope{iteration:route.clone(),group_starting_with:Some(1),bindings:bindings(&[("First",if profile.starts_with("X09"){11}else{2}),("GroupPosition",5)]),children:vec![Scope{target_field:"Members".into(),iteration:route,bindings:bindings(&[("Label",11),("MemberPosition",5),("LegacyPosition",600)]),..Default::default()}],..Default::default()};
 p.target=SchemaNode::group("Output",vec![string("First"),int("GroupPosition"),member_schema()]).repeating();
 if profile.starts_with("X07"){p.root.bindings.insert(2,Binding{target_field:"RootPin".into(),node:11});fields(&mut p.target).insert(2,string("RootPin"));}
 if profile=="X10-ordinary"{p.root.group_starting_with=None;}
 if profile=="X11"{p.extra_sources[1].schema=row_schema();p.root.children[0].iteration=iteration("Rows",&[]);p.graph.nodes.insert(12,field("Primary",&["Label"],Some(&[])));p.root.children[0].bindings=bindings(&[("Label",12)]);}
 p
}
fn batches(p:&mut Project,per_parent:bool){
 fields(&mut p.source).push(SchemaNode::group("Batches",vec![string("BatchName"),int("Limit"),row_schema()]).repeating());
 for(id,leaf)in[(401,"Start"),(402,"Label")]{p.graph.nodes.insert(id,field("Primary",&["Batches","Rows",leaf],Some(&["Batches","Rows"])));}
 p.graph.nodes.insert(405,position("Primary",&["Batches","Rows"]));p.graph.nodes.insert(406,field("Primary",&["Batches","BatchName"],Some(&["Batches"])));p.graph.nodes.insert(407,field("Primary",&["Batches","Limit"],Some(&["Batches"])));
 let mut scope=Scope{target_field:"Groups".into(),iteration:iteration("Primary",&["Batches","Rows"]),group_starting_with:Some(401),bindings:bindings(&[("First",402),("GroupPosition",405),("BatchName",406)]),children:vec![Scope{target_field:"Members".into(),iteration:iteration("Primary",&["Batches","Rows"]),bindings:bindings(&[("Label",402),("MemberPosition",405),("BatchName",406)]),..Default::default()}],..Default::default()};
 let gs=SchemaNode::group("Groups",vec![string("First"),int("GroupPosition"),string("BatchName"),SchemaNode::group("Members",vec![string("Label"),int("MemberPosition"),string("BatchName")]).repeating()]).repeating();
 if per_parent{scope.windows=vec![W::First{count:407}];p.root.children=vec![Scope{target_field:"Batches".into(),iteration:iteration("Primary",&["Batches"]),bindings:bindings(&[("BatchName",406)]),children:vec![scope],..Default::default()}];p.target=SchemaNode::group("Output",vec![SchemaNode::group("Batches",vec![string("BatchName"),gs]).repeating()]);}
 else{p.root.children=vec![scope];p.target=SchemaNode::group("Output",vec![gs]);}
}
fn item(p:&mut Project,id:u32,sequence:SequenceExpr,index:u32){p.graph.nodes.insert(sequence.item(),Node::SourceField{path:vec![],frame:None});p.graph.nodes.insert(id,Node::SequenceItemAt{sequence,index});}
pub fn project(profile:&str)->Project{
 assert!(PROFILES.contains(&profile),"unknown authored profile {profile}");
 if profile.starts_with("X07")||profile.starts_with("X08")||profile.starts_with("X09")||profile=="X10-ordinary"||profile=="X11"{return roots(profile);}
 let mut p=base();
 match profile{
  "S01"|"S06"=>{},"S02"=>p.root.children[0]=grouped("Rows",200),"S03"=>p.root.children[0]=grouped(EXACT,300),
  "S04"=>p.root.children[0].group_starting_with=Some(902),"S05"=>p.root.children[0].group_starting_with=Some(901),"S07"=>p.root.children[0].filter=Some(33),"S08"=>p.root.children[0].post_group_filter=Some(3),"S09"=>p.root.children[0].post_group_filter=Some(902),
  "S10"|"S11"|"S12"|"S13"|"S14"|"S16"|"S24"=>{let g=&mut p.root.children[0];g.sort_by=Some(4);if["S11","S12","S13"].contains(&profile){g.filter=Some(30);}if profile=="S11"{g.sort_filter_order=SortFilterOrder::FilterThenSort;}if profile=="S13"{g.post_group_filter=Some(32);}if profile=="S14"{g.group_starting_with=Some(31);}if profile=="S16"{g.windows=vec![W::First{count:7}];}},
  "S15"=>{p.root.children[0].sort_by=Some(5);p.root.children[0].sort_descending=true;},"S17"=>p.root.children[0].windows=vec![W::SkipFirst{count:911},W::Last{count:912}],"S18"=>p.root.children[0].windows=vec![W::Last{count:912},W::SkipFirst{count:911}],"S19"=>p.root.children[0].windows=vec![W::FromTo{first:912,last:913}],"S20"=>first(&mut p),"S21"=>{p.root.children[0].windows=vec![W::First{count:910}];p.root.children[0].bindings[0].node=106;},
  "S22"=>{p.graph.nodes.insert(75,Node::ValueMap{input:5,input_type:None,table:vec![(Value::Int(1),Value::Bool(true)),(Value::Int(3),Value::Bool(true)),(Value::Int(5),Value::Bool(true)),(Value::Int(6),Value::Bool(true))],default:Some(Value::Bool(false))});p.graph.nodes.insert(76,Node::If{condition:75,then:1,else_:101});p.graph.nodes.insert(77,Node::If{condition:75,then:901,else_:105});p.root.children[0].group_starting_with=Some(76);p.root.children[0].post_group_filter=Some(77);},
  "S23-boundary"|"S23-post"|"S23-ordinary"=>s23(&mut p,profile),
  "X01"=>nested(&mut p,"Primary",0,"Rows",200,W::First{count:912}),"X02"=>nested(&mut p,"Rows",200,"Primary",0,W::First{count:912}),
  "X03"|"X04"=>{nested(&mut p,"Primary",0,"Primary",0,W::FromTo{first:913,last:913});if profile=="X03"{p.root.children[0].children[0].children[0].group_starting_with=Some(902);}else{let outer=&mut p.root.children[0];outer.children=vec![grouped("Primary",0).children.remove(0),Scope{target_field:"After".into(),bindings:bindings(&[("Position",5)]),..Default::default()}];fields(&mut p.target)[0]=SchemaNode::group("Outer",vec![string("OuterFirst"),int("OuterPosition"),member_schema(),SchemaNode::group("After",vec![int("Position")])]).repeating();}},
  "X05"|"X06"=>batches(&mut p,profile=="X06"),
  "X12-inactive"|"X12-bypass"|"X12-strict"=>{p.source.repeating=profile=="X12-inactive";p.graph.nodes.insert(20,field("Primary",if profile=="X12-strict"{&["Rows","Label"]}else{&["Marker"]},Some(&[])));p.root.children[0].bindings[0].node=20;},
  "X13"=>{fields(&mut fields(&mut p.source)[2]).push(SchemaNode::group("Details",vec![string("Value")]).repeating());p.graph.nodes.insert(20,field("Primary",&["Rows","Details","Value"],Some(&["Rows"])));p.root.children[0].bindings[0].node=20;},
  "X14-scalar"|"X14-members"=>{p.extra_targets.clear();p.source=bool_("Input").repeating();p.graph.nodes.clear();constant(&mut p,900,Value::String("unused.json".into()));p.graph.nodes.insert(1,field("Primary",&[],None));p.graph.nodes.insert(5,position("Primary",&[]));p.graph.nodes.insert(600,Node::Position{collection:vec![]});if profile=="X14-scalar"{p.target=bool_("Output").repeating();p.root=Scope{iteration:iteration("Primary",&[]),group_starting_with:Some(1),construction:mapping::ScopeConstruction::Scalar{value:1},..Default::default()};}else{p.target=SchemaNode::group("Output",vec![SchemaNode::group("Groups",vec![int("GroupPosition"),SchemaNode::group("Members",vec![bool_("Value"),int("MemberPosition"),int("LegacyPosition")]).repeating()]).repeating()]);p.root=Scope{children:vec![Scope{target_field:"Groups".into(),iteration:iteration("Primary",&[]),group_starting_with:Some(1),bindings:bindings(&[("GroupPosition",5)]),children:vec![Scope{target_field:"Members".into(),iteration:iteration("Primary",&[]),bindings:bindings(&[("Value",1),("MemberPosition",5),("LegacyPosition",600)]),..Default::default()}],..Default::default()}],..Default::default()};}},
  "X15-by"|"X15-adjacent"|"X16"|"X17"=>{nested(&mut p,"Primary",0,"Primary",0,W::First{count:917});let outside=&mut p.root.children[0];if profile.starts_with("X15"){outside.group_starting_with=Some(902);let inside=&mut outside.children[0].children[0];inside.group_starting_with=None;if profile=="X15-by"{inside.group_by=Some(9);}else{inside.group_adjacent_by=Some(9);}}else{outside.group_starting_with=None;if profile=="X16"{outside.group_by=Some(9);}else{outside.group_adjacent_by=Some(9);}}},
  "E01"=>p.root.children[0].group_starting_with=Some(101),
  "E02"|"E03"|"E04"=>{p.graph.nodes.insert(40,Node::If{condition:34,then:102,else_:901});let g=&mut p.root.children[0];g.filter=Some(40);g.group_starting_with=Some(if profile=="E03"{1}else{101});g.post_group_filter=Some(105);if profile=="E04"{g.sort_by=Some(103);g.sort_filter_order=SortFilterOrder::FilterThenSort;g.windows=vec![W::First{count:104}];}},
  "E05"|"E06"=>{let g=&mut p.root.children[0];g.sort_by=Some(if profile=="E05"{103}else{4});g.windows=vec![W::First{count:104}];g.filter=Some(102);g.group_starting_with=Some(101);g.post_group_filter=Some(105);},
   "E07-zero"|"E07-first"|"E08"=>{let g=&mut p.root.children[0];if profile!="E07-first"{g.windows=vec![W::First{count:910}];}if profile=="E08"{g.post_group_filter=Some(105);}else{g.group_starting_with=Some(101);}if profile=="E07-first"{first(&mut p);}},
  "E09"=>{p.root.children[0].group_starting_with=Some(902);p.graph.nodes.insert(40,Node::If{condition:34,then:105,else_:901});p.root.children[0].post_group_filter=Some(40);},
  "E10-string"|"E10-null"|"E10-json-null"|"E10-int"|"E10-float"|"E10-xml-nil"=>{let v=match profile{"E10-string"=>Value::String("true".into()),"E10-null"=>Value::Null,"E10-json-null"=>Value::json_null(),"E10-int"=>Value::Int(1),"E10-float"=>Value::Float(f64::from_bits(0x3ff0000000000000)),_=>Value::xml_nil()};constant(&mut p,101,v);p.root.children[0].group_starting_with=Some(101);p.root.children[0].post_group_filter=Some(105);},
  "E11"=>{constant(&mut p,105,Value::String("false".into()));p.root.children[0].post_group_filter=Some(105);},"E12"=>{constant(&mut p,104,Value::Bool(true));p.root.children[0].windows=vec![W::First{count:104}];},"E13"=>p.root.children[0].windows=vec![W::First{count:910},W::First{count:104}],
  "E14"=>{let g=&mut p.root.children[0];g.filter=Some(902);g.group_starting_with=Some(101);g.post_group_filter=Some(105);g.windows=vec![W::First{count:7}];g.bindings[0].node=106;},"E15"=>{p.root.children[0].post_group_filter=Some(902);p.root.children[0].bindings[0].node=106;},"E16"=>{p.root.children[0].post_group_filter=Some(3);p.graph.nodes.insert(40,Node::If{condition:35,then:106,else_:2});p.root.children[0].bindings[0].node=40;},
  "E17"=>{p.extra_targets.clear();p.root.children[0]=grouped("Rows",200);},"E18-window"|"E18-post"|"E18-reached"=>{p.extra_targets.clear();p.root.children[0].bindings[0].node=202;if profile=="E18-window"{p.root.children[0].windows=vec![W::First{count:910}];}if profile=="E18-post"{p.root.children[0].post_group_filter=Some(902);}},
  "E19-position"=>p.root.children[0].bindings[1].node=205,"E19-pin"=>p.root.children[0].bindings[0].node=202,"E19-unpinned"=>{p.graph.nodes.insert(20,field("Rows",&["Rows","Label"],None));p.root.children[0].bindings[0].node=20;},
  "E20-row"|"E20-root"|"E20-ordinary"|"C03-global"|"C03-row"=>{p.graph.nodes.insert(50,Node::SourceField{path:path(&["Rows","Marker"]),frame:None});constant(&mut p,51,Value::String(if profile=="E20-row"{"row.reject"}else{"global.reject"}.into()));p.graph.nodes.insert(52,Node::If{condition:3,then:902,else_:902});p.graph.nodes.insert(54,Node::SourceField{path:path(&["Limit"]),frame:None});call(&mut p,53,"greater_than",&[54,910]);p.failure_rules=vec![mapping::FailureRule{iteration:mapping::FailureIteration::Source{collection:if["E20-row","C03-row"].contains(&profile){path(&["Rows"])}else{vec![]}},selection:if profile=="E20-row"{mapping::FailureSelection::WhenTrue{predicate:3}}else if profile=="E20-root"{mapping::FailureSelection::WhenTrue{predicate:53}}else if profile=="E20-ordinary"{mapping::FailureSelection::All}else{mapping::FailureSelection::WhenTrue{predicate:if profile=="C03-row"{52}else{902}}},message:Some(if profile=="C03-global"{50}else{51})}];if profile.starts_with("E20"){p.root.children[0].group_starting_with=Some(101);}},
  "E23-first"|"E23-last"=>p.root.children[0].windows=vec![W::FromTo{first:if profile=="E23-first"{104}else{911},last:107}],
  "I01"|"I04"|"I05"|"I06"=>{for(id,v)in[(950,Value::String("F|F|T|T|F|T|F".into())),(951,Value::String("|".into())),(952,Value::String("\\|".into())),(955,Value::String("FFTTFTF".into())),(956,Value::String("T".into()))]{constant(&mut p,id,v);}let seq=match profile{"I04"=>SequenceExpr::TokenizeByLength{input:955,length:911,item:970},"I05"=>SequenceExpr::TokenizeRegex{input:950,pattern:952,flags:None,item:970},"I06"=>SequenceExpr::Generate{from:Some(911),to:917,item:970},_=>SequenceExpr::Tokenize{input:950,delimiter:951,item:970}};item(&mut p,70,seq,5);call(&mut p,71,if profile=="I06"{"greater_than"}else{"equal"},&[70,if profile=="I06"{912}else{956}]);p.root.children[0].group_starting_with=Some(71);},
  "I02-group"|"I02-member"=>{constant(&mut p,953,Value::Int(10));constant(&mut p,954,Value::Int(12));item(&mut p,70,SequenceExpr::Generate{from:Some(953),to:954,item:970},5);let g=&mut p.root.children[0];g.bindings.clear();g.children.clear();let vs=SchemaNode::group("Members",vec![mixed("Value")]).repeating();if profile=="I02-group"{g.bindings=bindings(&[("Value",70)]);p.target=SchemaNode::group("Output",vec![SchemaNode::group("Groups",vec![mixed("Value")]).repeating()]);}else{g.children=vec![Scope{target_field:"Members".into(),iteration:iteration("Primary",&["Rows"]),bindings:bindings(&[("Value",70)]),..Default::default()}];p.target=SchemaNode::group("Output",vec![SchemaNode::group("Groups",vec![vs]).repeating()]);}},
  "I03"=>{for s in [&mut p.source,&mut p.extra_sources[1].schema]{fields(s).push(int("Index"));fields(&mut fields(s)[2]).push(int("Index"));}constant(&mut p,950,Value::String("0|2".into()));constant(&mut p,951,Value::String("|".into()));p.graph.nodes.insert(71,field("Rows",&["Index"],Some(&[])));item(&mut p,70,SequenceExpr::Tokenize{input:950,delimiter:951,item:970},71);p.root.children[0].windows=vec![W::First{count:70}];},
  "I07-producer"|"I07-empty"|"I07-null"=>{p.extra_targets.clear();constant(&mut p,1201,Value::String("producer".into()));constant(&mut p,1202,Value::String("index".into()));p.graph.nodes.insert(201,if profile=="I07-producer"{Node::Raise{message:Some(1201)}}else{Node::Const{value:if profile=="I07-null"{Value::Null}else{Value::Int(8)}}});p.graph.nodes.insert(202,Node::Raise{message:Some(1202)});item(&mut p,70,SequenceExpr::Generate{from:Some(201),to:917,item:970},202);p.root.children[0].group_starting_with=Some(70);},
  "I08"=>{for(id,v)in[(950,Value::String("T".into())),(951,Value::String("|".into())),(952,Value::String("1".into()))]{constant(&mut p,id,v);}item(&mut p,72,SequenceExpr::Tokenize{input:952,delimiter:951,item:972},911);item(&mut p,70,SequenceExpr::Tokenize{input:950,delimiter:951,item:970},72);call(&mut p,73,"equal",&[70,950]);p.root.children[0].group_starting_with=Some(902);p.root.children[0].post_group_filter=Some(73);},
  "C02"=>{p.graph.nodes.insert(24,field("Primary",&["Marker"],Some(&[])));p.graph.nodes.insert(25,Node::SourceField{path:path(&["Rows","Marker"]),frame:None});p.graph.nodes.insert(6,Node::If{condition:901,then:24,else_:25});},
  "C05"=>{p.graph.nodes.insert(700,field("Rows",&["Limit"],Some(&[])));call(&mut p,701,"equal",&[700,911]);let g=&mut p.root.children[0];g.windows=vec![W::First{count:700}];g.group_starting_with=Some(701);g.bindings=bindings(&[("First",2),("GroupPosition",5),("BoundEcho",700)]);g.children[0].bindings=bindings(&[("Label",2),("MemberPosition",5),("Echo",700)]);p.target=SchemaNode::group("Output",vec![SchemaNode::group("Groups",vec![string("First"),int("GroupPosition"),int("BoundEcho"),SchemaNode::group("Members",vec![string("Label"),int("MemberPosition"),int("Echo")]).repeating()]).repeating()]);},
   "Q02-islands"=>{
    for(id,leaf)in[(601,"Key"),(602,"Label"),(604,"Start")]{p.graph.nodes.insert(id,Node::SourceField{path:path(&[leaf]),frame:Some(path(&["Rows"]))});}
    p.graph.nodes.insert(603,Node::SourceField{path:path(&["Marker"]),frame:None});
    p.graph.nodes.insert(605,Node::Position{collection:path(&["Rows"])});
    for(mode,name)in["By","Adjacent","Starting","Ending","Blocks"].into_iter().enumerate(){
     let mut g=grouped("Primary",0);g.target_field=name.into();g.iteration=ScopeIteration::Source(path(&["Rows"]));g.group_starting_with=None;
     g.bindings=bindings(&[("First",602),("GroupPosition",605),("RootMarker",603)]);g.children[0].iteration=ScopeIteration::Source(vec![]);g.children[0].bindings=bindings(&[("Label",602),("MemberPosition",605),("LegacyPosition",605)]);
     match mode{0=>g.group_by=Some(601),1=>g.group_adjacent_by=Some(601),2=>g.group_starting_with=Some(604),3=>g.group_ending_with=Some(604),_=>g.group_into_blocks=Some(912)}
     p.root.children.push(g);let mut schema=groups_schema();schema.name=name.into();fields(&mut p.target).push(schema);
    }
   },
   _=>unreachable!("authored profile")
 }
 p
}
