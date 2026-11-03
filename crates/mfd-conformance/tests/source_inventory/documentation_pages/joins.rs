//! Resolve compared registries independently, never by trusting candidate joins.

use std::collections::BTreeMap;

use serde_json::Value;

use super::types::{Join, JoinKind, Registry};
use super::{BASE, Check, require};

pub(super) const REGISTRIES: [(Registry, &str); 6] = [
    (
        Registry::Contracts,
        "conformance/mfd-2026r2.source-contracts.json",
    ),
    (Registry::Ledger, "conformance/mfd-2026r2.json"),
    (
        Registry::Nonleaf,
        "conformance/mfd-2026r2-source-inventory/nonleaf.json",
    ),
    (
        Registry::Functions,
        "conformance/mfd-2026r2-source-inventory/functions.json",
    ),
    (
        Registry::Constructors,
        "conformance/mfd-2026r2-source-inventory/constructors.json",
    ),
    (
        Registry::Supporting,
        "conformance/mfd-2026r2-source-inventory/supporting.json",
    ),
];

pub(super) struct Compared {
    pub documents: BTreeMap<Registry, Value>,
    pub by_file: BTreeMap<String, Vec<Join>>,
}

impl Compared {
    pub fn load() -> Check<Self> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let documents = REGISTRIES
            .iter()
            .map(|(key, path)| {
                let bytes = std::fs::read(root.join(path)).map_err(|e| e.to_string())?;
                let value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                Ok((*key, value))
            })
            .collect::<Check<BTreeMap<_, _>>>()?;
        let mut result = Self {
            documents,
            by_file: BTreeMap::new(),
        };
        for (registry, array, file_pointer, kind) in [
            (
                Registry::Contracts,
                "/research_base_contexts",
                "/source_file",
                JoinKind::ResearchContext,
            ),
            (
                Registry::Nonleaf,
                "/coverage",
                "/source/source_file",
                JoinKind::NonleafBodyReview,
            ),
            (
                Registry::Functions,
                "/source_units",
                "/source_identity/source_file",
                JoinKind::FunctionBodyReview,
            ),
            (
                Registry::Constructors,
                "/source_units/xs_constructors",
                "/source/source_file",
                JoinKind::ConstructorReview,
            ),
            (
                Registry::Supporting,
                "/coverage",
                "/source_file",
                JoinKind::SupportingScopedReview,
            ),
            (
                Registry::Contracts,
                "/bounded_source_pages/API_pages",
                "/source_identity/file",
                JoinKind::ApiDecomposition,
            ),
            (
                Registry::Contracts,
                "/bounded_source_pages/appendix_pages",
                "/file",
                JoinKind::AppendixDecomposition,
            ),
        ] {
            let rows = result.documents[&registry]
                .pointer(array)
                .and_then(Value::as_array)
                .ok_or_else(|| format!("missing compared array {array}"))?;
            let files = rows
                .iter()
                .map(|row| {
                    row.pointer(file_pointer)
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .ok_or_else(|| format!("missing compared page {file_pointer}"))
                })
                .collect::<Check<Vec<_>>>()?;
            for (i, file) in files.into_iter().enumerate() {
                result.add(file, registry, format!("{array}/{i}"), kind);
            }
        }
        for (registry, array, url_key, kind) in [
            (
                Registry::Contracts,
                "/source_documents/release_and_scope_references",
                "official_url",
                JoinKind::SourceReference,
            ),
            (
                Registry::Ledger,
                "/evidence",
                "location",
                JoinKind::EvidenceReference,
            ),
        ] {
            let rows = result.documents[&registry]
                .pointer(array)
                .and_then(Value::as_array)
                .ok_or_else(|| format!("missing references {array}"))?;
            let files = rows
                .iter()
                .enumerate()
                .filter_map(|(i, row)| {
                    row[url_key]
                        .as_str()
                        .and_then(|url| url.strip_prefix(BASE))
                        .map(|file| (i, file.to_owned()))
                })
                .collect::<Vec<_>>();
            for (i, file) in files {
                result.add(file, registry, format!("{array}/{i}"), kind);
            }
        }
        require(
            result.documents[&Registry::Ledger]["inventory_complete"] == false,
            "canonical completeness changed",
        )?;
        require(
            result.documents[&Registry::Contracts]["inventory_complete"] == false,
            "source-contract completeness changed",
        )?;
        Ok(result)
    }

    fn add(&mut self, file: String, registry: Registry, pointer: String, kind: JoinKind) {
        self.by_file.entry(file).or_default().push(Join {
            registry,
            pointer,
            kind,
        });
    }

    pub fn resolve(&self, join: &Join) -> Check<&Value> {
        self.documents[&join.registry]
            .pointer(&join.pointer)
            .ok_or_else(|| "dangling compared-record pointer".to_owned())
    }
}
