//! Input-derived negative resource policy, independent of optional proof work.
//! Slots/bytes describe explicit Vec capacities, not allocator bookkeeping or
//! raw XML size. No strings or literal bodies are copied into routing storage.
use super::super::{Counter, count_visit};

#[derive(Clone, Copy, Debug, Default)]
pub(in crate::import) struct Dimensions {
    pub nodes: usize,
    pub pins: usize,
    pub edges: usize,
    pub attributes: usize,
    pub decoded: usize,
    pub registered: usize,
}

#[derive(Debug)]
pub(in crate::import) struct Ledger {
    pub dimensions: Dimensions,
    pub work: u128,
    pub text_work: u128,
    pub slots: usize,
    pub bytes: usize,
    pub failed: bool,
}

impl Ledger {
    pub fn new(dimensions: Dimensions) -> Self {
        Self {
            dimensions,
            work: 0,
            text_work: 0,
            slots: 0,
            bytes: 0,
            failed: false,
        }
    }
    /// Linear storage: geometric capacities, raw + registered links, two graph
    /// snapshots, reverse traversal and freeze receipts together use <256 slots
    /// per inspected node/attribute/pin/edge/event. Graph-array records are
    /// <=128 bytes; control blocks and route receipts use their actual sizes.
    /// Work permits charged linear scans and integer sorting/lookup even in the
    /// worst (quadratic) owner lookup case, plus repeated decoded numeric reads.
    pub fn limits(&self) -> (u128, usize, usize) {
        let d = self.dimensions;
        let units = (d.nodes as u128)
            + d.attributes as u128
            + d.pins as u128
            + d.edges as u128
            + d.registered as u128
            + 1;
        let work = 1024 * units * (units + d.decoded as u128 + 1);
        let slots = usize::try_from(256 * units).unwrap_or(usize::MAX);
        (work, slots, slots.saturating_mul(128))
    }
    pub fn extend_registered(&mut self, facts: usize, decoded: usize) -> Result<(), String> {
        self.check()?;
        let Some(registered) = self.dimensions.registered.checked_add(facts) else {
            return self.fail();
        };
        let Some(decoded) = self.dimensions.decoded.checked_add(decoded) else {
            return self.fail();
        };
        self.dimensions.registered = registered;
        self.dimensions.decoded = decoded;
        self.work(1)
    }
    pub fn work(&mut self, work: usize) -> Result<(), String> {
        self.check()?;
        let next = self.work + work as u128;
        if next > self.limits().0 {
            return self.fail();
        }
        self.work = next;
        count_visit(Counter::NegativeWork);
        Ok(())
    }
    pub fn text(&mut self, bytes: usize) -> Result<(), String> {
        self.work(bytes)?;
        self.text_work += bytes as u128;
        Ok(())
    }
    pub fn reserve(&mut self, slots: usize, bytes: usize) -> Result<(), String> {
        self.check()?;
        let Some(next_slots) = self.slots.checked_add(slots) else {
            return self.fail();
        };
        let Some(next_bytes) = self.bytes.checked_add(bytes) else {
            return self.fail();
        };
        let (_, slot_limit, byte_limit) = self.limits();
        if next_slots > slot_limit || next_bytes > byte_limit {
            return self.fail();
        }
        self.slots = next_slots;
        self.bytes = next_bytes;
        count_visit(Counter::NegativeReservation);
        Ok(())
    }
    fn fail<T>(&mut self) -> Result<T, String> {
        self.failed = true;
        Err("input-derived Count routing reservation failed".into())
    }
    pub fn check(&self) -> Result<(), String> {
        if self.failed {
            Err("incomplete input-derived Count routing".into())
        } else {
            Ok(())
        }
    }
    pub fn lookup(&mut self, length: usize) -> Result<(), String> {
        self.work(8 * (2 + usize::BITS as usize - length.leading_zeros() as usize))
    }
    pub fn sort(&mut self, length: usize) -> Result<(), String> {
        let Some(work) =
            length.checked_mul(16 * (2 + usize::BITS as usize - length.leading_zeros() as usize))
        else {
            return self.fail();
        };
        self.work(work)
    }
}

#[derive(Debug, Default)]
pub(super) struct Buffer<T>(pub Vec<T>);
impl<T> Buffer<T> {
    pub fn new() -> Self {
        Self(Vec::new())
    }
    pub fn reserve(&mut self, additional: usize, ledger: &mut Ledger) -> Result<(), String> {
        ledger.check()?;
        let Some(required) = self.0.len().checked_add(additional) else {
            return ledger.fail();
        };
        if required > self.0.capacity() {
            ledger.work(self.0.len() + 1)?;
            let capacity = required.max(self.0.capacity().saturating_mul(2)).max(4);
            let slots = capacity - self.0.capacity();
            let Some(bytes) = slots.checked_mul(std::mem::size_of::<T>()) else {
                return ledger.fail();
            };
            ledger.reserve(slots, bytes)?;
            if self.0.try_reserve_exact(capacity - self.0.len()).is_err() {
                ledger.failed = true;
                return Err("Count routing host allocation failed".into());
            }
        }
        Ok(())
    }
    pub fn push(&mut self, value: T, ledger: &mut Ledger) -> Result<(), String> {
        ledger.work(1)?;
        self.reserve(1, ledger)?;
        self.0.push(value);
        Ok(())
    }
}
