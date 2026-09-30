//! Closed, observed IDoc text-settings grammar. Unknown native settings never
//! become a provenance certificate; executable legacy imports remain intact.

use mapping::{
    IdocNativeTextSettings, IdocNativeValidationAction, IdocNativeValidationCase,
    IdocNativeValidationKind,
};
use roxmltree::Node;

const TEXT_BASE_ATTRIBUTES: &[&str] = &[
    "type",
    "kind",
    "config",
    "inputinstance",
    "outputinstance",
    "ferrule-unresolved-config",
    "ferrule-missing-config",
];
const TEXT_ATTRIBUTES: &[&str] = &[
    "type",
    "kind",
    "config",
    "inputinstance",
    "outputinstance",
    "encoding",
    "byteorder",
    "byteordermark",
];
const SETTINGS_ATTRIBUTES: &[&str] = &[
    "unpackedformat",
    "autocompletedata",
    "terminatewithlinefeed",
    "syntaxversionnumber",
    "controllingagency",
    "syntaxlevel",
    "isidoc",
];
const SEPARATOR_ATTRIBUTES: &[&str] = &[
    "dataelement",
    "component",
    "decimal",
    "escape",
    "repetition",
    "segment",
    "subcomponent",
];

pub(super) fn read(
    text: &Node<'_, '_>,
    has_certified_descriptor: bool,
    component_name: &str,
    warnings: &mut Vec<String>,
) -> Option<IdocNativeTextSettings> {
    if !has_native_settings_footprint(text) {
        return None;
    }
    if !has_certified_descriptor {
        warnings.push(format!(
            "EDI component `{component_name}` has native IDoc text settings without a certified configuration descriptor; the settings certificate was ignored"
        ));
        return None;
    }
    match parse(text) {
        Ok(settings) => Some(settings),
        Err(reason) => {
            warnings.push(format!(
                "EDI component `{component_name}` has unsupported native IDoc text settings ({reason}); the settings certificate was ignored"
            ));
            None
        }
    }
}

fn has_native_settings_footprint(text: &Node<'_, '_>) -> bool {
    if text.attributes().any(|attribute| {
        attribute.namespace().is_some() || !TEXT_BASE_ATTRIBUTES.contains(&attribute.name())
    }) {
        return true;
    }
    let mut settings_nodes = text
        .children()
        .filter(|child| child.has_tag_name("settings"));
    let first = settings_nodes.next();
    settings_nodes.next().is_some()
        || first.is_some_and(|settings| {
            settings.attributes().any(|attribute| {
                attribute.namespace().is_some() || attribute.name() != "autocompletedata"
            }) || settings.children().any(|child| child.is_element())
        })
}

fn parse(text: &Node<'_, '_>) -> Result<IdocNativeTextSettings, String> {
    if text.attributes().any(|attribute| {
        attribute.namespace().is_some() || !TEXT_ATTRIBUTES.contains(&attribute.name())
    }) {
        return Err("unknown IDoc text attribute".into());
    }
    for (name, value) in [
        ("encoding", "1"),
        ("byteorder", "1"),
        ("byteordermark", "0"),
    ] {
        if text.attribute(name) != Some(value) {
            return Err(format!("unsupported or missing `{name}` code"));
        }
    }
    let text_children = child_elements(text, 3)?;
    if text_children.iter().any(|child| {
        !matches!(
            child.tag_name().name(),
            "settings" | "ferrule-layout" | "ferrule-idoc-native-config"
        )
    }) {
        return Err("unknown IDoc text child".into());
    }
    let mut settings_nodes = text_children
        .iter()
        .filter(|child| child.has_tag_name("settings"));
    let settings = settings_nodes
        .next()
        .ok_or_else(|| "missing IDoc settings".to_string())?;
    if settings_nodes.next().is_some() {
        return Err("duplicate IDoc settings".into());
    }
    exact_attributes(settings, SETTINGS_ATTRIBUTES)?;
    for (name, value) in [
        ("syntaxversionnumber", "2"),
        ("controllingagency", "Fixed"),
        ("syntaxlevel", "A"),
        ("isidoc", "true"),
    ] {
        if settings.attribute(name) != Some(value) {
            return Err(format!("unsupported `{name}` value"));
        }
    }
    let unpacked_format = parse_bool(settings.attribute("unpackedformat"))?;
    let autocomplete_data = parse_bool(settings.attribute("autocompletedata"))?;
    let terminate_with_line_feed = parse_bool(settings.attribute("terminatewithlinefeed"))?;
    let settings_children = child_elements(settings, 2)?;
    if settings_children.len() != 2
        || !settings_children[0].has_tag_name("separators")
        || !settings_children[1].has_tag_name("validation")
    {
        return Err(
            "IDoc settings require one separators block followed by one validation block".into(),
        );
    }
    parse_separators(&settings_children[0])?;
    let validation_cases = parse_validation(&settings_children[1])?;
    IdocNativeTextSettings::new_observed_profile(
        unpacked_format,
        autocomplete_data,
        terminate_with_line_feed,
        validation_cases,
    )
    .map_err(|error| error.to_string())
}

fn parse_bool(value: Option<&str>) -> Result<bool, String> {
    match value {
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        _ => Err("IDoc setting requires a true or false value".into()),
    }
}

fn parse_separators(node: &Node<'_, '_>) -> Result<(), String> {
    exact_attributes(node, SEPARATOR_ATTRIBUTES)?;
    for (name, value) in [
        ("dataelement", "%20"),
        ("component", "%20"),
        ("decimal", "."),
        ("escape", "%20"),
        ("repetition", "%20"),
        ("segment", "%0A"),
        ("subcomponent", ""),
    ] {
        if node.attribute(name) != Some(value) {
            return Err(format!("unsupported `{name}` separator"));
        }
    }
    child_elements(node, 0)?;
    Ok(())
}

fn parse_validation(node: &Node<'_, '_>) -> Result<Vec<IdocNativeValidationCase>, String> {
    exact_attributes(node, &[])?;
    let children = child_elements(node, mapping::IDOC_NATIVE_VALIDATION_CASES)?;
    if children.len() != mapping::IDOC_NATIVE_VALIDATION_CASES {
        return Err("IDoc validation requires exactly 16 cases".into());
    }
    children
        .into_iter()
        .map(|child| {
            if !child.has_tag_name("case") {
                return Err("unknown IDoc validation child".into());
            }
            exact_attributes(&child, &["kind", "action"])?;
            child_elements(&child, 0)?;
            let kind = child
                .attribute("kind")
                .and_then(IdocNativeValidationKind::from_native_name)
                .ok_or_else(|| "unknown IDoc validation kind".to_string())?;
            let action = child
                .attribute("action")
                .and_then(IdocNativeValidationAction::from_native_name)
                .ok_or_else(|| "unknown IDoc validation action".to_string())?;
            Ok(IdocNativeValidationCase::new(kind, action))
        })
        .collect()
}

fn exact_attributes(node: &Node<'_, '_>, names: &[&str]) -> Result<(), String> {
    if node.attributes().len() != names.len()
        || node
            .attributes()
            .any(|attribute| attribute.namespace().is_some() || !names.contains(&attribute.name()))
    {
        return Err(format!(
            "unexpected or missing `{}` attribute",
            node.tag_name().name()
        ));
    }
    Ok(())
}

fn child_elements<'a, 'input>(
    node: &Node<'a, 'input>,
    max_elements: usize,
) -> Result<Vec<Node<'a, 'input>>, String> {
    let mut elements = Vec::with_capacity(max_elements);
    for child in node.children() {
        if child.is_element() {
            if child.tag_name().namespace().is_some() {
                return Err(format!(
                    "unexpected content inside `{}`",
                    node.tag_name().name()
                ));
            }
            if elements.len() >= max_elements {
                return Err(format!(
                    "too many child elements inside `{}`",
                    node.tag_name().name()
                ));
            }
            elements.push(child);
        } else if !(child.is_text() && child.text().is_some_and(|text| text.trim().is_empty())) {
            return Err(format!(
                "unexpected content inside `{}`",
                node.tag_name().name()
            ));
        }
    }
    Ok(elements)
}
