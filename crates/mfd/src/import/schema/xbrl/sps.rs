use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use ir::{SchemaKind, SchemaNode, XML_TEXT_FIELD};
use mapping::{XbrlFactBinding, XbrlFactType, XbrlNamespaceBinding};

use super::super::schema_node_at;

const MAX_SPS_BYTES: u64 = 32 * 1024 * 1024;

pub(super) fn fact_bindings(
    mfd_path: &Path,
    declared: &str,
    schema: &SchemaNode,
    namespaces: &[XbrlNamespaceBinding],
) -> Result<Vec<XbrlFactBinding>, String> {
    let path = resolve_sibling(mfd_path, declared)?;
    let metadata = std::fs::metadata(&path)
        .map_err(|error| format!("could not read `{}` ({error})", path.display()))?;
    if metadata.len() > MAX_SPS_BYTES {
        return Err(format!(
            "presentation `{}` exceeds the {} MiB size limit",
            path.display(),
            MAX_SPS_BYTES / (1024 * 1024)
        ));
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("could not read `{}` ({error})", path.display()))?;
    let document = roxmltree::Document::parse(&text)
        .map_err(|error| format!("could not parse `{}` ({error})", path.display()))?;
    let types = concept_types(&document)?;

    namespaces
        .iter()
        .filter(|binding| is_fact_path(schema, binding.path()))
        .filter_map(|binding| {
            let local = binding.path().last()?;
            types
                .get(&(binding.namespace(), local.as_str()))
                .copied()
                .map(|fact_type| XbrlFactBinding::new(binding.path().to_vec(), fact_type))
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("invalid XBRL fact binding ({error})"))
}

fn resolve_sibling(mfd_path: &Path, declared: &str) -> Result<PathBuf, String> {
    let portable = declared.replace('\\', "/");
    let relative = Path::new(&portable);
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err(format!(
            "presentation path `{declared}` is not a bounded relative path"
        ));
    }
    Ok(mfd_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(relative))
}

fn concept_types<'a>(
    document: &'a roxmltree::Document<'a>,
) -> Result<BTreeMap<(&'a str, &'a str), XbrlFactType>, String> {
    let namespace_pairs = document
        .descendants()
        .filter(|node| node.has_tag_name("nspair"))
        .filter_map(|node| Some((node.attribute("prefix")?, node.attribute("uri")?)))
        .collect::<BTreeMap<_, _>>();
    let mut result = BTreeMap::new();
    for template in document.descendants().filter(|node| {
        node.has_tag_name("template") && node.attribute("subtype") == Some("xbrl-concept-aspect")
    }) {
        let Some(qname) = template.attribute("match") else {
            continue;
        };
        let Some((prefix, local)) = qname.split_once(':') else {
            continue;
        };
        let Some(namespace) = template
            .lookup_namespace_uri(Some(prefix))
            .or_else(|| namespace_pairs.get(prefix).copied())
        else {
            continue;
        };
        let Some(fact_type) = template.descendants().find_map(|node| {
            (node.has_tag_name("calltemplate") && node.attribute("subtype") == Some("named"))
                .then(|| item_type(node.attribute("match")?))
                .flatten()
        }) else {
            continue;
        };
        if let Some(existing) = result.insert((namespace, local), fact_type)
            && existing != fact_type
        {
            return Err(format!(
                "presentation concept `{{{namespace}}}{local}` has conflicting numeric item types"
            ));
        }
    }
    Ok(result)
}

fn item_type(name: &str) -> Option<XbrlFactType> {
    match name {
        "monetaryItemType" | "monetaryItemTypeNegative" => Some(XbrlFactType::Monetary),
        "numericItemType" => Some(XbrlFactType::Numeric),
        "sharesItemType" => Some(XbrlFactType::Shares),
        name if name.starts_with("perShareItemType") => Some(XbrlFactType::PerShare),
        _ => None,
    }
}

fn is_fact_path(schema: &SchemaNode, path: &[String]) -> bool {
    let Some(node) = schema_node_at(schema, path) else {
        return false;
    };
    match &node.kind {
        SchemaKind::Scalar { .. } => !node.attribute && !node.text,
        SchemaKind::Group { .. } => node.child(XML_TEXT_FIELD).is_some(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use ir::ScalarType;

    use super::*;

    static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn compiles_expanded_concept_item_types_for_connected_fact_paths()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = std::env::temp_dir().join(format!(
            "ferrule_xbrl_sps_{}_{}",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory)?;
        let mfd_path = directory.join("mapping.mfd");
        let sps_path = directory.join("report.sps");
        std::fs::write(
            &sps_path,
            r#"<structure><schemasources><namespaces><nspair prefix="ex" uri="urn:example"/></namespaces></schemasources><template subtype="xbrl-concept-aspect" match="ex:Amount"><children><calltemplate subtype="named" match="monetaryItemType"/></children></template><template subtype="xbrl-concept-aspect" match="ex:Ratio"><children><calltemplate subtype="named" match="perShareItemType2"/></children></template></structure>"#,
        )?;
        let schema = SchemaNode::group(
            "xbrl",
            vec![SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Amount", ScalarType::Float),
                    SchemaNode::scalar("Ratio", ScalarType::Float),
                ],
            )],
        );
        let namespaces = vec![
            XbrlNamespaceBinding::new(
                vec!["Rows".to_string(), "Amount".to_string()],
                "urn:example",
            )?,
            XbrlNamespaceBinding::new(
                vec!["Rows".to_string(), "Ratio".to_string()],
                "urn:example",
            )?,
        ];
        let bindings = fact_bindings(&mfd_path, "report.sps", &schema, &namespaces)?;
        assert_eq!(bindings.len(), 2);
        assert!(bindings.iter().any(|binding| {
            binding.path().last().map(String::as_str) == Some("Amount")
                && binding.fact_type() == XbrlFactType::Monetary
        }));
        assert!(bindings.iter().any(|binding| {
            binding.path().last().map(String::as_str) == Some("Ratio")
                && binding.fact_type() == XbrlFactType::PerShare
        }));
        std::fs::remove_dir_all(directory)?;
        Ok(())
    }
}
