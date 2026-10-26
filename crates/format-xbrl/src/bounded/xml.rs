use quick_xml::{NsReader, events::Event, name::ResolveResult};

use super::{XbrlLimit, XbrlLimitOptions, add, check};
use crate::{XBRLDI, XBRLI, XbrlFormatError};

pub(crate) struct XmlBudget<'a> {
    limits: &'a XbrlLimitOptions,
    depth: usize,
    nodes: usize,
    attributes: usize,
    text: usize,
    facts: usize,
    contexts: usize,
    units: usize,
    dimensions: usize,
}

impl<'a> XmlBudget<'a> {
    pub(crate) fn new(limits: &'a XbrlLimitOptions) -> Result<Self, XbrlFormatError> {
        check(XbrlLimit::XmlNodes, 1, limits.max_xml_nodes)?;
        Ok(Self {
            limits,
            depth: 0,
            nodes: 1,
            attributes: 0,
            text: 0,
            facts: 0,
            contexts: 0,
            units: 0,
            dimensions: 0,
        })
    }

    pub(crate) fn event(
        &mut self,
        event: &Event<'_>,
        namespace: Option<&[u8]>,
    ) -> Result<(), XbrlFormatError> {
        match event {
            Event::Start(start) | Event::Empty(start) => {
                self.node()?;
                check(
                    XbrlLimit::XmlDepth,
                    self.depth + 1,
                    self.limits.max_xml_depth,
                )?;
                let local = start.local_name();
                let local = local.as_ref();
                if namespace == Some(XBRLI.as_bytes()) && local == b"context" {
                    add(
                        &mut self.contexts,
                        1,
                        XbrlLimit::Contexts,
                        self.limits.max_contexts,
                    )?;
                } else if namespace == Some(XBRLI.as_bytes()) && local == b"unit" {
                    add(&mut self.units, 1, XbrlLimit::Units, self.limits.max_units)?;
                } else if namespace == Some(XBRLDI.as_bytes())
                    && matches!(local, b"explicitMember" | b"typedMember")
                {
                    add(
                        &mut self.dimensions,
                        1,
                        XbrlLimit::Dimensions,
                        self.limits.max_dimensions,
                    )?;
                } else if self.depth == 1
                    && namespace != Some(XBRLI.as_bytes())
                    && namespace != Some(crate::LINK.as_bytes())
                {
                    add(&mut self.facts, 1, XbrlLimit::Facts, self.limits.max_facts)?;
                }
                for attribute in start.attributes() {
                    let attribute =
                        attribute.map_err(|e| XbrlFormatError::XmlPreflight(e.to_string()))?;
                    add(
                        &mut self.attributes,
                        1,
                        XbrlLimit::XmlAttributes,
                        self.limits.max_xml_attributes,
                    )?;
                    self.text(attribute.value.len())?;
                    // NsReader compares raw namespace values, whereas roxmltree
                    // decodes references. Refuse this differential explicitly.
                    if (attribute.key.as_ref() == b"xmlns"
                        || attribute.key.as_ref().starts_with(b"xmlns:"))
                        && attribute.value.contains(&b'&')
                    {
                        return Err(XbrlFormatError::UnsupportedBoundedProfile(
                            "references in namespace declarations",
                        ));
                    }
                }
                if matches!(event, Event::Start(_)) {
                    self.depth += 1;
                }
            }
            Event::End(_) => {
                self.depth = self
                    .depth
                    .checked_sub(1)
                    .ok_or_else(|| XbrlFormatError::XmlPreflight("unmatched end element".into()))?
            }
            Event::DocType(_) => {
                return Err(XbrlFormatError::UnsupportedBoundedProfile(
                    "DTDs and entity declarations",
                ));
            }
            Event::GeneralRef(reference) => {
                let value = reference.as_ref();
                if !matches!(value, b"lt" | b"gt" | b"amp" | b"apos" | b"quot")
                    && !value.starts_with(b"#")
                {
                    return Err(XbrlFormatError::UnsupportedBoundedProfile(
                        "general entities",
                    ));
                }
                self.node()?;
                self.text(value.len().saturating_add(2))?;
            }
            Event::Eof => {}
            _ => {
                // Only absent text is exempt. Empty comments and CDATA still
                // materialize DOM nodes and must consume the pre-DOM budget.
                if !matches!(event, Event::Text(text) if text.is_empty()) {
                    self.node()?;
                }
                self.text(event.len())?;
            }
        }
        Ok(())
    }

    fn node(&mut self) -> Result<(), XbrlFormatError> {
        add(
            &mut self.nodes,
            1,
            XbrlLimit::XmlNodes,
            self.limits.max_xml_nodes,
        )
    }

    pub(crate) fn whitespace(&mut self, bytes: usize) -> Result<(), XbrlFormatError> {
        self.node()?;
        self.text(bytes)
    }

    fn text(&mut self, bytes: usize) -> Result<(), XbrlFormatError> {
        add(
            &mut self.text,
            bytes,
            XbrlLimit::XmlTextBytes,
            self.limits.max_xml_text_bytes,
        )
    }
}

pub(super) fn preflight(text: &str, limits: &XbrlLimitOptions) -> Result<(), XbrlFormatError> {
    let mut reader = NsReader::from_str(text);
    reader.config_mut().check_comments = true;
    let mut budget = XmlBudget::new(limits)?;
    loop {
        let (namespace, event) = reader
            .read_resolved_event()
            .map_err(|e| XbrlFormatError::XmlPreflight(e.to_string()))?;
        let namespace = match &namespace {
            ResolveResult::Bound(namespace) => Some(namespace.as_ref()),
            ResolveResult::Unbound => None,
            ResolveResult::Unknown(_) => {
                return Err(XbrlFormatError::XmlPreflight(
                    "undeclared namespace prefix".into(),
                ));
            }
        };
        budget.event(&event, namespace)?;
        if matches!(event, Event::Eof) {
            return Ok(());
        }
    }
}
