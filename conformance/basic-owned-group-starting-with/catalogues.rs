//! Full independent entries, including untouched original declaration ordinals.
use super::*;
fn literal(required:bool,alias:bool)->Json{
 let entries=if alias{json!([
  {"declaration_index":0,"name":"unusedStatic","availability":"Optional"},
  {"declaration_index":1,"name":"Alias","availability":"Optional"},
  {"declaration_index":2,"name":" Røws💠 ","availability":"Optional"},
  {"declaration_index":3,"name":"unavailableDynamic","availability":"ReservedUnavailable"}
 ])}else{json!([
  {"declaration_index":0,"name":"unusedStatic","availability":"Optional"},
  {"declaration_index":1,"name":"Rows","availability":"Optional"},
  {"declaration_index":2,"name":" Røws💠 ","availability":"Optional"},
  {"declaration_index":3,"name":"unavailableDynamic","availability":"ReservedUnavailable"}
 ])};
 let mut entries=entries;if required{entries[1]["availability"]=json!("RequiredLegacy");}entries
}
pub fn expected(profile:&str)->Json{
 let alias=profile.contains("Alias");let global=profile=="C03-global";
 let primary=global||alias||["S23-boundary","S23-post","S23-ordinary","C02","Q02-islands"].contains(&profile);
 let named=if cases::standalone(profile){json!([])}else{json!([
  {"target_index":0,"name":"audit","catalogue":literal(global,alias)},
  {"target_index":1,"name":"unicode","catalogue":literal(global,alias)}
 ])};
 json!({"all_outputs":literal(primary,alias),"primary":literal(primary,alias),"named_targets":named})
}
