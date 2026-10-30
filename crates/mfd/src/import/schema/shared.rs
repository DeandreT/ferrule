use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ir::SchemaNode;

use crate::resource::ResourceResolver;

pub(super) fn read_json_schema_resource(
    mfd_path: &Path,
    resources: Option<&ResourceResolver>,
    reference: &str,
    description: &str,
) -> Result<SchemaNode, String> {
    match resources {
        Some(resources) => resources.import_json_schema(reference),
        None => {
            let path = super::resolve_resource_reference(mfd_path, None, reference, description)?;
            format_json::json_schema::import(&path).map_err(|error| error.to_string())
        }
    }
}

/// Carries request-specific authority; dispatching only by resolved pathname
/// would accidentally grant an unbound alias another request's trusted root.
#[derive(Clone)]
pub(in crate::import) struct XmlSchemaResource {
    path: PathBuf,
    bound: Option<std::rc::Rc<crate::resource::BoundResource>>,
    package: Option<std::rc::Rc<crate::package::PackageReader>>,
}

impl std::ops::Deref for XmlSchemaResource {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.path
    }
}

impl XmlSchemaResource {
    pub(in crate::import) fn unbound(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            bound: None,
            package: None,
        }
    }
    pub(in crate::import) fn is_bound(&self) -> bool {
        self.bound.is_some() || self.package.is_some()
    }

    pub(in crate::import) fn witness_bytes(&self) -> Option<std::sync::Arc<[u8]>> {
        if let Some(package) = &self.package {
            return package.bytes(&self.path).ok();
        }
        self.bound
            .as_ref()
            .and_then(|bound| bound.bytes(bound.path()).ok())
    }

    pub(in crate::import) fn import_root(
        &self,
        root: Option<&str>,
        resources: Option<&ResourceResolver>,
    ) -> Result<SchemaNode, format_xml::XmlFormatError> {
        let result = if let Some(package) = &self.package {
            format_xml::xsd::import_root_with_byte_reader(
                &self.path,
                root,
                crate::package::virtual_root(),
                package.clone(),
            )
        } else if let Some(bound) = &self.bound {
            bound.finish(format_xml::xsd::import_root_with_byte_reader(
                &self.path,
                root,
                bound.root(),
                bound.clone(),
            ))
        } else {
            match resources {
                Some(r) => format_xml::xsd::import_root_with_resource_root(
                    &self.path,
                    root,
                    r.package_root(),
                ),
                None => format_xml::xsd::import_root(&self.path, root),
            }
        };
        retain_xsd_resource_refusal(resources, &result);
        result.and_then(refuse_wildcard_exclusions)
    }

    pub(in crate::import) fn import_type(
        &self,
        name: &str,
        resources: Option<&ResourceResolver>,
    ) -> Result<SchemaNode, format_xml::XmlFormatError> {
        let result = if let Some(package) = &self.package {
            format_xml::xsd::import_type_with_byte_reader(
                &self.path,
                name,
                crate::package::virtual_root(),
                package.clone(),
            )
        } else if let Some(bound) = &self.bound {
            bound.finish(format_xml::xsd::import_type_with_byte_reader(
                &self.path,
                name,
                bound.root(),
                bound.clone(),
            ))
        } else {
            match resources {
                Some(r) => format_xml::xsd::import_type_with_resource_root(
                    &self.path,
                    name,
                    r.package_root(),
                ),
                None => format_xml::xsd::import_type(&self.path, name),
            }
        };
        retain_xsd_resource_refusal(resources, &result);
        result.and_then(refuse_wildcard_exclusions)
    }

    pub(in crate::import) fn import_type_base(
        &self,
        name: &str,
        resources: Option<&ResourceResolver>,
    ) -> Result<Option<String>, format_xml::XmlFormatError> {
        let result = if let Some(package) = &self.package {
            format_xml::xsd::import_type_base_with_byte_reader(
                &self.path,
                name,
                crate::package::virtual_root(),
                package.clone(),
            )
        } else if let Some(bound) = &self.bound {
            bound.finish(format_xml::xsd::import_type_base_with_byte_reader(
                &self.path,
                name,
                bound.root(),
                bound.clone(),
            ))
        } else {
            match resources {
                Some(r) => format_xml::xsd::import_type_base_with_resource_root(
                    &self.path,
                    name,
                    r.package_root(),
                ),
                None => format_xml::xsd::import_type_base(&self.path, name),
            }
        };
        retain_xsd_resource_refusal(resources, &result);
        result
    }
}

pub(in crate::import) fn resolve_xml_resource(
    mfd_path: &Path,
    resources: Option<&ResourceResolver>,
    reference: &str,
    description: &str,
) -> Result<XmlSchemaResource, String> {
    if let Some(r) = resources {
        if let Some(package) = r.package() {
            return Ok(XmlSchemaResource {
                path: r.resolve_package_file(reference)?,
                bound: None,
                package: Some(package.clone()),
            });
        }
        if let Some(bound) =
            r.bound_resource(crate::resource::ResourceBindingKindV1::XmlSchema, reference)?
        {
            return Ok(XmlSchemaResource {
                path: bound.path().to_path_buf(),
                bound: Some(bound),
                package: None,
            });
        }
        return r
            .resolve_file_retaining_boundary_refusals(reference, description)
            .map(|path| XmlSchemaResource::unbound(&path));
    }
    resolve_xml_schema_reference(mfd_path, reference).map(|path| XmlSchemaResource::unbound(&path))
}

#[derive(Debug)]
pub(in crate::import) enum XmlSchemaReadError {
    Xsd(format_xml::XmlFormatError),
    Other(String),
}

impl std::fmt::Display for XmlSchemaReadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Xsd(error) => error.fmt(formatter),
            Self::Other(error) => formatter.write_str(error),
        }
    }
}

pub(in crate::import) fn resolve_xml_schema_reference(
    mfd_path: &Path,
    relative: &str,
) -> Result<PathBuf, String> {
    let portable = relative.replace('\\', "/");
    let base = mfd_path.parent().unwrap_or_else(|| Path::new("."));
    let direct = base.join(portable);
    if direct.is_file() {
        return Ok(direct);
    }

    let file_name = direct
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("schema path `{relative}` has no file name"))?;
    let directory = direct.parent().unwrap_or(base);
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("could not resolve schema `{relative}` ({error})"))?;
    let mut matched = None;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("could not resolve schema `{relative}` ({error})"))?;
        if !entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.eq_ignore_ascii_case(file_name))
        {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|error| format!("could not inspect schema `{relative}` ({error})"))?;
        if !file_type.is_file() {
            continue;
        }
        if matched.is_some() {
            return Err(format!(
                "schema `{relative}` has multiple case-insensitive sibling matches"
            ));
        }
        matched = Some(entry.path());
    }
    matched.ok_or_else(|| format!("schema `{relative}` was not found"))
}

pub(in crate::import) fn read_xml_schema_file(
    schema_path: &XmlSchemaResource,
    root: Option<&str>,
    resources: Option<&ResourceResolver>,
) -> Result<SchemaNode, XmlSchemaReadError> {
    if let Some(package) = &schema_path.package
        && schema_path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("dtd"))
    {
        let bytes = package
            .bytes(schema_path)
            .map_err(|error| XmlSchemaReadError::Other(error.to_string()))?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|error| XmlSchemaReadError::Other(error.to_string()))?;
        return format_xml::dtd::import_root_str(text, root)
            .map_err(|error| XmlSchemaReadError::Other(error.to_string()));
    }
    if schema_path.is_bound() {
        return schema_path
            .import_root(root, resources)
            .map_err(XmlSchemaReadError::Xsd);
    }
    let extension = schema_path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    if extension.eq_ignore_ascii_case("dtd") {
        format_xml::dtd::import_root(schema_path, root)
            .map_err(|error| XmlSchemaReadError::Other(error.to_string()))
    } else {
        let result = schema_path.import_root(root, resources);
        retain_xsd_resource_refusal(resources, &result);
        result.map_err(XmlSchemaReadError::Xsd)
    }
}

/// MFD components cannot carry XSD 1.1 wildcard exclusions yet, so a schema
/// that uses them fails like any other unsupported wildcard and the caller's
/// usual fallback applies.
fn refuse_wildcard_exclusions(
    schema: SchemaNode,
) -> Result<SchemaNode, format_xml::XmlFormatError> {
    fn first_exclusion(node: &SchemaNode) -> Option<&SchemaNode> {
        if node.has_xml_wildcard_exclusions() {
            return Some(node);
        }
        let ir::SchemaKind::Group {
            children, dynamic, ..
        } = &node.kind
        else {
            return None;
        };
        children
            .iter()
            .find_map(first_exclusion)
            .or_else(|| dynamic.as_deref().and_then(first_exclusion))
    }
    const REASON: &str = "XSD 1.1 wildcard exclusions are not supported by MFD import";
    match first_exclusion(&schema) {
        None => Ok(schema),
        Some(node) if node.name == ir::XML_ATTRIBUTES_FIELD => {
            Err(format_xml::XmlFormatError::UnsupportedXmlAttributeWildcard { reason: REASON })
        }
        Some(_) => Err(format_xml::XmlFormatError::UnsupportedXmlWildcard { reason: REASON }),
    }
}

pub(in crate::import) fn retain_xsd_resource_refusal<T>(
    resources: Option<&ResourceResolver>,
    result: &Result<T, format_xml::XmlFormatError>,
) {
    if let Some(resources) = resources
        && let Err(error @ format_xml::XmlFormatError::Io(cause)) = result
        && cause.kind() == std::io::ErrorKind::PermissionDenied
    {
        resources.record_refusal(error.to_string());
    }
}

pub(in crate::import) fn read_xsd_metadata_text(
    resources: &ResourceResolver,
    declared: &str,
    description: &str,
    max_bytes: u64,
) -> Result<Option<String>, String> {
    let resolved = resolve_xml_resource(
        resources.mapping_path(),
        Some(resources),
        declared,
        description,
    )?;
    if resolved.package.is_some()
        && resolved
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("dtd"))
    {
        return Ok(None);
    }
    if let Some(package) = &resolved.package {
        let bytes = package
            .bytes(&resolved)
            .map_err(|error| error.to_string())?;
        return format_xml::xsd::decode_schema_bytes(&bytes, max_bytes)
            .map(Some)
            .map_err(|error| error.to_string());
    }
    if let Some(bound) = &resolved.bound {
        let result = (|| {
            let bytes = bound.bytes(bound.path()).map_err(|e| e.to_string())?;
            let text = format_xml::xsd::decode_schema_bytes(&bytes, max_bytes)
                .map_err(|e| e.to_string())?;
            let doc = roxmltree::Document::parse(&text).map_err(|e| e.to_string())?;
            if !doc
                .root_element()
                .has_tag_name(("http://www.w3.org/2001/XMLSchema", "schema"))
            {
                return Err("bound XML metadata resource is not an XSD document".to_string());
            }
            drop(doc);
            Ok(Some(text))
        })();
        return bound.finish(result);
    }
    if resolved
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("dtd"))
    {
        return Ok(None);
    }
    format_xml::xsd::read_text(&resolved, max_bytes)
        .map(Some)
        .map_err(|error| {
            format!(
                "could not read {description} `{declared}` at `{}` ({error})",
                resolved.display()
            )
        })
}

pub(in crate::import) fn parse_u32(attr: Option<&str>) -> Option<u32> {
    attr.and_then(|attribute| attribute.parse().ok())
}

pub(in crate::import) fn entry_key_sets(root: &roxmltree::Node) -> (BTreeSet<u32>, BTreeSet<u32>) {
    let mut inputs = BTreeSet::new();
    let mut outputs = BTreeSet::new();
    for entry in root.descendants().filter(|node| node.has_tag_name("entry")) {
        if let Some(key) = parse_u32(entry.attribute("inpkey")) {
            inputs.insert(key);
        }
        if let Some(key) = parse_u32(entry.attribute("outkey")) {
            outputs.insert(key);
        }
    }
    (inputs, outputs)
}

pub(in crate::import) fn is_default_output(component: &roxmltree::Node) -> bool {
    component
        .children()
        .find(|node| node.has_tag_name("properties"))
        .and_then(|properties| properties.attribute("XSLTDefaultOutput"))
        == Some("1")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "ferrule_mfd_schema_reference_{}_{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn exact_schema_reference_takes_precedence() {
        let dir = TempDir::new();
        let exact = dir.0.join("input.xsd");
        std::fs::write(&exact, "exact").unwrap();
        let fallback = dir.0.join("INPUT.XSD");
        if !fallback.exists() {
            // Distinct case-only siblings require a case-sensitive filesystem.
            std::fs::write(fallback, "fallback").unwrap();
        }

        let resolved =
            resolve_xml_schema_reference(&dir.0.join("mapping.mfd"), "input.xsd").unwrap();
        assert_eq!(resolved, exact);
        assert_eq!(std::fs::read_to_string(resolved).unwrap(), "exact");
    }

    #[test]
    fn schema_reference_fallback_is_unique_and_stays_in_its_parent() {
        let dir = TempDir::new();
        let schemas = dir.0.join("schemas");
        std::fs::create_dir(&schemas).unwrap();
        let matched = schemas.join("Input.xsd");
        std::fs::write(&matched, "schema").unwrap();
        std::fs::write(dir.0.join("INPUT.XSD"), "wrong parent").unwrap();

        let resolved =
            resolve_xml_schema_reference(&dir.0.join("mapping.mfd"), "schemas\\input.xsd").unwrap();
        if schemas.join("input.xsd").is_file() {
            assert_eq!(resolved, schemas.join("input.xsd"));
        } else {
            assert_eq!(resolved, matched);
        }
        assert_eq!(
            std::fs::canonicalize(resolved).unwrap(),
            std::fs::canonicalize(matched).unwrap()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn ambiguous_schema_references_are_rejected() {
        let dir = TempDir::new();
        std::fs::write(dir.0.join("Input.xsd"), "first").unwrap();
        std::fs::write(dir.0.join("INPUT.XSD"), "second").unwrap();

        let ambiguous =
            resolve_xml_schema_reference(&dir.0.join("mapping.mfd"), "input.xsd").unwrap_err();
        assert!(ambiguous.contains("multiple case-insensitive sibling matches"));
    }

    #[test]
    fn missing_schema_references_are_rejected() {
        let dir = TempDir::new();
        let missing =
            resolve_xml_schema_reference(&dir.0.join("mapping.mfd"), "missing.xsd").unwrap_err();
        assert!(missing.contains("was not found"));
    }
}
