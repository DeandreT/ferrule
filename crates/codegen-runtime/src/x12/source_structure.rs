//! Explicit native structural source reader; strict/legacy parsers are unchanged.
use super::*;
use mapping::X12SourceStructure;

#[cfg(test)]
#[path = "source_structure_tests.rs"]
mod tests;

struct Reader<'a> {
    text: &'a str,
    position: usize,
    element: char,
    component: char,
    terminator: char,
    repetition: Option<char>,
    nodes: usize,
    segments: usize,
}

pub(super) fn parse(
    text: &str,
    schema: &SchemaNode,
    structure: &X12SourceStructure,
) -> Result<Instance, X12BoundaryError> {
    structure.validate(schema).map_err(descriptor)?;
    let mut reader = Reader {
        text,
        position: 0,
        element: '*',
        component: ':',
        terminator: '~',
        repetition: None,
        nodes: 0,
        segments: 0,
    };
    reader.skip();
    if reader.rest().starts_with("ISA") {
        reader.discover()?;
    }
    // Unconsumed source text is a native diagnostic, not an admission failure.
    // Matching a recognized particle still acquires and validates its terminator.
    reader.group(schema, structure, 0)
}

impl Reader<'_> {
    fn syntax_error(&self, reason: &'static str) -> X12BoundaryError {
        X12BoundaryError::Syntax {
            segment: self.segments,
            reason,
        }
    }
    fn rest(&self) -> &str {
        &self.text[self.position..]
    }
    fn skip(&mut self) {
        self.position = self.text.len()
            - self
                .rest()
                .trim_start_matches(super::source_mapforce::whitespace)
                .len();
    }
    fn tag(&self) -> &str {
        let text = self
            .rest()
            .trim_start_matches(super::source_mapforce::whitespace);
        if text.starts_with("ISA") {
            return "ISA";
        }
        text.split([self.element, self.component, self.terminator])
            .next()
            .unwrap_or("")
    }
    fn discover(&mut self) -> Result<(), X12BoundaryError> {
        let text = self.rest();
        let element = text
            .chars()
            .nth(3)
            .ok_or_else(|| self.syntax_error("truncated native ISA"))?;
        let fields = text.split(element).take(17).collect::<Vec<_>>();
        if fields.len() != 17 {
            return Err(self.syntax_error("truncated native ISA"));
        }
        let mut last = fields[16].chars();
        let component = last
            .next()
            .ok_or_else(|| self.syntax_error("truncated native ISA component separator"))?;
        let terminator = last
            .next()
            .ok_or_else(|| self.syntax_error("truncated native ISA terminator"))?;
        let repetition = fields[11].chars().next().filter(|c| !c.is_alphanumeric());
        self.element = element;
        self.component = component;
        self.terminator = terminator;
        self.repetition = repetition;
        Ok(())
    }
    fn matches(&self, node: &SchemaNode, plan: &X12SourceStructure) -> bool {
        if let Some(id) = &plan.segment {
            if self.tag() != id {
                return false;
            }
            if id == "ISA" {
                return true;
            }
            let mut fields = self
                .rest()
                .trim_start_matches(super::source_mapforce::whitespace)
                .split(self.terminator)
                .next()
                .unwrap_or("")
                .split(self.element)
                .skip(1);
            let SchemaKind::Group { children, .. } = &node.kind else {
                return false;
            };
            for child in children {
                let field = fields.next().unwrap_or("");
                if let Some(fixed) = &child.fixed
                    && field != fixed
                {
                    return false;
                }
            }
            return true;
        }
        let SchemaKind::Group { children, .. } = &node.kind else {
            return false;
        };
        for (child, plan) in children.iter().zip(&plan.children) {
            if self.matches(child, plan) {
                return true;
            }
            if plan.minimum > 0 {
                break;
            }
        }
        false
    }
    fn group(
        &mut self,
        node: &SchemaNode,
        plan: &X12SourceStructure,
        depth: usize,
    ) -> Result<Instance, X12BoundaryError> {
        self.nodes += 1;
        if depth > 64 || self.nodes > MAX_X12_NODES {
            return Err(descriptor("native X12 structure limit exceeded"));
        }
        if let Some(id) = &plan.segment {
            self.skip();
            if id == "ISA" {
                self.discover()?;
            }
            let end = self
                .rest()
                .find(self.terminator)
                .ok_or_else(|| self.syntax_error("unterminated native X12 segment"))?;
            let raw = &self.rest()[..end];
            resource(
                "x12_elements",
                raw.matches(self.element).count(),
                MAX_ELEMENTS,
            )?;
            let mut tokens = raw.matches(self.element).count();
            for field in raw.split(self.element).skip(1) {
                let components = if id == "ISA" {
                    1
                } else {
                    field.matches(self.component).count() + 1
                };
                resource("x12_components", components, MAX_ELEMENTS)?;
                let repeats = self
                    .repetition
                    .filter(|_| id != "ISA")
                    .map_or(1, |separator| field.matches(separator).count() + 1);
                resource("x12_repetitions", repeats, MAX_X12_LOOP_INSTANCES)?;
                tokens += components + repeats;
            }
            resource("x12_nodes", self.nodes + tokens, MAX_X12_NODES)?;
            let elements = raw
                .split(self.element)
                .skip(1)
                .map(|field| {
                    let repeats = if id == "ISA" {
                        vec![field]
                    } else {
                        self.repetition.map_or_else(
                            || vec![field],
                            |separator| field.split(separator).collect(),
                        )
                    };
                    repeats
                        .into_iter()
                        .map(|repeat| {
                            if id == "ISA" {
                                vec![repeat.to_string()]
                            } else {
                                repeat.split(self.component).map(str::to_string).collect()
                            }
                        })
                        .collect()
                })
                .collect();
            let segment = format_edi::Segment {
                id: id.clone(),
                elements,
            };
            let value = format_edi::x12::project_lexical_segment(node, &segment, self.component)?;
            self.nodes += tokens;
            self.position += end + self.terminator.len_utf8();
            self.segments += 1;
            if self.segments > MAX_X12_SEGMENTS {
                return Err(descriptor("native X12 segment limit exceeded"));
            }
            return Ok(value);
        }
        let SchemaKind::Group { children, .. } = &node.kind else {
            return Err(descriptor("native X12 container must be a group"));
        };
        let mut values = Vec::with_capacity(children.len());
        for (child, particle) in children.iter().zip(&plan.children) {
            let maximum = if particle.merged_entries > 1 {
                Some(particle.merged_entries)
            } else if particle.respect_maximum || particle.maximum.is_some_and(|n| n <= 1) {
                particle.maximum
            } else {
                None
            };
            let mut items = Vec::new();
            while maximum.is_none_or(|max| items.len() < (max as usize)) {
                self.skip();
                if self.rest().is_empty() || !self.matches(child, particle) {
                    break;
                }
                let before = self.position;
                items.push(self.group(child, particle, depth + 1)?);
                if self.position == before {
                    return Err(descriptor("native X12 particle made no progress"));
                }
            }
            if items.len() < particle.minimum as usize {
                return Err(self.syntax_error("missing required native X12 particle"));
            }
            let value = if child.repeating {
                Instance::Repeated(items)
            } else {
                items
                    .into_iter()
                    .next()
                    .unwrap_or(Instance::Scalar(Value::Null))
            };
            values.push((child.name.clone(), value));
        }
        Ok(Instance::Group(values.into()))
    }
}
