use mapping::{
    PdfCommand, PdfMetricMatch, PdfTextCase, PdfTextGroup, PdfTextGroupOutput, PdfTextGroups,
    PdfTextMatch, PdfTextProperties, PdfTextRows,
};

use super::{
    ParseContext, child, current_region, parse_commands, parse_f64, parse_minimum_extent,
    parse_optional_region, parse_points, require_one_group_per_page, required_text_child,
};

pub(super) fn parse_text_find_splitter(
    node: &roxmltree::Node<'_, '_>,
    text_find: &roxmltree::Node<'_, '_>,
    context: &mut ParseContext,
) -> Result<Vec<PdfCommand>, String> {
    validate_splitter_controls(node)?;
    let coordinate = child(text_find, "Coordinate")
        .ok_or_else(|| "PDF TextFind has no Coordinate mode".to_string())?;
    require_only_choice(&coordinate, "cell-minimum", "PDF TextFind coordinate")?;
    require_empty_child(text_find, "Displace", "PDF TextFind displacement")?;
    let matcher_node =
        child(text_find, "Match").ok_or_else(|| "PDF TextFind has no Match block".to_string())?;
    let matcher = parse_text_match(&matcher_node, context)?;
    let children = child(node, "Children")
        .ok_or_else(|| "PDF TextFind Splitter has no Children block".to_string())?;
    let groups = children
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let [group] = groups.as_slice() else {
        return Err("PDF TextFind must contain exactly one output grouping".to_string());
    };
    require_one_group_per_page(group)?;
    require_empty_filter(group, "PDF TextFind output grouping")?;
    if parse_optional_region(group)? != current_region() {
        return Err("PDF TextFind output grouping cannot narrow its candidate region".to_string());
    }
    let name = required_text_child(group, "Label")?;
    let group_children = child(group, "Children")
        .ok_or_else(|| "PDF TextFind output grouping has no Children block".to_string())?;
    let children = parse_commands(&group_children, false, context)?;
    Ok(vec![PdfCommand::TextGroups(PdfTextGroups {
        region: parse_optional_region(node)?,
        groups: vec![PdfTextGroup {
            output: PdfTextGroupOutput::Repeated { name },
            matcher,
            children,
        }],
    })])
}

pub(super) fn parse_object_find_splitter(
    node: &roxmltree::Node<'_, '_>,
    object_find: &roxmltree::Node<'_, '_>,
    context: &mut ParseContext,
) -> Result<Vec<PdfCommand>, String> {
    validate_splitter_controls(node)?;
    let minimum_extent = validate_object_find(object_find)?;
    let children = child(node, "Children")
        .ok_or_else(|| "PDF ObjectFind Splitter has no Children block".to_string())?;
    let groups = children
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if groups.is_empty() || groups.iter().any(|group| !group.has_tag_name("Grouping")) {
        return Err("PDF ObjectFind supports only direct Grouping children".to_string());
    }
    let group_by_text = groups
        .iter()
        .filter(|group| group_by_text_kind(group).is_some())
        .count();
    if group_by_text == groups.len() {
        let groups = groups
            .iter()
            .map(|group| parse_group_by_text(group, context))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(vec![PdfCommand::TextGroups(PdfTextGroups {
            region: parse_optional_region(node)?,
            groups,
        })])
    } else if group_by_text == 0 {
        for group in &groups {
            require_one_group_per_page(group)?;
            require_empty_filter(group, "PDF ObjectFind row grouping")?;
        }
        Ok(vec![PdfCommand::TextRows(PdfTextRows {
            region: parse_optional_region(node)?,
            minimum_extent: Some(minimum_extent),
            children: parse_commands(&children, false, context)?,
        })])
    } else {
        Err("PDF ObjectFind cannot mix GroupByText and OneGroupPerPage children".to_string())
    }
}

fn parse_group_by_text(
    node: &roxmltree::Node<'_, '_>,
    context: &mut ParseContext,
) -> Result<PdfTextGroup, String> {
    require_empty_filter(node, "PDF GroupByText grouping")?;
    if parse_optional_region(node)? != current_region() {
        return Err("PDF GroupByText grouping cannot narrow its candidate region".to_string());
    }
    let group_by = group_by_text_kind(node)
        .ok_or_else(|| "PDF grouping kind is not GroupByText".to_string())?;
    require_empty_child(&group_by, "Region", "PDF GroupByText region")?;
    let mode = child(&group_by, "Mode").ok_or_else(|| "PDF GroupByText has no Mode".to_string())?;
    require_only_choice(&mode, "filter-containing", "PDF GroupByText mode")?;
    let matcher = child(&group_by, "Match")
        .ok_or_else(|| "PDF GroupByText has no Match block".to_string())?;
    let children = child(node, "Children")
        .ok_or_else(|| "PDF GroupByText grouping has no Children block".to_string())?;
    let children = parse_commands(&children, false, context)?;
    let name = child(node, "Label")
        .and_then(|label| label.text())
        .map(str::trim)
        .unwrap_or_default();
    let output = if name.is_empty() {
        PdfTextGroupOutput::Flatten
    } else {
        PdfTextGroupOutput::Repeated {
            name: name.to_string(),
        }
    };
    Ok(PdfTextGroup {
        output,
        matcher: parse_text_match(&matcher, context)?,
        children,
    })
}

fn group_by_text_kind<'a, 'input>(
    node: &roxmltree::Node<'a, 'input>,
) -> Option<roxmltree::Node<'a, 'input>> {
    child(node, "Kind").and_then(|kind| child(&kind, "GroupByText"))
}

fn parse_text_match(
    node: &roxmltree::Node<'_, '_>,
    _context: &mut ParseContext,
) -> Result<PdfTextMatch, String> {
    for element in node.children().filter(roxmltree::Node::is_element) {
        if !matches!(
            element.tag_name().name(),
            "Search" | "AllowArbitrarySpace" | "WordAnchor" | "CaseFolding" | "TextProperties"
        ) {
            return Err(format!(
                "PDF text match uses unsupported <{}> metadata",
                element.tag_name().name()
            ));
        }
    }
    let needle = required_text_child(node, "Search")?;
    let arbitrary_space = required_text_child(node, "AllowArbitrarySpace")?;
    if arbitrary_space != "1" {
        return Err(format!(
            "PDF text match AllowArbitrarySpace `{arbitrary_space}` is unsupported"
        ));
    }
    let word_anchor =
        child(node, "WordAnchor").ok_or_else(|| "PDF text match has no WordAnchor".to_string())?;
    require_only_choice(&word_anchor, "none", "PDF text match word anchor")?;
    let folding = child(node, "CaseFolding")
        .ok_or_else(|| "PDF text match has no CaseFolding".to_string())?;
    require_only_choice(&folding, "ignore-case", "PDF text match case folding")?;
    let properties = child(node, "TextProperties")
        .map(|properties| parse_text_properties(&properties))
        .transpose()?
        .unwrap_or_default();
    Ok(PdfTextMatch {
        needle,
        case: PdfTextCase::AsciiInsensitive,
        flexible_whitespace: true,
        properties,
    })
}

fn parse_text_properties(node: &roxmltree::Node<'_, '_>) -> Result<PdfTextProperties, String> {
    let mut properties = PdfTextProperties::default();
    for element in node.children().filter(roxmltree::Node::is_element) {
        match element.tag_name().name() {
            "FaceNameMatch" => {
                require_exact_text(&element, "Enable", "1", "PDF face-name match")?;
                properties.font_face = Some(required_text_child(&element, "FontFace")?);
            }
            "Weight" => require_only_choice(&element, "normal", "PDF text weight")?,
            "Style" => require_only_choice(&element, "upright", "PDF text style")?,
            "CellHeightMatch" => {
                require_exact_text(&element, "Enable", "1", "PDF cell-height match")?;
                let height = parse_points(&required_text_child(&element, "CellHeight")?)?;
                let deviation = parse_points(&required_text_child(&element, "Deviation")?)?;
                if height <= 0.0 || deviation < 0.0 {
                    return Err("PDF cell-height match has an invalid extent".to_string());
                }
                properties.cell_height = Some(PdfMetricMatch {
                    value: height,
                    deviation,
                });
            }
            "BaselineMatch" => {
                require_exact_text(&element, "Enable", "1", "PDF baseline match")?;
                let angle = parse_f64(&required_text_child(&element, "BaselineAngle")?)?;
                let deviation = parse_f64(&required_text_child(&element, "AngleDeviation")?)?;
                if deviation < 0.0 {
                    return Err("PDF baseline match has a negative deviation".to_string());
                }
                properties.baseline_angle = Some(PdfMetricMatch {
                    value: angle,
                    deviation,
                });
            }
            other => {
                return Err(format!(
                    "PDF text match uses unsupported <{other}> text properties"
                ));
            }
        }
    }
    Ok(properties)
}

fn validate_object_find(node: &roxmltree::Node<'_, '_>) -> Result<f64, String> {
    require_exact_text(node, "Background", "#fff", "PDF ObjectFind background")?;
    require_exact_text(node, "Tolerance", "10", "PDF ObjectFind tolerance")?;
    require_exact_text(node, "Fill", "0pt", "PDF ObjectFind fill")?;
    require_exact_text(node, "Displace", "0pt", "PDF ObjectFind displacement")?;
    let edge = child(node, "Edge").ok_or_else(|| "PDF ObjectFind has no Edge mode".to_string())?;
    require_only_choice(&edge, "start", "PDF ObjectFind edge")?;
    let minimum_extent = parse_points(&required_text_child(node, "MinimumExtent")?)?;
    if minimum_extent <= 0.0 {
        return Err("PDF ObjectFind minimum extent must be positive".to_string());
    }
    Ok(minimum_extent)
}

fn validate_splitter_controls(node: &roxmltree::Node<'_, '_>) -> Result<(), String> {
    require_empty_child(node, "Search", "PDF Splitter search")?;
    for name in ["SkipInitial", "SkipFinal"] {
        if let Some(value) = child(node, name).and_then(|value| value.text())
            && value.trim() != "0"
        {
            return Err(format!(
                "PDF Splitter {name} `{}` is unsupported",
                value.trim()
            ));
        }
    }
    if parse_minimum_extent(node)?.is_some() {
        return Err(
            "PDF text/object Splitter post-process minimum extent is unsupported".to_string(),
        );
    }
    Ok(())
}

fn require_empty_filter(node: &roxmltree::Node<'_, '_>, description: &str) -> Result<(), String> {
    let filter = child(node, "Filter")
        .and_then(|value| value.text())
        .map(str::trim)
        .unwrap_or_default();
    if matches!(filter, "" | "1") {
        Ok(())
    } else {
        Err(format!("{description} filter `{filter}` is unsupported"))
    }
}

fn require_empty_child(
    node: &roxmltree::Node<'_, '_>,
    name: &str,
    description: &str,
) -> Result<(), String> {
    if child(node, name)
        .and_then(|value| value.text())
        .is_some_and(|value| !value.trim().is_empty())
    {
        Err(format!("{description} must be empty"))
    } else {
        Ok(())
    }
}

fn require_exact_text(
    node: &roxmltree::Node<'_, '_>,
    name: &str,
    expected: &str,
    description: &str,
) -> Result<(), String> {
    let value = required_text_child(node, name)?;
    if value == expected {
        Ok(())
    } else {
        Err(format!("{description} `{value}` is unsupported"))
    }
}

fn require_only_choice(
    node: &roxmltree::Node<'_, '_>,
    expected: &str,
    description: &str,
) -> Result<(), String> {
    let choices = node
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    match choices.as_slice() {
        [choice] if choice.has_tag_name(expected) => Ok(()),
        [choice] => Err(format!(
            "{description} <{}> is unsupported",
            choice.tag_name().name()
        )),
        _ => Err(format!("{description} must contain exactly one mode")),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::{ParseContext, parse_text_match, validate_object_find};

    fn context() -> ParseContext {
        ParseContext {
            merge_sources: BTreeMap::new(),
            merge_targets: BTreeSet::new(),
        }
    }

    #[test]
    fn text_and_object_matchers_reject_unmodeled_modes() {
        let Ok(text) = roxmltree::Document::parse(
            "<Match><Search>Item Code:</Search><AllowArbitrarySpace>1</AllowArbitrarySpace><WordAnchor><none/></WordAnchor><CaseFolding><respect-case/></CaseFolding></Match>",
        ) else {
            panic!("unsupported text-match XML must parse");
        };
        assert!(matches!(
            parse_text_match(&text.root_element(), &mut context()),
            Err(message) if message.contains("respect-case")
        ));

        let Ok(object) = roxmltree::Document::parse(
            "<ObjectFind><Background>#000</Background><Tolerance>10</Tolerance><MinimumExtent>4pt</MinimumExtent><Fill>0pt</Fill><Edge><start/></Edge><Displace>0pt</Displace></ObjectFind>",
        ) else {
            panic!("unsupported object-find XML must parse");
        };
        assert!(matches!(
            validate_object_find(&object.root_element()),
            Err(message) if message.contains("background")
        ));
    }

    #[test]
    fn text_properties_are_retained_as_runtime_constraints() {
        let Ok(text) = roxmltree::Document::parse(
            "<Match><Search>Item Code:</Search><AllowArbitrarySpace>1</AllowArbitrarySpace><WordAnchor><none/></WordAnchor><CaseFolding><ignore-case/></CaseFolding><TextProperties><FaceNameMatch><Enable>1</Enable><FontFace>Sans</FontFace></FaceNameMatch><Weight><normal/></Weight><Style><upright/></Style><CellHeightMatch><Enable>1</Enable><CellHeight>10pt</CellHeight><Deviation>1pt</Deviation></CellHeightMatch><BaselineMatch><Enable>1</Enable><BaselineAngle>0</BaselineAngle><AngleDeviation>0</AngleDeviation></BaselineMatch></TextProperties></Match>",
        ) else {
            panic!("supported text-property XML must parse");
        };
        let mut context = context();
        let Ok(parsed) = parse_text_match(&text.root_element(), &mut context) else {
            panic!("supported text properties should parse");
        };
        assert_eq!(parsed.properties.font_face.as_deref(), Some("Sans"));
        assert_eq!(
            parsed.properties.cell_height.map(|metric| metric.value),
            Some(10.0)
        );
        assert_eq!(
            parsed.properties.baseline_angle.map(|metric| metric.value),
            Some(0.0)
        );
    }
}
