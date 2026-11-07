//! Registration events extend the negative union before the ordinary value is
//! retained. They do not depend on positive origins, selected outputs or plans.
use super::*;
use crate::import::{
    external_scalar, external_xslt, flextext_parser, json_parser, json_serializer, udf::Call,
    xml_serializer,
};

pub(in crate::import) trait Fact {
    const DOMAIN: u8;
    fn routing(&self, event: &mut Event<'_>) -> Result<(), String>;
}
pub(in crate::import) struct Event<'a> {
    pub(super) state: &'a mut Routing,
    pub(super) vertex: Key,
    pub(super) origin: Origin,
    pub(super) slot: usize,
    pub(super) replay: bool,
}
impl Event<'_> {
    pub fn metadata(&mut self, bytes: usize) -> Result<(), String> {
        self.state.ledger.extend_registered(1, bytes)?;
        self.state.ledger.text(bytes)
    }
    pub fn pin(&mut self, key: u32, output: bool) -> Result<(), String> {
        self.state.ledger.extend_registered(1, 0)?;
        if output {
            self.state.link(self.vertex, Key::Port(key))
        } else {
            self.state.link(Key::Port(key), self.vertex)
        }
    }
    fn raw(&self) -> Option<RawOwner> {
        match self.origin {
            Origin::ActiveMain(owner) => Some(owner),
            _ => None,
        }
    }
    pub fn count(&mut self) -> Result<(), String> {
        self.state.ledger.work(self.state.counts.0.len() + 1)?;
        if !self.replay || !self.state.counts.0.iter().any(|c| c.slot == self.slot) {
            self.state.counts.push(
                CountRegistration {
                    slot: self.slot,
                    raw: self.raw(),
                    vertex: self.vertex,
                },
                &mut self.state.ledger,
            )?;
        }
        Ok(())
    }
    pub fn filter(&mut self) -> Result<(), String> {
        self.state.ledger.work(self.state.filters.0.len() + 1)?;
        if !self.replay || !self.state.filters.0.iter().any(|f| f.slot == self.slot) {
            self.state.filters.push(
                FilterRegistration {
                    raw: self.raw(),
                    slot: self.slot,
                },
                &mut self.state.ledger,
            )?;
        }
        Ok(())
    }
    pub fn group(&mut self, key: u32) -> Result<(), String> {
        if !self.replay {
            self.state.groups.push(
                GroupClaim {
                    slot: self.slot,
                    key,
                    raw: self.raw(),
                },
                &mut self.state.ledger,
            )?;
        }
        Ok(())
    }
}

impl Fact for FnComponent {
    const DOMAIN: u8 = 1;
    fn routing(&self, event: &mut Event<'_>) -> Result<(), String> {
        event.metadata(self.name.len() + self.library.len())?;
        if self.name == "count" && self.kind == 5 {
            event.count()?;
        }
        if self.name == "filter" {
            event.filter()?;
        }
        for key in self.inputs.iter().flatten() {
            event.pin(*key, false)?;
        }
        for key in self.outputs.iter().chain(&self.declared_output_keys) {
            event.pin(*key, true)?;
        }
        Ok(())
    }
}
impl Fact for SchemaComponent {
    const DOMAIN: u8 = 0;
    fn routing(&self, event: &mut Event<'_>) -> Result<(), String> {
        event.metadata(0)?;
        for key in &self.input_keys {
            event.pin(*key, false)?;
        }
        // ports also includes original aliases that are not output_keys.
        for key in self.ports.keys().chain(&self.output_keys) {
            event.pin(*key, true)?;
        }
        for query in &self.db_queries {
            for key in query.certificate_computed_outputs() {
                event.pin(key, true)?;
            }
        }
        if self.format == ComponentFormat::Json || self.is_variable {
            for key in self.ports.keys() {
                event.group(*key)?;
            }
        }
        Ok(())
    }
}
impl Fact for Call {
    const DOMAIN: u8 = 2;
    fn routing(&self, event: &mut Event<'_>) -> Result<(), String> {
        event.metadata(0)?;
        for key in self.input_pins() {
            event.pin(key, false)?;
        }
        for key in self.outputs.keys() {
            event.pin(*key, true)?;
        }
        Ok(())
    }
}
macro_rules! scalar_recipe {
    ($ty:ty, $domain:expr) => {
        impl Fact for $ty {
            const DOMAIN: u8 = $domain;
            fn routing(&self, event: &mut Event<'_>) -> Result<(), String> {
                event.metadata(0)?;
                for key in self.input_pins() {
                    event.pin(key, false)?;
                }
                event.pin(self.output, true)
            }
        }
    };
}
scalar_recipe!(external_scalar::Recipe, 3);
scalar_recipe!(external_xslt::Recipe, 4);
scalar_recipe!(json_serializer::Recipe, 5);
scalar_recipe!(xml_serializer::Recipe, 6);
impl Fact for json_parser::Recipe {
    const DOMAIN: u8 = 7;
    fn routing(&self, event: &mut Event<'_>) -> Result<(), String> {
        event.metadata(0)?;
        for key in self.input_pins() {
            event.pin(key, false)?;
        }
        for key in self.outputs.keys().chain(self.object_outputs.keys()) {
            event.pin(*key, true)?;
        }
        Ok(())
    }
}
impl Fact for flextext_parser::Recipe {
    const DOMAIN: u8 = 8;
    fn routing(&self, event: &mut Event<'_>) -> Result<(), String> {
        event.metadata(0)?;
        for key in self.input_pins() {
            event.pin(key, false)?;
        }
        for key in self.outputs.keys() {
            event.pin(*key, true)?;
        }
        Ok(())
    }
}
#[cfg(test)]
impl Fact for i32 {
    const DOMAIN: u8 = 9;
    fn routing(&self, event: &mut Event<'_>) -> Result<(), String> {
        event.metadata(0)
    }
}
