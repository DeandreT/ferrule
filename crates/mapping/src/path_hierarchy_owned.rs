//! Path-hierarchy construction over an explicitly owned input collection.
//!
//! Only the input collection carries an owner. The directory, file and name
//! fields describe the generated target structure and never select a source.

use std::fmt;

use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::OwnedSourceRef;

/// Validated construction of a recursive directory tree from the scalar
/// values of one exact endpoint-relative collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OwnedPathHierarchyPlan {
    collection: OwnedSourceRef,
    separator: String,
    directories: String,
    files: String,
    name: String,
}

impl OwnedPathHierarchyPlan {
    /// The collection path is endpoint data and gains no path restriction here;
    /// schema validation decides whether it selects a repeating scalar.
    pub fn new(
        collection: OwnedSourceRef,
        separator: String,
        directories: String,
        files: String,
        name: String,
    ) -> Option<Self> {
        (!separator.is_empty()
            && !directories.is_empty()
            && !files.is_empty()
            && !name.is_empty()
            && directories != files)
            .then_some(Self {
                collection,
                separator,
                directories,
                files,
                name,
            })
    }

    pub fn collection(&self) -> &OwnedSourceRef {
        &self.collection
    }

    pub fn separator(&self) -> &str {
        &self.separator
    }

    pub fn directories(&self) -> &str {
        &self.directories
    }

    pub fn files(&self) -> &str {
        &self.files
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

const FIELDS: &[&str] = &["collection", "separator", "directories", "files", "name"];

struct PlanVisitor;

impl<'de> Visitor<'de> for PlanVisitor {
    type Value = OwnedPathHierarchyPlan;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an owned path-hierarchy plan object")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut collection = None;
        let mut strings: [Option<String>; 4] = Default::default();
        while let Some(field) = map.next_key::<String>()? {
            let slot = match field.as_str() {
                "collection" => {
                    if collection.is_some() {
                        return Err(de::Error::duplicate_field("collection"));
                    }
                    collection = Some(map.next_value::<OwnedSourceRef>()?);
                    continue;
                }
                "separator" => 0,
                "directories" => 1,
                "files" => 2,
                "name" => 3,
                _ => return Err(de::Error::unknown_field(&field, FIELDS)),
            };
            if strings[slot].is_some() {
                return Err(de::Error::duplicate_field(FIELDS[slot + 1]));
            }
            strings[slot] = Some(map.next_value::<String>()?);
        }
        let collection = collection.ok_or_else(|| de::Error::missing_field("collection"))?;
        let [separator, directories, files, name] = strings;
        let separator = separator.ok_or_else(|| de::Error::missing_field("separator"))?;
        let directories = directories.ok_or_else(|| de::Error::missing_field("directories"))?;
        let files = files.ok_or_else(|| de::Error::missing_field("files"))?;
        let name = name.ok_or_else(|| de::Error::missing_field("name"))?;
        OwnedPathHierarchyPlan::new(collection, separator, directories, files, name).ok_or_else(
            || {
                de::Error::custom(
                    "owned path hierarchy requires a non-empty separator and name plus distinct non-empty directory and file fields",
                )
            },
        )
    }
}

impl<'de> Deserialize<'de> for OwnedPathHierarchyPlan {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(PlanVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PathHierarchyPlan, Project, ScopeConstruction, SourceOwner, project_file};

    fn plan(owner: SourceOwner) -> OwnedPathHierarchyPlan {
        OwnedPathHierarchyPlan::new(
            OwnedSourceRef {
                owner,
                path: vec!["Files".into(), "File".into()],
            },
            "/".into(),
            "directory".into(),
            "file".into(),
            "name".into(),
        )
        .expect("valid owned plan")
    }

    #[test]
    fn owned_plan_round_trips_exact_owner_and_path() {
        for owner in [
            SourceOwner::Primary,
            SourceOwner::Named {
                name: " audit ".into(),
            },
        ] {
            let plan = plan(owner);
            let wire = serde_json::to_value(&plan).unwrap();
            assert_eq!(
                serde_json::from_value::<OwnedPathHierarchyPlan>(wire.clone()).unwrap(),
                plan
            );
            assert_eq!(
                wire["collection"]["path"],
                serde_json::json!(["Files", "File"])
            );
            let construction = ScopeConstruction::OwnedPathHierarchy { plan: plan.clone() };
            let wire = serde_json::to_value(&construction).unwrap();
            assert!(wire.get("owned_path_hierarchy").is_some(), "{wire}");
            assert_eq!(
                serde_json::from_value::<ScopeConstruction>(wire).unwrap(),
                construction
            );
        }
        // Empty paths and segments are endpoint data for schema validation.
        let root = OwnedPathHierarchyPlan::new(
            OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: vec![String::new()],
            },
            "\\".into(),
            "d".into(),
            "f".into(),
            "n".into(),
        );
        assert!(root.is_some());
    }

    #[test]
    fn owned_plan_rejects_malformed_owners_fields_and_output_structure() {
        let valid = r#"{"collection":{"owner":{"kind":"primary"},"path":["File"]},"separator":"/","directories":"directory","files":"file","name":"name"}"#;
        assert!(serde_json::from_str::<OwnedPathHierarchyPlan>(valid).is_ok());
        for invalid in [
            // Malformed or unknown owners.
            r#"{"collection":{"owner":{"kind":"named","name":""},"path":["File"]},"separator":"/","directories":"directory","files":"file","name":"name"}"#,
            r#"{"collection":{"owner":{"kind":"named"},"path":["File"]},"separator":"/","directories":"directory","files":"file","name":"name"}"#,
            r#"{"collection":{"owner":{"kind":"remote","name":"x"},"path":["File"]},"separator":"/","directories":"directory","files":"file","name":"name"}"#,
            r#"{"collection":{"owner":{"kind":"primary","name":"x"},"path":["File"]},"separator":"/","directories":"directory","files":"file","name":"name"}"#,
            // A legacy path is not an owner.
            r#"{"collection":["File"],"separator":"/","directories":"directory","files":"file","name":"name"}"#,
            r#"{"collection":{"owner":{"kind":"primary"}},"separator":"/","directories":"directory","files":"file","name":"name"}"#,
            r#"{"collection":{"owner":{"kind":"primary"},"path":["File"],"frame":[]},"separator":"/","directories":"directory","files":"file","name":"name"}"#,
            // Output structure is not a source selector and keeps its rules.
            r#"{"collection":{"owner":{"kind":"primary"},"path":["File"]},"separator":"","directories":"directory","files":"file","name":"name"}"#,
            r#"{"collection":{"owner":{"kind":"primary"},"path":["File"]},"separator":"/","directories":"item","files":"item","name":"name"}"#,
            r#"{"collection":{"owner":{"kind":"primary"},"path":["File"]},"separator":"/","directories":"directory","files":"file","name":""}"#,
            r#"{"collection":{"owner":{"kind":"primary"},"path":["File"]},"separator":"/","directories":"directory","files":"file"}"#,
            r#"{"collection":{"owner":{"kind":"primary"},"path":["File"]},"separator":"/","directories":"directory","files":"file","name":"name","owner":{"kind":"primary"}}"#,
            r#"{"collection":{"owner":{"kind":"primary"},"path":["File"]},"collection":{"owner":{"kind":"primary"},"path":["File"]},"separator":"/","directories":"directory","files":"file","name":"name"}"#,
        ] {
            assert!(
                serde_json::from_str::<OwnedPathHierarchyPlan>(invalid).is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn legacy_plan_wire_is_unchanged_and_old_readers_refuse_owned_plans() {
        let legacy = PathHierarchyPlan::new(
            vec!["File".into()],
            "/".into(),
            "directory".into(),
            "file".into(),
            "name".into(),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_string(&ScopeConstruction::PathHierarchy { plan: legacy }).unwrap(),
            r#"{"path_hierarchy":{"plan":{"collection":["File"],"separator":"/","directories":"directory","files":"file","name":"name"}}}"#
        );
        // An owned plan cannot be read as the legacy plan or legacy variant.
        let owned = serde_json::to_value(plan(SourceOwner::Primary)).unwrap();
        assert!(serde_json::from_value::<PathHierarchyPlan>(owned.clone()).is_err());
        let wire = serde_json::json!({ "path_hierarchy": { "plan": owned } });
        assert!(serde_json::from_value::<ScopeConstruction>(wire).is_err());

        // A reader that only knows the earlier construction variants rejects
        // the new variant instead of dropping or reinterpreting it.
        #[derive(Debug, serde::Deserialize)]
        #[serde(rename_all = "snake_case")]
        #[allow(dead_code)]
        enum OldConstruction {
            Constructed,
            CopyCurrentSource,
            PathHierarchy { plan: PathHierarchyPlan },
        }
        let construction = ScopeConstruction::OwnedPathHierarchy {
            plan: plan(SourceOwner::Primary),
        };
        let wire = serde_json::to_value(&construction).unwrap();
        assert!(serde_json::from_value::<OldConstruction>(wire).is_err());

        let project = Project {
            source: ir::SchemaNode::group("Input", vec![]),
            target: ir::SchemaNode::group("Output", vec![]),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![],
            extra_targets: vec![],
            failure_rules: vec![],
            user_functions: Default::default(),
            graph: Default::default(),
            root: crate::Scope {
                construction: construction.clone(),
                ..crate::Scope::default()
            },
        };
        let text = project_file::encode_pretty(&project).unwrap();
        assert!(text.contains("owned_path_hierarchy"));
        assert_eq!(
            project_file::decode_str(&text).unwrap().root.construction,
            construction
        );
        assert_eq!(
            crate::first_project_owned_source_scope(&project),
            Some(crate::SourceScopeLocation::root(
                crate::SourceScopeTarget::Primary
            ))
        );
    }
}
