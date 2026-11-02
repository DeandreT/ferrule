//! Explicit finite cohort/owner policies, shared by validation and coverage.

use std::sync::OnceLock;

use serde_json::Value;

use super::schema::Primitive;

pub struct DeclaredCohort {
    pub id: &'static str,
    pub file: &'static str,
    pub records: u32,
    pub parameters: usize,
    pub owners: &'static [(u32, &'static str)],
    pub events: bool,
    oracle: &'static str,
    expected: OnceLock<Vec<Value>>,
}

impl DeclaredCohort {
    pub fn expected(&self) -> &[Value] {
        self.expected.get_or_init(|| {
            self.oracle
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        })
    }

    pub fn catalog_owner(&self, index: u32) -> &'static str {
        self.owners.iter().find(|o| o.0 == index).unwrap().1
    }

    pub fn return_family(&self) -> Primitive {
        if self.events {
            Primitive::Void
        } else {
            Primitive::Hresult
        }
    }

    pub fn has_function_dump(&self, owner: u32, record: u32) -> bool {
        !(self.events && matches!((owner, record), (52, 0 | 1) | (55, 0) | (57, 0 | 1)))
    }
}

macro_rules! cohort {
    ($id:literal, $records:literal, $parameters:literal, $events:literal, $owners:expr) => {
        DeclaredCohort {
            id: $id,
            file: concat!("com-", $id, "-declarations.json"),
            records: $records,
            parameters: $parameters,
            owners: $owners,
            events: $events,
            oracle: include_str!(concat!("../oracles/com-", $id, "-declarations.jsonl")),
            expected: OnceLock::new(),
        }
    };
}

pub static DECLARED: [DeclaredCohort; 5] = [
    cohort!(
        "core",
        114,
        119,
        false,
        &[
            (0, "Application"),
            (1, "Options"),
            (7, "Document"),
            (37, "Documents")
        ]
    ),
    cohort!(
        "graph",
        79,
        110,
        false,
        &[
            (8, "MapForceView"),
            (9, "Mapping"),
            (10, "Components"),
            (11, "Component"),
            (19, "Datapoint"),
            (21, "Connection"),
            (27, "Mappings")
        ]
    ),
    cohort!(
        "projects",
        68,
        79,
        false,
        &[
            (32, "LibraryImports"),
            (33, "LibraryImport"),
            (34, "Project"),
            (35, "ProjectItem")
        ]
    ),
    cohort!(
        "diagnostics",
        28,
        44,
        false,
        &[
            (15, "AppOutputLine"),
            (16, "AppOutputLineSymbol"),
            (25, "ErrorMarker")
        ]
    ),
    cohort!(
        "events",
        8,
        7,
        true,
        &[
            (48, "Application"),
            (52, "Document"),
            (55, "Project"),
            (57, "ProjectItem")
        ]
    ),
];

pub fn by_id(id: &str) -> &'static DeclaredCohort {
    DECLARED.iter().find(|c| c.id == id).unwrap()
}
