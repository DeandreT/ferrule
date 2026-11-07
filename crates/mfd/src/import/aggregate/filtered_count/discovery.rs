//! Installation accepts only a completed raw/registered negative certificate.
use super::*;

pub(super) fn discover(
    table: &mut CountProofTable,
    demand: routing::CompleteDemand,
) -> Result<(), String> {
    demand.install(table)
}
