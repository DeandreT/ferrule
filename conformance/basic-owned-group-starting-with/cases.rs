//! Independent complete literals. Helpers only expand explicitly listed fields
//! and members; none partition, sort, filter, window or execute input data.
use super::*;
pub fn scalar(value:Value)->Instance{Instance::Scalar(value)}
pub fn text(s:&str)->Instance{scalar(Value::String(s.into()))}
pub fn integer(n:i64)->Instance{scalar(Value::Int(n))}
pub fn boolean(b:bool)->Instance{scalar(Value::Bool(b))}
pub fn group(fields:Vec<(&str,Instance)>)->Instance{Instance::Group(fields.into_iter().map(|(name,value)|(name.into(),value)).collect::<Vec<_>>().into())}
pub fn repeated(items:Vec<Instance>)->Instance{Instance::Repeated(items)}
pub fn replace(value:&mut Instance,name:&str,new:Instance){let Instance::Group(fields)=value else{panic!("literal Group")};fields.iter_mut().find(|(key,_)|key==name).unwrap().1=new;}
pub fn append(value:&mut Instance,name:&str,new:Instance){let Instance::Group(fields)=value else{panic!("literal Group")};fields.push((name.into(),new));}
pub fn row(label:&str,start:bool,keep:bool,rank:i64,key:&str)->Instance{group(vec![("Start",boolean(start)),("Label",text(label)),("Keep",boolean(keep)),("Rank",integer(rank)),("Limit",integer(0)),("Key",text(key))])}
fn fresh(label:&str,start:bool)->Instance{row(label,start,true,0,"A")}
pub fn endpoint(marker:&str,limit:i64,rows:Vec<Instance>)->Instance{group(vec![("Marker",text(marker)),("Limit",integer(limit)),("Rows",repeated(rows))])}
pub fn source()->Instance{endpoint("P.root",2,vec![row("p0",false,false,30,"A"),row("p1",false,true,10,"B"),row("p2",true,false,20,"A"),row("p3",true,false,25,"A"),row("p4",false,true,40,"C"),row("p5",true,false,50,"A"),row("p6",false,true,60,"A")])}
pub fn inputs()->Vec<(String,Instance)>{vec![("Rows".into(),endpoint("N.root",1,vec![row("n0",true,false,300,"A"),row("n1",false,true,100,"B"),row("n2",false,false,200,"A"),row("n3",true,false,250,"A"),row("n4",false,true,400,"C"),row("n5",false,false,500,"A"),row("n6",true,true,600,"A")])),(projects::EXACT.into(),endpoint("U.root",2,vec![row("u0",false,true,7,"A")]))]}
pub fn m(pos:i64,label:&str)->Instance{group(vec![("Label",text(label)),("MemberPosition",integer(pos)),("LegacyPosition",integer(pos))])}
fn members(labels:&[&str])->Instance{repeated(labels.iter().enumerate().map(|(i,label)|m(i as i64+1,label)).collect())}
pub fn g(pos:i64,first:&str,marker:&str,labels:&[&str])->Instance{group(vec![("First",text(first)),("GroupPosition",integer(pos)),("RootMarker",text(marker)),("Members",members(labels))])}
fn root_g(pos:i64,first:&str,labels:&[&str])->Instance{group(vec![("First",text(first)),("GroupPosition",integer(pos)),("Members",members(labels))])}
fn item_g(pos:i64,label:&str)->Instance{group(vec![("First",text(label)),("GroupPosition",integer(pos)),("Members",repeated(vec![m(pos,label)]))])}
fn h(pos:i64,first:&str,labels:&[&str])->Instance{group(vec![("First",text(first)),("GroupPosition",integer(pos)),("Members",repeated(labels.iter().enumerate().map(|(i,label)|group(vec![("Label",text(label)),("MemberPosition",integer(i as i64+1))])).collect()))])}
pub fn o(gs:Vec<Instance>)->Instance{group(vec![("Groups",repeated(gs))])}
pub fn p4()->Vec<Instance>{vec![g(1,"p0","P.root",&["p0","p1"]),g(2,"p2","P.root",&["p2"]),g(3,"p3","P.root",&["p3","p4"]),g(4,"p5","P.root",&["p5","p6"])]}
pub fn n3()->Vec<Instance>{vec![g(1,"n0","N.root",&["n0","n1","n2"]),g(2,"n3","N.root",&["n3","n4","n5"]),g(3,"n6","N.root",&["n6"])]}
pub fn u1()->Vec<Instance>{vec![g(1,"u0","U.root",&["u0"])]}
fn kept()->Vec<Instance>{vec![g(1,"p0","P.root",&["p0","p1"]),g(2,"p3","P.root",&["p3","p4"]),g(3,"p5","P.root",&["p5","p6"])]}
fn whole()->Vec<Instance>{vec![g(1,"p0","P.root",&["p0","p1","p2","p3","p4","p5","p6"])]}
pub fn ok(outputs:Vec<(Option<&str>,Instance)>)->Json{json!({"Ok":outputs.into_iter().map(|(name,tree)|json!([name,codec::encode(&tree)])).collect::<Vec<_>>()})}
pub fn expected(tree:Instance)->Json{ok(vec![(None,tree)])}
pub fn raised(node:u32,message:&str)->Json{json!({"Err":{"kind":"MappingException","node":node,"message":message}})}
pub fn owned(node:Option<u32>,cause:&str,owner:&str,path:&[&str],native_path:&[&str],segment:Option<usize>,collection:Option<&[&str]>,scope:Json)->Json{json!({"Err":{"kind":"OwnedSource","node":node,"cause":cause,"owner":owner,"path":path,"native_path":native_path,"segment":segment,"found":null,"collection":collection,"native_scope":scope}})}
pub fn standalone(profile:&str)->bool{profile.starts_with("S23")||profile.starts_with("X07")||profile.starts_with("X08")||profile.starts_with("X09")||profile=="X10-ordinary"||profile=="X11"||profile.starts_with("X14")||profile=="E17"||profile.starts_with("E18")||profile.starts_with("I07")}
fn normal(id:&'static str,profile:&'static str,outcome:Json)->Case{
 let named=!standalone(profile);
 let all=if let Some(outputs)=outcome.get("Ok"){let mut outputs=outputs.as_array().unwrap().clone();if named{outputs.push(json!(["audit",codec::encode(&o(n3()))]));outputs.push(json!(["unicode",codec::encode(&o(u1()))]));}json!({"Ok":outputs})}else{outcome.clone()};
 let mut calls=vec![json!({"selection":"All","expected":all}),json!({"selection":"Primary","expected":outcome})];
 if named{calls.push(json!({"selection":"audit","expected":ok(vec![(Some("audit"),o(n3()))])}));calls.push(json!({"selection":"unicode","expected":ok(vec![(Some("unicode"),o(u1()))])}));}
 Case{id,profile,source:source(),inputs:inputs(),calls,native:true}
}
fn success(profile:&'static str,tree:Instance)->Case{normal(profile,profile,expected(tree))}
fn empty(profile:&'static str)->Case{success(profile,o(vec![]))}
fn root_source()->Instance{repeated(vec![fresh("a",false),fresh("b",true),fresh("c",true),fresh("d",false)])}
fn root_output()->Instance{repeated(vec![root_g(1,"a",&["a"]),root_g(2,"b",&["b"]),root_g(3,"c",&["c","d"])])}
fn outer(first:&str,pos:i64,inner:Instance)->Instance{group(vec![("OuterFirst",text(first)),("OuterPosition",integer(pos)),("Inner",inner)])}
fn outer_output(items:Vec<Instance>)->Instance{group(vec![("Outer",repeated(items))])}
fn bg(pos:i64,first:&str,batch:&str,labels:&[&str])->Instance{group(vec![("First",text(first)),("GroupPosition",integer(pos)),("BatchName",text(batch)),("Members",repeated(labels.iter().enumerate().map(|(i,label)|group(vec![("Label",text(label)),("MemberPosition",integer(i as i64+1)),("BatchName",text(batch))])).collect()))])}
fn batches(per_parent:bool)->Instance{
 let x=if per_parent{vec![fresh("x1",false),fresh("x2",true),fresh("x3",false)]}else{vec![fresh("x1",false),fresh("x2",true)]};
 let y=if per_parent{vec![fresh("y1",false),fresh("y2",true),fresh("y3",true)]}else{vec![fresh("y1",false),fresh("y2",true)]};
 group(vec![("Marker",text("P.root")),("Limit",integer(0)),("Rows",repeated(vec![])),("Batches",repeated(vec![group(vec![("BatchName",text("X")),("Limit",integer(1)),("Rows",repeated(x))]),group(vec![("BatchName",text("Y")),("Limit",integer(2)),("Rows",repeated(y))])]))])
}
pub fn cases()->Vec<Case>{
 let mut cases=vec![success("S01",o(p4())),success("S02",o(n3())),success("S03",o(u1())),success("S04",o(whole())),
  success("S05",o(vec![g(1,"p0","P.root",&["p0"]),g(2,"p1","P.root",&["p1"]),g(3,"p2","P.root",&["p2"]),g(4,"p3","P.root",&["p3"]),g(5,"p4","P.root",&["p4"]),g(6,"p5","P.root",&["p5"]),g(7,"p6","P.root",&["p6"])])),
  success("S06",o(vec![g(1,"p2","P.root",&["p2"]),g(2,"p3","P.root",&["p3","p4"])])),success("S07",o(vec![g(1,"p0","P.root",&["p0","p1","p4","p6"])])),success("S08",o(kept())),empty("S09"),
  success("S10",o(vec![g(1,"p1","P.root",&["p1"]),g(2,"p2","P.root",&["p2"]),g(3,"p3","P.root",&["p3","p0","p4"]),g(4,"p5","P.root",&["p5","p6"])])),
  success("S11",o(vec![g(1,"p2","P.root",&["p2"]),g(2,"p3","P.root",&["p3","p4"]),g(3,"p5","P.root",&["p5","p6"])])),success("S12",o(vec![g(1,"p3","P.root",&["p3","p0","p4"]),g(2,"p5","P.root",&["p5","p6"])])),success("S13",o(vec![g(1,"p3","P.root",&["p3","p0","p4"])])),
  success("S14",o(vec![g(1,"p1","P.root",&["p1"]),g(2,"p2","P.root",&["p2"]),g(3,"p3","P.root",&["p3","p0","p4","p5","p6"])])),
  success("S15",o(vec![g(1,"p6","P.root",&["p6"]),g(2,"p5","P.root",&["p5","p4"]),g(3,"p3","P.root",&["p3"]),g(4,"p2","P.root",&["p2","p1","p0"])])),success("S16",o(vec![g(1,"p1","P.root",&["p1"]),g(2,"p2","P.root",&["p2"])])),
  success("S17",o(vec![g(1,"p3","P.root",&["p3","p4"]),g(2,"p5","P.root",&["p5","p6"])])),success("S18",o(vec![g(1,"p5","P.root",&["p5","p6"])])),success("S19",o(vec![g(1,"p2","P.root",&["p2"]),g(2,"p3","P.root",&["p3","p4"])])),
  success("S20",group(vec![("Groups",g(1,"p0","P.root",&["p0","p1"]))])),empty("S21"),success("S22",o(vec![g(1,"a","P.root",&["a","b"]),g(2,"c","P.root",&["c","d"])])),success("S24",o(p4())),
  success("X01",outer_output(vec![outer("p0",1,o(n3())),outer("p2",2,o(n3()))])),success("X02",outer_output(vec![outer("n0",1,o(p4())),outer("n3",2,o(p4()))])),success("X03",outer_output(vec![outer("p3",1,o(vec![g(1,"p3","P.root",&["p3","p4"])]))])),
  success("X04",outer_output(vec![group(vec![("OuterFirst",text("p3")),("OuterPosition",integer(1)),("Members",members(&["p3","p4"])),("After",group(vec![("Position",integer(1))]))])])),
 ];
 replace(&mut cases.iter_mut().find(|c|c.profile=="S06").unwrap().source,"Rows",repeated(vec![row("p2",true,false,20,"A"),row("p3",true,false,25,"A"),row("p4",false,true,40,"C")]));
 let mut no_rows=normal("S20-empty","S20",expected(group(vec![("Groups",group(vec![]))])));replace(&mut no_rows.source,"Rows",repeated(vec![]));cases.push(no_rows);
 replace(&mut cases.iter_mut().find(|c|c.profile=="S22").unwrap().source,"Rows",repeated(vec![fresh("a",false),scalar(Value::Null),fresh("b",false),scalar(Value::json_null()),fresh("c",true),fresh("d",false)]));
 replace(&mut cases.iter_mut().find(|c|c.profile=="S24").unwrap().source,"Rows",repeated(vec![row("p0",false,false,0,"A"),row("p1",false,true,0,"B"),row("p2",true,false,0,"A"),row("p3",true,false,0,"A"),row("p4",false,true,0,"C"),row("p5",true,false,0,"A"),row("p6",false,true,0,"A")]));
 for id in ["S23-boundary","S23-post","S23-ordinary"]{cases.push(success(id,o(vec![h(1,"p0",&["p0","p1"]),h(2,"p2",&["p2"]),h(3,"p3",&["p3","p4"]),h(4,"p5",&["p5","p6"])])));}
 let mut flat=success("X05",o(vec![bg(1,"x1","X",&["x1"]),bg(2,"x2","X",&["x2","y1"]),bg(3,"y2","Y",&["y2"])]));flat.source=batches(false);cases.push(flat);
 let mut per=success("X06",group(vec![("Batches",repeated(vec![group(vec![("BatchName",text("X")),("Groups",repeated(vec![bg(1,"x1","X",&["x1"])]))]),group(vec![("BatchName",text("Y")),("Groups",repeated(vec![bg(1,"y1","Y",&["y1"]),bg(2,"y2","Y",&["y2"])]))])]))]));per.source=batches(true);cases.push(per);
 for id in ["X07-P","X07-N","X08-P","X08-N","X08-Alias","X09-P-true","X09-P-false","X09-N-true","X09-N-false","X09-Alias-true","X09-Alias-false","X10-ordinary","X11"]{
  let alias=id.contains("Alias");let named=alias||id.contains("-N");let own=if alias{"Alias"}else if named{"Rows"}else{"Primary"};
  let outcome=if id.starts_with("X09"){owned(Some(11),"UniteratedRepetition",own,&["Label"],&["Label"],Some(0),None,Json::Null)}else if id=="X11"{owned(Some(12),"UniteratedRepetition","Primary",&["Label"],&["Label"],Some(0),None,Json::Null)}else{expected(if id.starts_with("X07"){repeated(vec![group(vec![("First",text("single")),("GroupPosition",integer(1)),("RootPin",text("single")),("Members",members(&["single"]))])])}else if id=="X10-ordinary"{repeated(vec![item_g(1,"a"),item_g(2,"b"),item_g(3,"c"),item_g(4,"d")])}else{root_output()})};
  let mut case=normal(id,id,outcome);let input=if id.starts_with("X07"){fresh("single",false)}else{root_source()};if named{case.source=group(vec![]);case.inputs[0]=(own.into(),input);}else{case.source=input;}if id=="X11"{case.inputs[0].1=root_source();}cases.push(case);
 }
 let mut shadow=normal("X10-grouped-shadow","X08-P",expected(root_output()));shadow.source=root_source();cases.push(shadow);
 cases.push(normal("X12-inactive","X12-inactive",owned(Some(20),"InactiveSourceFrame","Primary",&[],&[],None,Some(&[]),Json::Null)));
 cases.push(success("X12-bypass",o(vec![g(1,"P.root","P.root",&["p0","p1"]),g(2,"P.root","P.root",&["p2"]),g(3,"P.root","P.root",&["p3","p4"]),g(4,"P.root","P.root",&["p5","p6"])])));
 cases.push(normal("X12-strict","X12-strict",owned(Some(20),"UniteratedRepetition","Primary",&["Rows","Label"],&["Rows","Label"],Some(1),None,Json::Null)));
 let mut details=normal("X13","X13",owned(Some(20),"UniteratedRepetition","Primary",&["Rows","Details","Value"],&["Details","Value"],Some(2),None,Json::Null));let Instance::Group(fs)=&mut details.source else{unreachable!()};let Instance::Repeated(rows)=&mut fs[2].1 else{unreachable!()};append(&mut rows[0],"Details",repeated(vec![group(vec![("Value",text("first"))]),group(vec![("Value",text("second"))])]));cases.push(details);
 let mut reps=success("X14-scalar",repeated(vec![boolean(false),boolean(true),boolean(true)]));reps.source=repeated(vec![boolean(false),boolean(true),boolean(true),boolean(false)]);cases.push(reps);
 fn bm(value:bool,pos:i64)->Instance{group(vec![("Value",boolean(value)),("MemberPosition",integer(pos)),("LegacyPosition",integer(pos))])}
 let mut payloads=success("X14-members",o(vec![group(vec![("GroupPosition",integer(1)),("Members",repeated(vec![bm(false,1)]))]),group(vec![("GroupPosition",integer(2)),("Members",repeated(vec![bm(true,1)]))]),group(vec![("GroupPosition",integer(3)),("Members",repeated(vec![bm(true,1),bm(false,2)]))])]));payloads.source=repeated(vec![boolean(false),boolean(true),boolean(true),boolean(false)]);cases.push(payloads);
 cases.push(success("X15-by",outer_output(vec![outer("p0",1,o(vec![g(1,"p0","P.root",&["p0","p2","p3","p5","p6"]),g(2,"p1","P.root",&["p1"]),g(3,"p4","P.root",&["p4"])]))])));
 cases.push(success("X15-adjacent",outer_output(vec![outer("p0",1,o(vec![g(1,"p0","P.root",&["p0"]),g(2,"p1","P.root",&["p1"]),g(3,"p2","P.root",&["p2","p3"]),g(4,"p4","P.root",&["p4"]),g(5,"p5","P.root",&["p5","p6"])]))])));
 cases.push(success("X16",outer_output(vec![outer("p0",1,o(vec![g(1,"p0","P.root",&["p0"]),g(2,"p2","P.root",&["p2"]),g(3,"p3","P.root",&["p3"]),g(4,"p5","P.root",&["p5","p6"])])),outer("p1",2,o(vec![g(1,"p1","P.root",&["p1"])])),outer("p4",3,o(vec![g(1,"p4","P.root",&["p4"])]))])));
 cases.push(success("X17",outer_output(vec![outer("p0",1,o(vec![g(1,"p0","P.root",&["p0"])])),outer("p1",2,o(vec![g(1,"p1","P.root",&["p1"])])),outer("p2",3,o(vec![g(1,"p2","P.root",&["p2"]),g(2,"p3","P.root",&["p3"])])),outer("p4",4,o(vec![g(1,"p4","P.root",&["p4"])])),outer("p5",5,o(vec![g(1,"p5","P.root",&["p5","p6"])]))])));
 errors(&mut cases);consumers(&mut cases);catalogues(&mut cases);islands(&mut cases);cases
}
fn errors(cases:&mut Vec<Case>){
 for(id,node,msg)in[("E01",101,"boundary"),("E02",101,"boundary"),("E03",105,"post"),("E04",102,"filter"),("E05",103,"sort"),("E06",104,"bound"),("E07-zero",101,"boundary"),("E07-first",101,"boundary"),("E08",105,"post"),("E09",105,"post"),("E13",104,"bound"),("E23-first",104,"bound"),("E23-last",107,"last-bound")]{cases.push(normal(id,id,raised(node,msg)));}
 for(id,found)in[("E10-string","string"),("E10-null","null"),("E10-json-null","json null"),("E10-int","int"),("E10-float","float"),("E10-xml-nil","xml nil")]{cases.push(normal(id,id,json!({"Err":{"kind":"NotABool","node":101,"found":found}})));}
 cases.push(normal("E11","E11",json!({"Err":{"kind":"NotABool","node":105,"found":"string"}})));
 let mut bad_count=normal("E12","E12",json!({"Err":{"kind":"NotAnItemCount","node":104,"found":"bool"}}));replace(&mut bad_count.source,"Rows",repeated(vec![]));cases.push(bad_count);
 cases.extend([empty("E14"),empty("E15"),success("E16",o(kept()))]);
 let mut absent=normal("E17","E17",owned(None,"SourceUnavailable","Rows",&[],&[],None,None,json!({"target":"Primary","children":[0]})));absent.inputs.remove(0);cases.push(absent);
 for id in ["E18-window","E18-post","E18-reached"]{let mut c=if id=="E18-reached"{normal(id,id,owned(Some(202),"SourceUnavailable","Rows",&[],&[],None,None,Json::Null))}else{empty(id)};c.inputs.remove(0);cases.push(c);}
 for(id,node)in[("E19-position",205),("E19-pin",202)]{cases.push(normal(id,id,owned(Some(node),"InactiveSourceFrame","Rows",&[],&[],None,Some(&["Rows"]),Json::Null)));}
 cases.push(normal("E19-unpinned","E19-unpinned",owned(Some(20),"UniteratedRepetition","Rows",&["Rows","Label"],&["Rows","Label"],Some(1),None,Json::Null)));
 for(id,message)in[("E20-row","row.reject"),("E20-root","global.reject"),("E20-ordinary","global.reject")]{let mut c=normal(id,id,json!({"Err":{"kind":"MappingFailure","rule":1,"message":message}}));let expectation=c.calls[0]["expected"].clone();for call in &mut c.calls{call["expected"]=expectation.clone();}cases.push(c);}
 let mut bad_target=normal("E22-target-before-input-and-policy","S01",Json::Null);bad_target.inputs.clear();bad_target.calls=vec![json!({"selection":" audit ","invalid_limits":true,"expected":{"Err":{"kind":"UnknownTarget","name":" audit "}}})];cases.push(bad_target);
}
fn consumers(cases:&mut Vec<Case>){
 for id in["I01","I04","I05"]{cases.push(success(id,o(p4())));}
 let v=|x:Instance|group(vec![("Value",x)]);
 cases.push(success("I02-group",o(vec![v(integer(10)),v(integer(11)),v(integer(12)),v(scalar(Value::Null))])));
 cases.push(success("I02-member",o(vec![group(vec![("Members",repeated(vec![v(integer(10)),v(integer(11))]))]),group(vec![("Members",repeated(vec![v(integer(10))]))]),group(vec![("Members",repeated(vec![v(integer(10)),v(integer(11))]))]),group(vec![("Members",repeated(vec![v(integer(10)),v(integer(11))]))])])));
 let mut parent=success("I03",o(vec![g(1,"p0","P.root",&["p0","p1"]),g(2,"p2","P.root",&["p2"])]));append(&mut parent.source,"Index",integer(1));append(&mut parent.inputs[0].1,"Index",integer(2));for root in [&mut parent.source,&mut parent.inputs[0].1]{let Instance::Group(fs)=root else{unreachable!()};let Instance::Repeated(rows)=&mut fs[2].1 else{unreachable!()};for r in rows{append(r,"Index",integer(1));}}cases.push(parent);
 cases.push(success("I06",o(vec![g(1,"p0","P.root",&["p0","p1"]),g(2,"p2","P.root",&["p2"]),g(3,"p3","P.root",&["p3"]),g(4,"p4","P.root",&["p4"]),g(5,"p5","P.root",&["p5"]),g(6,"p6","P.root",&["p6"])])));
 for id in["I07-producer","I07-empty","I07-null"]{cases.push(normal(id,id,raised(if id=="I07-producer"{201}else{202},if id=="I07-producer"{"producer"}else{"index"})));}
 cases.push(success("I08",o(whole())));
}
fn catalogues(cases:&mut Vec<Case>){
 let mut c=normal("C01-absent-optional","S01",expected(o(p4())));c.inputs.clear();
 let n=owned(None,"SourceUnavailable","Rows",&[],&[],None,None,json!({"target":{"index":0,"named":"audit"},"children":[0]}));let u=owned(None,"SourceUnavailable",projects::EXACT,&[],&[],None,None,json!({"target":{"index":1,"named":"unicode"},"children":[0]}));
 c.calls=vec![json!({"selection":"All","expected":n}),json!({"selection":"Primary","expected":expected(o(p4()))}),json!({"selection":"audit","expected":n}),json!({"selection":"unicode","expected":u})];cases.push(c);
 cases.extend([success("C02",o(p4())),success("C03-global",o(p4())),success("C03-row",o(p4()))]);
  let member=group(vec![("Label",text("p0")),("MemberPosition",integer(1)),("Echo",integer(1))]);
  let run=group(vec![("First",text("p0")),("GroupPosition",integer(1)),("BoundEcho",integer(1)),("Members",repeated(vec![member]))]);
  cases.push(success("C05",o(vec![run])));
}
fn islands(cases:&mut Vec<Case>){
 let mut result=o(p4());
 append(&mut result,"By",repeated(vec![g(1,"p0","P.root",&["p0","p2","p3","p5","p6"]),g(2,"p1","P.root",&["p1"]),g(3,"p4","P.root",&["p4"])]));
 append(&mut result,"Adjacent",repeated(vec![g(1,"p0","P.root",&["p0"]),g(2,"p1","P.root",&["p1"]),g(3,"p2","P.root",&["p2","p3"]),g(4,"p4","P.root",&["p4"]),g(5,"p5","P.root",&["p5","p6"])]));
 append(&mut result,"Starting",repeated(vec![g(1,"p0","P.root",&["p0","p1"]),g(2,"p2","P.root",&["p2"]),g(3,"p3","P.root",&["p3","p4"]),g(4,"p5","P.root",&["p5","p6"])]));
 append(&mut result,"Ending",repeated(vec![g(1,"p0","P.root",&["p0","p1","p2"]),g(2,"p3","P.root",&["p3"]),g(3,"p4","P.root",&["p4","p5"]),g(4,"p6","P.root",&["p6"])]));
 append(&mut result,"Blocks",repeated(vec![g(1,"p0","P.root",&["p0","p1"]),g(2,"p2","P.root",&["p2","p3"]),g(3,"p4","P.root",&["p4","p5"]),g(4,"p6","P.root",&["p6"])]));
 cases.push(success("Q02-islands",result));
}
