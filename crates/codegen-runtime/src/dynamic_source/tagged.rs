//! Tagged host contracts and their adapter to the existing generated boundaries.
//!
//! Legacy string loaders need no adapter: each legacy trait's provided
//! `load_tagged` method is the old-to-new adaptation.

use ir::Instance;

use super::{DynamicJsonSourceLoader, DynamicSourceFault, DynamicSourceLoader};
use crate::DynamicXmlSourceLoader;

/// Tagged host contract for already parsed dynamic documents.
pub trait TaggedDynamicSourceLoader {
    fn load_tagged(&self, source: &str, logical_path: &str)
    -> Result<Instance, DynamicSourceFault>;
}

/// Tagged host contract for raw JSON or XML dynamic documents.
pub trait TaggedDynamicBytesLoader {
    fn load_tagged(&self, source: &str, logical_path: &str) -> Result<Vec<u8>, DynamicSourceFault>;
}

/// Presents a tagged host wherever generated mappings accept a legacy loader.
///
/// Generated code calls `load_tagged` and receives each original fault. The
/// legacy `load` methods render the same fault with
/// [`DynamicSourceFault::into_message`].
pub struct TaggedLoaderAdapter<'a, L: ?Sized>(pub &'a L);

impl<L: TaggedDynamicSourceLoader + ?Sized> DynamicSourceLoader for TaggedLoaderAdapter<'_, L> {
    fn load(&self, source: &str, logical_path: &str) -> Result<Instance, String> {
        TaggedDynamicSourceLoader::load_tagged(self.0, source, logical_path)
            .map_err(DynamicSourceFault::into_message)
    }

    fn load_tagged(
        &self,
        source: &str,
        logical_path: &str,
    ) -> Result<Instance, DynamicSourceFault> {
        TaggedDynamicSourceLoader::load_tagged(self.0, source, logical_path)
    }
}

impl<L: TaggedDynamicBytesLoader + ?Sized> DynamicJsonSourceLoader for TaggedLoaderAdapter<'_, L> {
    fn load(&self, source: &str, logical_path: &str) -> Result<Vec<u8>, String> {
        TaggedDynamicBytesLoader::load_tagged(self.0, source, logical_path)
            .map_err(DynamicSourceFault::into_message)
    }

    fn load_tagged(&self, source: &str, logical_path: &str) -> Result<Vec<u8>, DynamicSourceFault> {
        TaggedDynamicBytesLoader::load_tagged(self.0, source, logical_path)
    }
}

impl<L: TaggedDynamicBytesLoader + ?Sized> DynamicXmlSourceLoader for TaggedLoaderAdapter<'_, L> {
    fn load(&self, source: &str, logical_path: &str) -> Result<Vec<u8>, String> {
        TaggedDynamicBytesLoader::load_tagged(self.0, source, logical_path)
            .map_err(DynamicSourceFault::into_message)
    }

    fn load_tagged(&self, source: &str, logical_path: &str) -> Result<Vec<u8>, DynamicSourceFault> {
        TaggedDynamicBytesLoader::load_tagged(self.0, source, logical_path)
    }
}
