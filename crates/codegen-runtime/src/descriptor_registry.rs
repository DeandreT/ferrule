//! Bounded process-wide preparations of emitter-owned endpoint descriptors.

use std::collections::BTreeMap;
use std::sync::{Arc, PoisonError, RwLock};

/// Most descriptors one registry retains.
pub(crate) const MAX_SHARED_DESCRIPTORS: usize = 64;

/// Immutable preparations keyed by descriptor text.
///
/// Generated mappings pass a finite set of descriptor constants to the
/// descriptor-taking endpoint functions, so each descriptor is prepared once
/// per process and the result is shared by repeated and concurrent calls.
/// Only successful preparations are retained: a rejected descriptor is
/// prepared again by every call and returns its original error in its
/// original order. A registry holds at most [`MAX_SHARED_DESCRIPTORS`]
/// descriptors totalling its descriptor byte budget; admitting one more starts
/// it over, and a descriptor larger than the whole budget is prepared for its
/// call without being retained. These bounds count descriptor text, not the
/// memory of the prepared values, which is typically an order of magnitude
/// larger.
pub(crate) struct DescriptorRegistry<T> {
    entries: RwLock<Entries<T>>,
    max_descriptor_bytes: usize,
}

struct Entries<T> {
    prepared: BTreeMap<Box<str>, Arc<T>>,
    descriptor_bytes: usize,
}

impl<T> DescriptorRegistry<T> {
    pub(crate) const fn new(max_descriptor_bytes: usize) -> Self {
        Self {
            entries: RwLock::new(Entries {
                prepared: BTreeMap::new(),
                descriptor_bytes: 0,
            }),
            max_descriptor_bytes,
        }
    }

    /// Returns the shared preparation of `descriptor`, preparing it on first use.
    pub(crate) fn get_or_prepare<E>(
        &self,
        descriptor: &str,
        prepare: impl FnOnce(&str) -> Result<T, E>,
    ) -> Result<Arc<T>, E> {
        if descriptor.len() > self.max_descriptor_bytes {
            return prepare(descriptor).map(Arc::new);
        }
        if let Some(prepared) = self
            .entries
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .prepared
            .get(descriptor)
        {
            return Ok(Arc::clone(prepared));
        }
        // Prepare outside the lock. Concurrent first calls may both prepare;
        // the first inserted value is the one every later call shares.
        let prepared = Arc::new(prepare(descriptor)?);
        let mut entries = self.entries.write().unwrap_or_else(PoisonError::into_inner);
        if let Some(existing) = entries.prepared.get(descriptor) {
            return Ok(Arc::clone(existing));
        }
        if entries.prepared.len() >= MAX_SHARED_DESCRIPTORS
            || entries.descriptor_bytes + descriptor.len() > self.max_descriptor_bytes
        {
            entries.prepared.clear();
            entries.descriptor_bytes = 0;
        }
        entries.descriptor_bytes += descriptor.len();
        entries
            .prepared
            .insert(descriptor.into(), Arc::clone(&prepared));
        Ok(prepared)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn retains_only_successful_preparations() {
        let registry = DescriptorRegistry::<String>::new(16);
        let calls = Cell::new(0);
        let prepare = |descriptor: &str| {
            calls.set(calls.get() + 1);
            if descriptor.starts_with('!') {
                Err(descriptor.len())
            } else {
                Ok(descriptor.to_uppercase())
            }
        };
        let first = registry.get_or_prepare("abc", prepare);
        let second = registry.get_or_prepare("abc", prepare);
        assert!(matches!((&first, &second), (Ok(a), Ok(b)) if Arc::ptr_eq(a, b) && **a == "ABC"));
        assert_eq!(calls.get(), 1);
        assert_eq!(registry.get_or_prepare("!x", prepare), Err(2));
        assert_eq!(registry.get_or_prepare("!x", prepare), Err(2));
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn descriptor_bytes_bound_retention() {
        let registry = DescriptorRegistry::<usize>::new(8);
        let calls = Cell::new(0);
        let prepare = |descriptor: &str| {
            calls.set(calls.get() + 1);
            Ok::<_, ()>(descriptor.len())
        };
        // Larger than the whole budget: prepared for each call, never retained.
        for _ in 0..2 {
            assert_eq!(
                registry.get_or_prepare("123456789", prepare).map(|v| *v),
                Ok(9)
            );
        }
        assert_eq!(calls.get(), 2);
        assert!(registry.get_or_prepare("12345", prepare).is_ok());
        assert!(registry.get_or_prepare("12345", prepare).is_ok());
        assert_eq!(calls.get(), 3);
        // Admitting four more bytes exceeds the budget and starts over.
        assert!(registry.get_or_prepare("6789", prepare).is_ok());
        assert!(registry.get_or_prepare("12345", prepare).is_ok());
        assert_eq!(calls.get(), 5);
    }

    #[test]
    fn concurrent_calls_share_one_preparation() {
        let registry = DescriptorRegistry::<String>::new(1024);
        let shared = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| registry.get_or_prepare("same", |d| Ok::<_, ()>(d.to_owned())))
                })
                .collect();
            handles
                .into_iter()
                .filter_map(|handle| handle.join().ok().and_then(Result::ok))
                .collect::<Vec<_>>()
        });
        assert_eq!(shared.len(), 8);
        let Ok(retained) = registry.get_or_prepare("same", |_| Err(())) else {
            panic!("the first inserted preparation is retained");
        };
        assert!(shared.iter().all(|value| Arc::ptr_eq(value, &retained)));
    }
}
