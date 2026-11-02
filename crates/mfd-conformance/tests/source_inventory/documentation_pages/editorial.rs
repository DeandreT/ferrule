//! Candidate-derived regression fingerprints, NOT an independent source oracle.
//!
//! Keep source/facet validation separate. These fingerprints only prevent later
//! prose-only edits from escaping those structural controls after editorial review.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use super::types::Inventory;
use super::{Check, oracle, require};

fn digest<'a>(values: impl IntoIterator<Item = &'a str>) -> String {
    let mut hash = Sha256::new();
    for value in values {
        hash.update((value.len() as u64).to_be_bytes());
        hash.update(value.as_bytes());
    }
    format!("{:x}", hash.finalize())
}

pub(super) fn validate(inventory: &Inventory) -> Check {
    let frozen: Vec<(Option<usize>, String)> = oracle(include_str!(
        "../../../../../conformance/mfd-2026r2-source-inventory/oracles/documentation-editorial-sha256.jsonl"
    ))?;
    let mut actual = BTreeMap::new();
    actual.insert(
        None,
        digest(
            std::iter::once(inventory.purpose.as_str())
                .chain(inventory.limitations.iter().map(String::as_str)),
        ),
    );
    for page in &inventory.pages {
        if let Some(disposition) = &page.disposition {
            actual.insert(
                Some(page.index),
                digest(
                    std::iter::once(disposition.authored_reason.as_str()).chain(
                        disposition
                            .observations
                            .iter()
                            .map(|observation| observation.authored_claim.as_str()),
                    ),
                ),
            );
        }
    }
    require(
        frozen.len() == 25 && actual.len() == frozen.len(),
        "editorial regression extent",
    )?;
    for (index, hash) in frozen {
        require(
            actual.get(&index) == Some(&hash),
            &format!("editorial wording changed at {index:?}; requires source readback"),
        )?;
    }
    Ok(())
}
