use ir::SchemaNode;
use mapping::FormatOptions;

fn eligibility(
    schema: &SchemaNode,
    path: Option<&str>,
    options: &FormatOptions,
    observed: bool,
) -> Result<(), &'static str> {
    if path.is_some_and(|path| {
        path.split_once("://").is_some_and(|(scheme, _)| {
            scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
        })
    }) {
        return Err("These root policies require a local XML input.");
    }
    let mut ordinary = options.clone();
    ordinary.xml_document = false;
    ordinary.xml_allow_inactive_root_type_members = false;
    ordinary.xml_root_view_read_policy = false;
    if ordinary != FormatOptions::default() {
        return Err("These root policies cannot be combined with other format settings.");
    }
    if !ir::xml_inactive_root_type_members_are_supported(schema) {
        return Err("The source must be a closed, flat XML root with declared type alternatives.");
    }
    if observed && !ir::xml_root_view_read_policy_is_supported(schema) {
        return Err(
            "Observed root annotations require ordinary string attributes without fixed values.",
        );
    }
    Ok(())
}

fn set_inactive(
    schema: &SchemaNode,
    path: Option<&str>,
    options: &mut FormatOptions,
    enabled: bool,
) -> Result<(), &'static str> {
    if enabled {
        eligibility(schema, path, options, false)?;
        options.xml_document = true;
    } else {
        options.xml_root_view_read_policy = false;
    }
    options.xml_allow_inactive_root_type_members = enabled;
    Ok(())
}

fn set_observed(
    schema: &SchemaNode,
    path: Option<&str>,
    options: &mut FormatOptions,
    enabled: bool,
) -> Result<(), &'static str> {
    if enabled {
        eligibility(schema, path, options, true)?;
        options.xml_document = true;
        options.xml_allow_inactive_root_type_members = true;
    }
    options.xml_root_view_read_policy = enabled;
    Ok(())
}

pub(super) fn show(
    ui: &mut egui::Ui,
    schema: &SchemaNode,
    path: Option<&str>,
    options: &mut FormatOptions,
    editing_enabled: bool,
) {
    if !schema.xml_type_alternatives
        && !options.xml_allow_inactive_root_type_members
        && !options.xml_root_view_read_policy
    {
        return;
    }
    egui::CollapsingHeader::new("Primary XML root input")
        .id_salt("primary_xml_root_input")
        .show(ui, |ui| {
            let inactive_problem = eligibility(schema, path, options, false).err();
            let observed_problem = eligibility(schema, path, options, true).err();
            let mut inactive = options.xml_allow_inactive_root_type_members;
            let mut observed = options.xml_root_view_read_policy;
            if ui.add_enabled(
                editing_enabled && (inactive || inactive_problem.is_none()),
                egui::Checkbox::new(&mut inactive, "Read declared fields outside the selected type"),
            ).changed() {
                let _ = set_inactive(schema, path, options, inactive);
                observed = options.xml_root_view_read_policy;
            }
            if ui.add_enabled(
                editing_enabled && (observed || observed_problem.is_none()),
                egui::Checkbox::new(&mut observed, "Retain observed root annotations"),
            ).changed() {
                let _ = set_observed(schema, path, options, observed);
            }
            ui.weak("A required field is checked when its connected expression reads it.");
            if let Some(reason) = inactive_problem.or(observed_problem) {
                ui.colored_label(ui.visuals().warn_fg_color, reason);
            }
            if (options.xml_allow_inactive_root_type_members || options.xml_root_view_read_policy)
                && (!options.xml_document || !options.xml_allow_inactive_root_type_members)
            {
                ui.colored_label(ui.visuals().warn_fg_color,
                    "The saved root policy is missing its XML or declared-field prerequisite. Disable it or select it again to repair the setting.");
            }
        });
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use ir::{GroupAlternative, ScalarType, XmlNamespace};

    pub(crate) fn schema() -> SchemaNode {
        let attribute = |name: &str| {
            let mut scalar = SchemaNode::scalar(name, ScalarType::String).attribute();
            scalar.xml_namespace = Some(XmlNamespace::Unqualified);
            scalar
        };
        let mut schema = SchemaNode::group("Root", vec![attribute("Code"), attribute("Extra")])
            .with_alternatives(vec![
                GroupAlternative {
                    name: "Base".into(),
                    members: vec!["Code".into()],
                    required: vec![],
                    constraints: vec![],
                },
                GroupAlternative {
                    name: "{urn:root}Derived".into(),
                    members: vec!["Code".into(), "Extra".into()],
                    required: vec![],
                    constraints: vec![],
                },
            ])
            .unwrap();
        schema.xml_namespace = Some(XmlNamespace::Unqualified);
        schema.xml_type_alternatives = true;
        schema.xml_default_type = Some("Base".into());
        schema
    }

    #[test]
    fn policy_dependencies_are_atomic_and_serialization_defaults_remain_omitted() {
        let schema = schema();
        let mut options = FormatOptions::default();
        let defaults = serde_json::to_value(&options).unwrap();
        assert!(defaults.get("xml_root_view_read_policy").is_none());
        set_observed(&schema, None, &mut options, true).unwrap();
        assert!(
            options.xml_document
                && options.xml_allow_inactive_root_type_members
                && options.xml_root_view_read_policy
        );
        let serialized = serde_json::to_vec(&options).unwrap();
        assert_eq!(
            serde_json::from_slice::<FormatOptions>(&serialized).unwrap(),
            options
        );
        set_inactive(&schema, None, &mut options, false).unwrap();
        assert!(options.xml_document);
        assert!(
            !options.xml_allow_inactive_root_type_members && !options.xml_root_view_read_policy
        );
    }

    #[test]
    fn incompatible_policy_actions_preserve_all_loaded_options() {
        let schema = schema();
        for (path, options) in [
            (
                Some("HTTPS://example.test/input.xml"),
                FormatOptions::default(),
            ),
            (
                None,
                FormatOptions {
                    json_document: true,
                    ..Default::default()
                },
            ),
            (
                None,
                FormatOptions {
                    xml_document: true,
                    local_xml_file_set: true,
                    xml_root_view_read_policy: true,
                    ..Default::default()
                },
            ),
        ] {
            let mut changed = options.clone();
            assert!(set_observed(&schema, path, &mut changed, true).is_err());
            assert_eq!(changed, options);
            set_observed(&schema, path, &mut changed, false).unwrap();
            let mut expected = options;
            expected.xml_root_view_read_policy = false;
            assert_eq!(changed, expected);
        }
    }

    #[test]
    fn non_string_root_can_use_only_the_older_member_policy() {
        let mut schema = schema();
        let ir::SchemaKind::Group { children, .. } = &mut schema.kind else {
            unreachable!()
        };
        children[0].kind = ir::SchemaKind::Scalar {
            ty: ScalarType::Int,
        };
        let mut options = FormatOptions::default();
        set_inactive(&schema, Some("input.xml"), &mut options, true).unwrap();
        let before = options.clone();
        assert!(set_observed(&schema, Some("input.xml"), &mut options, true).is_err());
        assert_eq!(options, before);
    }
}
