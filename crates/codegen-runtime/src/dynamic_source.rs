use ir::{Instance, Value};

use crate::{RuntimeError, ScopeContext};

/// Maximum number of per-driver documents one dynamic source may request.
pub const MAX_DYNAMIC_SOURCE_LOADS: usize = 1_000_000;

/// Maximum UTF-8 byte length of one host-owned logical source path.
pub const MAX_DYNAMIC_SOURCE_PATH_BYTES: usize = 4096;

/// Maximum document size accepted by generated JSON loader adapters.
pub const MAX_DYNAMIC_SOURCE_BYTES: usize = 64 * 1024 * 1024;

/// Maximum combined bytes accepted by one generated JSON mapping execution.
pub const MAX_DYNAMIC_SOURCE_TOTAL_BYTES: usize = 256 * 1024 * 1024;

/// Host boundary for already parsed dynamic source documents.
///
/// Generated mappings never access a filesystem. The host resolves and
/// confines `logical_path`, then returns exactly one instance matching the
/// source's embedded schema.
pub trait DynamicSourceLoader {
    fn load(&self, source: &str, logical_path: &str) -> Result<Instance, String>;
}

/// Host boundary used by generated schema-shaped JSON entry points.
///
/// Generated code applies byte and UTF-8 limits and parses the returned
/// document against the dynamic source's embedded schema.
pub trait DynamicJsonSourceLoader {
    fn load(&self, source: &str, logical_path: &str) -> Result<Vec<u8>, String>;
}

/// Owns documents loaded for one dynamic source and retains the exact driver
/// context associated with each document.
pub struct DynamicSourceItems<'a> {
    source: &'static str,
    tail: Vec<String>,
    drivers: Vec<ScopeContext<'a>>,
    documents: Vec<Instance>,
}

impl<'a> DynamicSourceItems<'a> {
    pub fn load(
        context: &ScopeContext<'a>,
        source: &'static str,
        driver: &[&str],
        tail: &[&str],
        node: u32,
        mut path: impl FnMut(&ScopeContext<'a>) -> Result<Value, RuntimeError>,
    ) -> Result<Self, RuntimeError> {
        let loader = context
            .dynamic_source_loader()
            .ok_or(RuntimeError::MissingDynamicSourceLoader { source })?;
        let candidates = context.walk_source(driver);
        if candidates.len() > MAX_DYNAMIC_SOURCE_LOADS {
            return Err(RuntimeError::DynamicSourceTooMany {
                source,
                maximum: MAX_DYNAMIC_SOURCE_LOADS,
            });
        }

        let mut drivers = Vec::with_capacity(candidates.len());
        let mut documents = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            let logical_path = match path(&candidate)? {
                Value::Null | Value::JsonNull(_) => continue,
                Value::String(path) => path,
                value => {
                    return Err(RuntimeError::DynamicSourcePath {
                        source,
                        node,
                        found: value.type_name(),
                    });
                }
            };
            if logical_path.len() > MAX_DYNAMIC_SOURCE_PATH_BYTES {
                return Err(RuntimeError::DynamicSourcePathTooLong {
                    source,
                    maximum: MAX_DYNAMIC_SOURCE_PATH_BYTES,
                });
            }
            let document = loader.load(source, &logical_path).map_err(|message| {
                RuntimeError::DynamicSourceLoad {
                    source,
                    path: logical_path,
                    message,
                }
            })?;
            drivers.push(candidate);
            documents.push(document);
        }

        Ok(Self {
            source,
            tail: tail.iter().map(|segment| (*segment).to_string()).collect(),
            drivers,
            documents,
        })
    }

    /// Reborrows each loaded document and appends only its matching driver
    /// frames, preserving deterministic driver and document order.
    pub fn contexts<'b>(&'b self) -> Vec<ScopeContext<'b>>
    where
        'a: 'b,
    {
        let tail = self.tail.iter().map(String::as_str).collect::<Vec<_>>();
        self.drivers
            .iter()
            .zip(&self.documents)
            .flat_map(|(driver, document)| driver.walk_loaded_source(self.source, document, &tail))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use ir::{Instance, Value};

    use super::*;
    use crate::{field, group, repeated, scalar};

    struct Loader {
        calls: RefCell<Vec<(String, String)>>,
    }

    impl DynamicSourceLoader for Loader {
        fn load(&self, source: &str, path: &str) -> Result<Instance, String> {
            self.calls
                .borrow_mut()
                .push((source.to_string(), path.to_string()));
            Ok(group([field(
                "Rows",
                repeated([group([field("value", scalar(Value::String(path.into())))])]),
            )]))
        }
    }

    #[test]
    fn loads_in_driver_order_and_retains_each_driver_context() {
        let source = group([field(
            "Files",
            repeated([
                group([field("path", scalar(Value::String("a.json".into())))]),
                group([field("path", scalar(Value::String("b.json".into())))]),
            ]),
        )]);
        let loader = Loader {
            calls: RefCell::new(Vec::new()),
        };
        let root = ScopeContext::new(&source).with_dynamic_source_loader(&loader);
        let items =
            DynamicSourceItems::load(&root, "Catalog", &["Files"], &["Rows"], 7, |driver| {
                driver.resolve_scalar(&["path"]).map_err(Into::into)
            })
            .unwrap();
        let contexts = items.contexts();

        assert_eq!(
            loader.calls.borrow().clone(),
            [
                ("Catalog".into(), "a.json".into()),
                ("Catalog".into(), "b.json".into())
            ]
        );
        assert_eq!(
            contexts
                .iter()
                .map(|context| context.resolve_scalar(&["path"]).unwrap())
                .collect::<Vec<_>>(),
            [
                Value::String("a.json".into()),
                Value::String("b.json".into())
            ]
        );
        assert_eq!(
            contexts
                .iter()
                .map(|context| context.resolve_scalar(&["value"]).unwrap())
                .collect::<Vec<_>>(),
            [
                Value::String("a.json".into()),
                Value::String("b.json".into())
            ]
        );
    }

    #[test]
    fn skips_absent_paths_and_reports_typed_host_boundaries() {
        let source = repeated([
            scalar(Value::Null),
            scalar(Value::JsonNull(ir::JsonNull)),
            scalar(Value::Bool(true)),
        ]);
        let loader = Loader {
            calls: RefCell::new(Vec::new()),
        };
        let root = ScopeContext::new(&source).with_dynamic_source_loader(&loader);
        assert_eq!(
            DynamicSourceItems::load(&root, "Catalog", &[], &[], 9, |context| {
                context.resolve_scalar(&[]).map_err(Into::into)
            })
            .err(),
            Some(RuntimeError::DynamicSourcePath {
                source: "Catalog",
                node: 9,
                found: "bool",
            })
        );
        assert_eq!(*loader.calls.borrow(), Vec::new());
        assert_eq!(
            DynamicSourceItems::load(&ScopeContext::new(&source), "Catalog", &[], &[], 9, |_| Ok(
                Value::String("a.json".into())
            ))
            .err(),
            Some(RuntimeError::MissingDynamicSourceLoader { source: "Catalog" })
        );
    }
}
