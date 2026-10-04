//! A separate bounded ordinary reader. Legacy and observed root readers keep
//! their own policies; this path proves the complete projection before allocation.

use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};

mod parser_guards;

use super::{
    XmlFormatError, attribute_value, element_matches_schema, expanded_node_name, has_xml_nil,
    parse_input_schema_scalar, parse_schema_scalar, schema_xml_name,
};

/// Parse a closed ordinary schema using one DOM and a prospective result budget.
/// DTDs, alternatives and mixed content are deliberately outside this profile.
pub fn from_str_structured(text: &str, schema: &SchemaNode) -> Result<Instance, XmlFormatError> {
    if text.len() > ir::MAX_STRUCTURED_XML_DOCUMENT_BYTES {
        return Err(limit(
            "document bytes",
            ir::MAX_STRUCTURED_XML_DOCUMENT_BYTES,
        ));
    }
    if !ir::xml_structured_document_input_is_supported(schema) {
        return Err(XmlFormatError::UnsupportedStructuredInputSchema);
    }
    parser_guards::scan_physical_input(text)?;
    let document = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: ir::MAX_STRUCTURED_XML_PHYSICAL_NODES as u32,
            ..roxmltree::ParsingOptions::default()
        },
    )?;
    let root = document.root_element();
    if !element_matches_schema(&root, schema) {
        return Err(XmlFormatError::UnexpectedRoot {
            expected: schema_xml_name(schema),
            found: expanded_node_name(&root),
        });
    }
    let mut budget = ProjectionBudget::default();
    preflight(root, schema, &mut budget)?;
    materialize(root, schema)
}

fn limit(budget: &'static str, limit: usize) -> XmlFormatError {
    XmlFormatError::StructuredInputLimit { budget, limit }
}

struct ProjectionBudget {
    nodes: usize,
    bytes: usize,
    work: usize,
    node_limit: usize,
    byte_limit: usize,
    work_limit: usize,
}

impl Default for ProjectionBudget {
    fn default() -> Self {
        Self {
            nodes: 0,
            bytes: 0,
            work: 0,
            node_limit: ir::MAX_STRUCTURED_XML_MATERIALIZED_NODES,
            byte_limit: ir::MAX_STRUCTURED_XML_MATERIALIZED_BYTES,
            work_limit: ir::MAX_STRUCTURED_XML_PROJECTION_WORK,
        }
    }
}

impl ProjectionBudget {
    fn node(&mut self) -> Result<(), XmlFormatError> {
        self.nodes = self.nodes.saturating_add(1);
        if self.nodes > self.node_limit {
            return Err(limit("materialized nodes", self.node_limit));
        }
        Ok(())
    }
    fn bytes(&mut self, bytes: usize) -> Result<(), XmlFormatError> {
        self.bytes = self.bytes.saturating_add(bytes);
        if self.bytes > self.byte_limit {
            return Err(limit(
                "materialized field, string and text bytes",
                self.byte_limit,
            ));
        }
        Ok(())
    }
    fn work(&mut self, work: usize) -> Result<(), XmlFormatError> {
        self.work = self.work.saturating_add(work);
        if self.work > self.work_limit {
            return Err(limit("projection work", self.work_limit));
        }
        Ok(())
    }
}

fn direct_text_segments<'a, 'input: 'a>(
    element: roxmltree::Node<'a, 'input>,
) -> impl Iterator<Item = &'a str> {
    element
        .children()
        .filter(|node| node.is_text())
        .filter_map(|node| node.text())
        .filter(|text| {
            !text.trim().is_empty()
                || !text
                    .chars()
                    .any(|character| matches!(character, '\n' | '\r'))
        })
}

fn preflight(
    element: roxmltree::Node<'_, '_>,
    schema: &SchemaNode,
    budget: &mut ProjectionBudget,
) -> Result<(), XmlFormatError> {
    budget.node()?;
    for _ in element.attributes() {
        budget.work(1)?;
    }
    if element
        .attribute(("http://www.w3.org/2001/XMLSchema-instance", "nil"))
        .is_some_and(|nil| matches!(nil, "true" | "1"))
    {
        for _ in element.children() {
            budget.work(1)?;
        }
    }
    let nil = has_xml_nil(&element, schema)?;
    match &schema.kind {
        SchemaKind::Scalar { ty } => {
            for node in element.children() {
                budget.work(1)?;
                if node.is_element() {
                    return Err(XmlFormatError::StructuredInputContent {
                        name: schema.name.clone(),
                    });
                }
            }
            if !nil && *ty == ScalarType::String {
                budget.bytes(element.text().unwrap_or("").len())?;
            }
        }
        SchemaKind::Group { children, .. } => {
            if nil {
                return Err(XmlFormatError::UnsupportedXmlNilGroup {
                    name: schema.name.clone(),
                });
            }
            if children.iter().any(|child| child.text) {
                for node in element.children() {
                    budget.work(1)?;
                    if node.is_element() {
                        return Err(XmlFormatError::StructuredInputContent {
                            name: schema.name.clone(),
                        });
                    }
                }
            }
            for child in children {
                budget.work(1)?;
                if child.attribute {
                    for _ in element.attributes() {
                        budget.work(1)?;
                    }
                    budget.bytes(child.name.len())?;
                    budget.node()?;
                    if matches!(
                        &child.kind,
                        SchemaKind::Scalar {
                            ty: ScalarType::String
                        }
                    ) {
                        budget.bytes(attribute_value(&element, child).map_or(0, str::len))?;
                    }
                } else if child.text {
                    budget.bytes(child.name.len())?;
                    budget.node()?;
                    for _ in element.children() {
                        budget.work(1)?;
                    }
                    // Numeric and Boolean simple content also needs an owned
                    // temporary lexical String during parsing. Charge it before
                    // any concatenation, alongside retained String values.
                    for segment in direct_text_segments(element) {
                        budget.bytes(segment.len())?;
                    }
                } else {
                    if child.repeating {
                        budget.bytes(child.name.len())?;
                        budget.node()?;
                    }
                    let mut found = false;
                    for node in element.children() {
                        budget.work(1)?;
                        if !node.is_element() || !element_matches_schema(&node, child) {
                            continue;
                        }
                        if !child.repeating {
                            budget.bytes(child.name.len())?;
                        }
                        preflight(node, child, budget)?;
                        found = true;
                        if !child.repeating {
                            break;
                        }
                    }
                    if !child.repeating
                        && !found
                        && matches!(&child.kind, SchemaKind::Scalar { .. })
                    {
                        budget.bytes(child.name.len())?;
                        budget.node()?;
                    }
                }
            }
        }
        SchemaKind::ScalarUnion { .. } => {
            return Err(XmlFormatError::UnsupportedStructuredInputSchema);
        }
    }
    Ok(())
}

fn materialize(
    element: roxmltree::Node<'_, '_>,
    schema: &SchemaNode,
) -> Result<Instance, XmlFormatError> {
    if has_xml_nil(&element, schema)? {
        return Ok(Instance::Scalar(Value::xml_nil()));
    }
    match &schema.kind {
        SchemaKind::Scalar { ty } => Ok(Instance::Scalar(parse_input_schema_scalar(
            schema,
            *ty,
            element.text().unwrap_or(""),
        )?)),
        SchemaKind::Group { children, .. } => {
            // Do not reserve for absent group declarations: only emitted fields
            // and values were admitted by the prospective allocation budget.
            let mut fields = Vec::new();
            for child in children {
                if child.attribute || child.text {
                    let SchemaKind::Scalar { ty } = &child.kind else {
                        return Err(XmlFormatError::UnsupportedStructuredInputSchema);
                    };
                    let value = if child.attribute {
                        attribute_value(&element, child)
                            .map(|text| parse_schema_scalar(child, *ty, text))
                            .transpose()?
                            .unwrap_or(Value::Null)
                    } else {
                        let text: String = direct_text_segments(element).collect();
                        if *ty == ScalarType::String {
                            Value::String(text)
                        } else {
                            parse_input_schema_scalar(child, *ty, &text)?
                        }
                    };
                    fields.push((child.name.clone(), Instance::Scalar(value)));
                } else if child.repeating {
                    let mut items = Vec::new();
                    for node in element
                        .children()
                        .filter(|node| node.is_element() && element_matches_schema(node, child))
                    {
                        items.push(materialize(node, child)?);
                    }
                    fields.push((child.name.clone(), Instance::Repeated(items)));
                } else if let Some(node) = element
                    .children()
                    .find(|node| node.is_element() && element_matches_schema(node, child))
                {
                    fields.push((child.name.clone(), materialize(node, child)?));
                } else if matches!(&child.kind, SchemaKind::Scalar { .. }) {
                    fields.push((child.name.clone(), Instance::Scalar(Value::Null)));
                }
            }
            Ok(Instance::Group(fields.into()))
        }
        SchemaKind::ScalarUnion { .. } => Err(XmlFormatError::UnsupportedStructuredInputSchema),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_presence_scalar_text_and_nil_are_retained() {
        let mut price = SchemaNode::scalar("Price", ScalarType::Float);
        price.nillable = true;
        let mut item = SchemaNode::group(
            "Item",
            vec![price, SchemaNode::scalar("Code", ScalarType::String)],
        );
        item.repeating = true;
        let schema = SchemaNode::group("Items", vec![item, SchemaNode::group("Missing", vec![])]);
        for input in [
            "<Items/>",
            "<Items><Item/></Items>",
            "<Items xmlns:xsi='http://www.w3.org/2001/XMLSchema-instance'><Item><Price xsi:nil='true'/><Code>first<!--split-->second</Code></Item></Items>",
        ] {
            assert_eq!(
                from_str_structured(input, &schema).unwrap(),
                super::super::from_str(input, &schema).unwrap()
            );
        }
    }

    #[test]
    fn preflight_counts_absent_null_and_field_names_before_materialization() {
        let mut item = SchemaNode::group(
            "Item",
            vec![SchemaNode::scalar("Absent", ScalarType::String); 1],
        );
        item.repeating = true;
        let schema = SchemaNode::group("Items", vec![item]);
        let document = roxmltree::Document::parse("<Items><Item/><Item/></Items>").unwrap();
        let mut budget = ProjectionBudget {
            node_limit: 5,
            ..Default::default()
        };
        assert!(matches!(
            preflight(document.root_element(), &schema, &mut budget),
            Err(XmlFormatError::StructuredInputLimit {
                budget: "materialized nodes",
                limit: 5
            })
        ));
        let mut budget = ProjectionBudget {
            byte_limit: 15,
            ..Default::default()
        };
        assert!(matches!(
            preflight(document.root_element(), &schema, &mut budget),
            Err(XmlFormatError::StructuredInputLimit {
                budget: "materialized field, string and text bytes",
                limit: 15
            })
        ));
    }

    #[test]
    fn new_profile_rejects_doctype_depth_and_integer_exponent_without_changing_legacy() {
        let schema = SchemaNode::scalar("Value", ScalarType::Int);
        assert!(matches!(
            from_str_structured(
                "<!DOCTYPE Value [<!ELEMENT Value (#PCDATA)>]><Value>1</Value>",
                &schema
            ),
            Err(XmlFormatError::StructuredInputDtd)
        ));
        assert!(from_str_structured("<Value>1e0</Value>", &schema).is_err());
        let nested = format!("{}1{}", "<Value>".repeat(65), "</Value>".repeat(65));
        assert!(matches!(
            from_str_structured(&nested, &schema),
            Err(XmlFormatError::StructuredInputLimit {
                budget: "physical depth",
                ..
            })
        ));
    }

    #[test]
    fn simple_content_attributes_namespaces_and_numeric_lexicals_match_native() {
        let mut text = SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String);
        text.text = true;
        let mut attribute = SchemaNode::scalar("Code", ScalarType::String);
        attribute.attribute = true;
        let schema = SchemaNode::group("Record", vec![text, attribute]);
        for input in [
            "<q:Record xmlns:q='urn:ignored' Code='a&#x9;b'>a<![CDATA[b]]><!--split-->c</q:Record>",
            "<Record>\n  </Record>",
            "<Record> </Record>",
        ] {
            assert_eq!(
                from_str_structured(input, &schema).unwrap(),
                super::super::from_str(input, &schema).unwrap()
            );
        }
        assert!(matches!(
            from_str_structured("<Record>a<Unknown/>b</Record>", &schema),
            Err(XmlFormatError::StructuredInputContent { .. })
        ));
        for (ty, lexical) in [
            (ScalarType::Int, " +42 "),
            (ScalarType::Float, " 1e2 "),
            (ScalarType::Bool, " 0 "),
        ] {
            let schema = SchemaNode::scalar("Value", ty);
            let input = format!("<Value>{lexical}</Value>");
            assert_eq!(
                from_str_structured(&input, &schema).unwrap(),
                super::super::from_str(&input, &schema).unwrap()
            );
        }
    }

    #[test]
    fn small_document_cannot_expand_absent_scalars_or_cloned_names_past_result_caps() {
        let fields = (0..1000)
            .map(|index| SchemaNode::scalar(format!("F{index}"), ScalarType::String))
            .collect();
        let mut item = SchemaNode::group("Item", fields);
        item.repeating = true;
        let schema = SchemaNode::group("Items", vec![item]);
        let input = format!("<Items>{}</Items>", "<Item/>".repeat(1001));
        assert!(matches!(
            from_str_structured(&input, &schema),
            Err(XmlFormatError::StructuredInputLimit {
                budget: "materialized nodes",
                ..
            })
        ));
        let fields = (0..100)
            .map(|index| {
                SchemaNode::scalar(
                    format!("F{}X{index:03}", "x".repeat(4091)),
                    ScalarType::String,
                )
            })
            .collect();
        let mut item = SchemaNode::group("Item", fields);
        item.repeating = true;
        let schema = SchemaNode::group("Items", vec![item]);
        let input = format!("<Items>{}</Items>", "<Item/>".repeat(170));
        assert!(matches!(
            from_str_structured(&input, &schema),
            Err(XmlFormatError::StructuredInputLimit {
                budget: "materialized field, string and text bytes",
                ..
            })
        ));
    }

    #[test]
    fn absent_group_declarations_do_not_reserve_unemitted_field_slots() {
        let fields = (0..1000)
            .map(|index| SchemaNode::group(format!("G{index}"), vec![]))
            .collect();
        let mut item = SchemaNode::group("Item", fields);
        item.repeating = true;
        let schema = SchemaNode::group("Items", vec![item]);
        let input = format!("<Items>{}</Items>", "<Item/>".repeat(1001));
        let Instance::Group(fields) = from_str_structured(&input, &schema).unwrap() else {
            panic!("root group")
        };
        let Instance::Repeated(items) = &fields[0].1 else {
            panic!("repeated items")
        };
        assert_eq!(items.len(), 1001);
        assert!(
            items
                .iter()
                .all(|item| matches!(item, Instance::Group(fields) if fields.is_empty()))
        );
    }

    #[test]
    fn native_first_child_and_nil_comment_rules_are_preserved() {
        let mut schema = SchemaNode::scalar("Value", ScalarType::String);
        schema.nillable = true;
        for input in [
            "<Value><!--barrier-->later text</Value>",
            "<Value><?barrier value?>later text</Value>",
            "<Value xmlns:xsi='http://www.w3.org/2001/XMLSchema-instance' xsi:nil='true'><!-- --><?allowed value?></Value>",
        ] {
            assert_eq!(
                from_str_structured(input, &schema).unwrap(),
                super::super::from_str(input, &schema).unwrap()
            );
        }
        let input = "<Value xmlns:xsi='http://www.w3.org/2001/XMLSchema-instance' xsi:nil='1'><!--nonblank--></Value>";
        assert!(matches!(
            from_str_structured(input, &schema),
            Err(XmlFormatError::XmlNilWithContent { .. })
        ));
        assert!(matches!(
            super::super::from_str(input, &schema),
            Err(XmlFormatError::XmlNilWithContent { .. })
        ));
    }
    #[test]
    fn scalar_children_are_outside_the_new_profile_even_when_legacy_reads_a_value() {
        let schema = SchemaNode::scalar("Value", ScalarType::String);
        for input in [
            "<Value><Unknown/>later</Value>",
            "<Value>earlier<Unknown/></Value>",
        ] {
            assert!(matches!(
                from_str_structured(input, &schema),
                Err(XmlFormatError::StructuredInputContent { .. })
            ));
            assert!(super::super::from_str(input, &schema).is_ok());
        }
    }

    #[test]
    fn numeric_simple_content_lexical_bytes_are_charged_before_concatenation() {
        let mut text = SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::Int);
        text.text = true;
        let schema = SchemaNode::group("Value", vec![text]);
        let input = "<Value>1<![CDATA[2]]></Value>";
        let document = roxmltree::Document::parse(input).unwrap();
        let mut budget = ProjectionBudget {
            byte_limit: 6,
            ..Default::default()
        };
        assert!(matches!(
            preflight(document.root_element(), &schema, &mut budget),
            Err(XmlFormatError::StructuredInputLimit {
                budget: "materialized field, string and text bytes",
                limit: 6
            })
        ));
        assert_eq!(
            from_str_structured(input, &schema).unwrap(),
            super::super::from_str(input, &schema).unwrap()
        );
    }
}
