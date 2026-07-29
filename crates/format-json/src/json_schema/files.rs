use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::JsonFormatError;

const MAX_DOCUMENTS: usize = 256;
const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
const MAX_REFERENCE_DEPTH: usize = 64;
const MAX_JSON_DEPTH: usize = 128;
const MAX_REFERENCES: usize = 100_000;
const EXTERNAL_DEFS_KEY: &str = "__ferrule_external_documents";
const IGNORE_REF_SIBLINGS_KEY: &str = "__ferrule_ignore_ref_siblings";
const VALIDATION_DIALECT_KEY: &str = "__ferrule_validation_dialect";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ValidationDialect {
    Draft4,
    Draft6Or7,
    Modern,
}

impl ValidationDialect {
    pub(super) fn supports_property_names(self) -> bool {
        !matches!(self, Self::Draft4)
    }

    pub(super) fn supports_contains(self) -> bool {
        !matches!(self, Self::Draft4)
    }

    pub(super) fn supports_contains_counts(self) -> bool {
        matches!(self, Self::Modern)
    }

    fn marker(self) -> &'static str {
        match self {
            Self::Draft4 => "draft4",
            Self::Draft6Or7 => "draft6_or_7",
            Self::Modern => "modern",
        }
    }
}

struct Document {
    path: PathBuf,
    value: serde_json::Value,
}

struct Loader {
    package_root: PathBuf,
    indexes: BTreeMap<PathBuf, usize>,
    documents: Vec<Document>,
    total_bytes: usize,
    references: usize,
}

pub(super) fn load(path: &Path, package_root: &Path) -> Result<serde_json::Value, JsonFormatError> {
    let package_root =
        std::fs::canonicalize(package_root).map_err(|error| JsonFormatError::SchemaResource {
            reference: package_root.display().to_string(),
            base: package_root.to_path_buf(),
            reason: error.to_string(),
        })?;
    if !package_root.is_dir() {
        return Err(JsonFormatError::SchemaResource {
            reference: package_root.display().to_string(),
            base: package_root.clone(),
            reason: "the package root is not a directory".to_string(),
        });
    }
    let root_path =
        std::fs::canonicalize(path).map_err(|error| JsonFormatError::SchemaResource {
            reference: path.display().to_string(),
            base: package_root.clone(),
            reason: error.to_string(),
        })?;
    ensure_contained(&root_path, &package_root, path, &package_root)?;

    let mut loader = Loader {
        package_root,
        indexes: BTreeMap::new(),
        documents: Vec::new(),
        total_bytes: 0,
        references: 0,
    };
    let root_index = loader.load_document(&root_path, 0)?;
    debug_assert_eq!(root_index, 0);
    loader.finish()
}

impl Loader {
    fn load_document(
        &mut self,
        path: &Path,
        reference_depth: usize,
    ) -> Result<usize, JsonFormatError> {
        if let Some(index) = self.indexes.get(path) {
            return Ok(*index);
        }
        if reference_depth > MAX_REFERENCE_DEPTH {
            return Err(JsonFormatError::SchemaResourceLimit {
                kind: "reference depth",
                limit: MAX_REFERENCE_DEPTH,
            });
        }
        if self.documents.len() >= MAX_DOCUMENTS {
            return Err(JsonFormatError::SchemaResourceLimit {
                kind: "documents",
                limit: MAX_DOCUMENTS,
            });
        }

        let bytes = std::fs::read(path).map_err(|error| JsonFormatError::SchemaResource {
            reference: path.display().to_string(),
            base: path.parent().unwrap_or(path).to_path_buf(),
            reason: error.to_string(),
        })?;
        self.total_bytes = self.total_bytes.checked_add(bytes.len()).ok_or(
            JsonFormatError::SchemaResourceLimit {
                kind: "total bytes",
                limit: MAX_TOTAL_BYTES,
            },
        )?;
        if self.total_bytes > MAX_TOTAL_BYTES {
            return Err(JsonFormatError::SchemaResourceLimit {
                kind: "total bytes",
                limit: MAX_TOTAL_BYTES,
            });
        }
        let mut value = serde_json::from_slice(&bytes)?;
        let dialect = validation_dialect_for_document(&value);
        let ignore_ref_siblings = !matches!(dialect, ValidationDialect::Modern);
        let index = self.documents.len();
        self.indexes.insert(path.to_path_buf(), index);
        self.documents.push(Document {
            path: path.to_path_buf(),
            value: serde_json::Value::Null,
        });
        self.rewrite_references(
            &mut value,
            index,
            reference_depth,
            0,
            ignore_ref_siblings,
            dialect,
        )?;
        self.documents[index].value = value;
        Ok(index)
    }

    fn rewrite_references(
        &mut self,
        value: &mut serde_json::Value,
        document_index: usize,
        reference_depth: usize,
        json_depth: usize,
        ignore_ref_siblings: bool,
        dialect: ValidationDialect,
    ) -> Result<(), JsonFormatError> {
        if json_depth > MAX_JSON_DEPTH {
            return Err(JsonFormatError::SchemaResourceLimit {
                kind: "JSON nesting depth",
                limit: MAX_JSON_DEPTH,
            });
        }
        match value {
            serde_json::Value::Object(object) => {
                if object.contains_key(IGNORE_REF_SIBLINGS_KEY) {
                    return self.resource_error(
                        document_index,
                        IGNORE_REF_SIBLINGS_KEY,
                        "a schema object uses ferrule's reserved `$ref` policy key",
                    );
                }
                if object.contains_key(VALIDATION_DIALECT_KEY) {
                    return self.resource_error(
                        document_index,
                        VALIDATION_DIALECT_KEY,
                        "a schema object uses ferrule's reserved dialect-policy key",
                    );
                }
                let reference = object
                    .get("$ref")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string);
                if let Some(reference) = reference {
                    self.references += 1;
                    if self.references > MAX_REFERENCES {
                        return Err(JsonFormatError::SchemaResourceLimit {
                            kind: "references",
                            limit: MAX_REFERENCES,
                        });
                    }
                    let rewritten =
                        self.resolve_reference(document_index, &reference, reference_depth)?;
                    object.insert("$ref".to_string(), serde_json::Value::String(rewritten));
                    if ignore_ref_siblings {
                        object.insert(
                            IGNORE_REF_SIBLINGS_KEY.to_string(),
                            serde_json::Value::Bool(true),
                        );
                    }
                }
                if object.contains_key("$ref")
                    || ["propertyNames", "contains", "minContains", "maxContains"]
                        .into_iter()
                        .any(|keyword| object.contains_key(keyword))
                {
                    object.insert(
                        VALIDATION_DIALECT_KEY.to_string(),
                        serde_json::Value::String(dialect.marker().to_string()),
                    );
                }
                for child in object.values_mut() {
                    self.rewrite_references(
                        child,
                        document_index,
                        reference_depth,
                        json_depth + 1,
                        ignore_ref_siblings,
                        dialect,
                    )?;
                }
            }
            serde_json::Value::Array(items) => {
                for child in items {
                    self.rewrite_references(
                        child,
                        document_index,
                        reference_depth,
                        json_depth + 1,
                        ignore_ref_siblings,
                        dialect,
                    )?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn resolve_reference(
        &mut self,
        document_index: usize,
        reference: &str,
        reference_depth: usize,
    ) -> Result<String, JsonFormatError> {
        let (resource, encoded_fragment) = reference
            .split_once('#')
            .map_or((reference, ""), |(resource, fragment)| (resource, fragment));
        let fragment = decode_uri_component(encoded_fragment).ok_or_else(|| {
            self.make_resource_error(
                document_index,
                reference,
                "the fragment contains invalid percent encoding or UTF-8",
            )
        })?;
        if !fragment.is_empty() && !fragment.starts_with('/') {
            return self.resource_error(
                document_index,
                reference,
                "only JSON Pointer fragments are supported; named anchors are not",
            );
        }

        let target_index = if resource.is_empty() {
            document_index
        } else {
            let decoded = decode_uri_component(resource).ok_or_else(|| {
                self.make_resource_error(
                    document_index,
                    reference,
                    "the resource path contains invalid percent encoding or UTF-8",
                )
            })?;
            if decoded.contains('?')
                || Path::new(&decoded).is_absolute()
                || has_uri_scheme(&decoded)
            {
                return self.resource_error(
                    document_index,
                    reference,
                    "only relative local-file references are supported",
                );
            }
            let base = self.documents[document_index]
                .path
                .parent()
                .unwrap_or(&self.documents[document_index].path);
            let candidate = base.join(decoded.replace('\\', "/"));
            let canonical = std::fs::canonicalize(&candidate).map_err(|error| {
                self.make_resource_error(document_index, reference, &error.to_string())
            })?;
            ensure_contained(&canonical, &self.package_root, Path::new(reference), base)?;
            self.load_document(&canonical, reference_depth + 1)?
        };

        if target_index == 0 {
            Ok(format!("#{fragment}"))
        } else {
            Ok(format!(
                "#/$defs/{EXTERNAL_DEFS_KEY}/{target_index}{fragment}"
            ))
        }
    }

    fn finish(mut self) -> Result<serde_json::Value, JsonFormatError> {
        let mut root = std::mem::take(&mut self.documents[0].value);
        if self.documents.len() == 1 {
            return Ok(root);
        }
        let root_object = root
            .as_object_mut()
            .ok_or_else(|| JsonFormatError::SchemaResource {
                reference: self.documents[0].path.display().to_string(),
                base: self.package_root.clone(),
                reason: "a schema with external references must have an object root".to_string(),
            })?;
        let definitions = match root_object.entry("$defs".to_string()) {
            serde_json::map::Entry::Vacant(entry) => {
                entry.insert(serde_json::Value::Object(serde_json::Map::new()))
            }
            serde_json::map::Entry::Occupied(entry) => entry.into_mut(),
        };
        let definitions =
            definitions
                .as_object_mut()
                .ok_or_else(|| JsonFormatError::SchemaResource {
                    reference: self.documents[0].path.display().to_string(),
                    base: self.package_root.clone(),
                    reason: "`$defs` must be an object when external references are used"
                        .to_string(),
                })?;
        if definitions.contains_key(EXTERNAL_DEFS_KEY) {
            return Err(JsonFormatError::SchemaResource {
                reference: self.documents[0].path.display().to_string(),
                base: self.package_root,
                reason: format!(
                    "reserved `$defs/{EXTERNAL_DEFS_KEY}` key conflicts with the resource bundle"
                ),
            });
        }
        let external = self
            .documents
            .into_iter()
            .enumerate()
            .skip(1)
            .map(|(index, document)| (index.to_string(), document.value))
            .collect();
        definitions.insert(
            EXTERNAL_DEFS_KEY.to_string(),
            serde_json::Value::Object(external),
        );
        Ok(root)
    }

    fn resource_error<T>(
        &self,
        document_index: usize,
        reference: &str,
        reason: &str,
    ) -> Result<T, JsonFormatError> {
        Err(self.make_resource_error(document_index, reference, reason))
    }

    fn make_resource_error(
        &self,
        document_index: usize,
        reference: &str,
        reason: &str,
    ) -> JsonFormatError {
        JsonFormatError::SchemaResource {
            reference: reference.to_string(),
            base: self.documents[document_index].path.clone(),
            reason: reason.to_string(),
        }
    }
}

pub(super) fn ref_siblings_apply(schema: &serde_json::Value) -> bool {
    schema.get(IGNORE_REF_SIBLINGS_KEY) != Some(&serde_json::Value::Bool(true))
}

pub(super) fn is_internal_ref_keyword(keyword: &str) -> bool {
    matches!(keyword, IGNORE_REF_SIBLINGS_KEY | VALIDATION_DIALECT_KEY)
}

pub(super) fn validation_dialect(schema: &serde_json::Value) -> ValidationDialect {
    match schema
        .get(VALIDATION_DIALECT_KEY)
        .and_then(serde_json::Value::as_str)
    {
        Some("draft4") => ValidationDialect::Draft4,
        Some("draft6_or_7") => ValidationDialect::Draft6Or7,
        _ => ValidationDialect::Modern,
    }
}

fn validation_dialect_for_document(schema: &serde_json::Value) -> ValidationDialect {
    let Some(dialect) = schema.get("$schema").and_then(serde_json::Value::as_str) else {
        return ValidationDialect::Modern;
    };
    let dialect = dialect.strip_suffix('#').unwrap_or(dialect);
    match dialect {
        "http://json-schema.org/draft-04/schema" | "https://json-schema.org/draft-04/schema" => {
            ValidationDialect::Draft4
        }
        "http://json-schema.org/draft-06/schema"
        | "https://json-schema.org/draft-06/schema"
        | "http://json-schema.org/draft-07/schema"
        | "https://json-schema.org/draft-07/schema" => ValidationDialect::Draft6Or7,
        _ => ValidationDialect::Modern,
    }
}

fn ensure_contained(
    path: &Path,
    package_root: &Path,
    reference: &Path,
    base: &Path,
) -> Result<(), JsonFormatError> {
    if path.starts_with(package_root) {
        return Ok(());
    }
    Err(JsonFormatError::SchemaResource {
        reference: reference.display().to_string(),
        base: base.to_path_buf(),
        reason: format!(
            "canonical resource `{}` escapes package root `{}`",
            path.display(),
            package_root.display()
        ),
    })
}

fn has_uri_scheme(path: &str) -> bool {
    let first_separator = path.find(['/', '\\']).unwrap_or(path.len());
    path[..first_separator].contains(':')
}

fn decode_uri_component(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = *bytes.get(index + 1)?;
            let low = *bytes.get(index + 2)?;
            decoded.push(hex_value(high)? << 4 | hex_value(low)?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
