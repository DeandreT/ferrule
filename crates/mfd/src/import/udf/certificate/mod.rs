//! Import-only ownership certificate, discarded before project publication.
mod materialized;
mod origin;
mod view;
pub(in crate::import) use materialized::ArgumentSeal;
#[cfg(test)]
pub(in crate::import) use materialized::probe_speculative_builder;
pub(in crate::import) use origin::{CatalogSource, Origin, RawOwner, RegisteredVec, SchemaSink};
pub(in crate::import) use view::{Normalized, certify};

use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RawPin {
    pub(super) owner: RawOwner,
    pub(super) range: (usize, usize),
    pub(super) output: bool,
}

pub(super) enum Profile {
    XmlField {
        path: Vec<String>,
        attribute: bool,
    },
    Function {
        inputs: Vec<Option<u32>>,
        outputs: Vec<u32>,
    },
    DeclaredCall,
    Unproved,
}

pub(super) struct RawOutput {
    pub(super) pin: RawPin,
    pub(super) profile: Profile,
}

pub(super) struct Boundary {
    pub(super) pin: RawPin,
    pub(super) key: u32,
    pub(super) output: u32,
    pub(super) attribute: bool,
    pub(super) path: Option<Vec<String>>,
    pub(super) scopes: Vec<(Vec<String>, u32, u32)>,
}

#[derive(Default)]
pub(super) struct ArgumentClosure {
    pub(super) arguments: BTreeMap<u32, RawOutput>,
    pub(super) drivers: BTreeMap<u32, RawOutput>,
    pub(super) boundaries: Vec<Boundary>,
    pub(super) outputs: BTreeMap<u32, RawOutput>,
}
