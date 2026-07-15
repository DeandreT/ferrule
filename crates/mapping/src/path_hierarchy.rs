use serde::{Deserialize, Deserializer, Serialize};

/// Validated construction of a recursive directory tree from scalar paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PathHierarchyPlan {
    collection: Vec<String>,
    separator: String,
    directories: String,
    files: String,
    name: String,
}

impl PathHierarchyPlan {
    pub fn new(
        collection: Vec<String>,
        separator: String,
        directories: String,
        files: String,
        name: String,
    ) -> Option<Self> {
        let valid_collection =
            !collection.is_empty() && collection.iter().all(|segment| !segment.is_empty());
        (valid_collection
            && !separator.is_empty()
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

    pub fn collection(&self) -> &[String] {
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

impl<'de> Deserialize<'de> for PathHierarchyPlan {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            collection: Vec<String>,
            separator: String,
            directories: String,
            files: String,
            name: String,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(
            wire.collection,
            wire.separator,
            wire.directories,
            wire.files,
            wire.name,
        )
        .ok_or_else(|| {
            serde::de::Error::custom(
                "path hierarchy requires a non-empty collection and separator plus distinct directory and file fields",
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructor_rejects_ambiguous_or_non_terminating_plans() {
        let plan = PathHierarchyPlan::new(
            vec!["File".into()],
            "\\".into(),
            "directory".into(),
            "file".into(),
            "name".into(),
        )
        .unwrap();
        assert_eq!(
            serde_json::from_str::<PathHierarchyPlan>(&serde_json::to_string(&plan).unwrap())
                .unwrap(),
            plan
        );
        assert!(
            PathHierarchyPlan::new(
                vec!["File".into()],
                String::new(),
                "directory".into(),
                "file".into(),
                "name".into(),
            )
            .is_none()
        );
        assert!(
            PathHierarchyPlan::new(
                vec!["File".into()],
                "/".into(),
                "item".into(),
                "item".into(),
                "name".into(),
            )
            .is_none()
        );
        assert!(
            serde_json::from_str::<PathHierarchyPlan>(
                r#"{"collection":["File"],"separator":"","directories":"directory","files":"file","name":"name"}"#,
            )
            .is_err()
        );
    }
}
