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
    known_segments: &'a [String],
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
        known_segments: &structure.known_segments,
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
    fn separator(&self, ch: char) -> bool {
        ch == self.element
            || ch == self.component
            || ch == self.terminator
            || Some(ch) == self.repetition
            || ch == '\0'
    }
    fn ignorable(&self, ch: char) -> bool {
        matches!(ch, '\t' | '\r' | '\n') && !self.separator(ch)
    }
    fn skip(&mut self) {
        while let Some(ch) = self.rest().chars().next() {
            if !super::source_mapforce::whitespace(ch) || self.separator(ch) {
                break;
            }
            self.position += ch.len_utf8();
        }
    }
    fn tag(&self) -> String {
        let text = self.rest();
        if text.starts_with("ISA") {
            return "ISA".into();
        }
        let mut tag = String::new();
        let mut escaped = false;
        for ch in text.chars() {
            if self.ignorable(ch) {
                continue;
            }
            if escaped {
                tag.push(ch);
                escaped = false;
            } else if ch == '\0' {
                escaped = true;
            } else if self.separator(ch) {
                break;
            } else {
                tag.push(ch);
            }
            if tag.len() > 32 {
                break;
            }
        }
        tag
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
            if self.tag() != *id {
                return false;
            }
            if id == "ISA" {
                return true;
            }
            let SchemaKind::Group { children, .. } = &node.kind else {
                return false;
            };
            if children.iter().all(|child| child.fixed.is_none()) {
                return true;
            }
            let Ok((segment, _, _)) = self.scan(id, false) else {
                return false;
            };
            for (index, child) in children.iter().enumerate() {
                let field = segment
                    .elements
                    .get(index)
                    .and_then(|repeats| repeats.first())
                    .and_then(|parts| parts.first())
                    .map_or("", String::as_str);
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
    fn scan(
        &self,
        id: &str,
        required: bool,
    ) -> Result<(format_edi::Segment, usize, bool), X12BoundaryError> {
        let mut fields = vec![vec![vec![String::new()]]];
        let mut escaped = false;
        let mut consumed = 0;
        let mut terminated = false;
        let mut tokens = 1usize;
        for ch in self.rest().chars() {
            consumed += ch.len_utf8();
            if self.ignorable(ch) {
                continue;
            }
            if escaped {
                fields
                    .last_mut()
                    .unwrap()
                    .last_mut()
                    .unwrap()
                    .last_mut()
                    .unwrap()
                    .push(ch);
                escaped = false;
            } else if ch == '\0' {
                if id == "ISA" {
                    return Err(self.syntax_error("NUL in native ISA"));
                }
                escaped = true;
            } else if ch == self.terminator {
                terminated = true;
                break;
            } else if ch == self.element {
                fields.push(vec![vec![String::new()]]);
                tokens += 2;
            } else if id != "ISA" && Some(ch) == self.repetition {
                let repeats = fields.last_mut().unwrap();
                repeats.push(vec![String::new()]);
                resource("x12_repetitions", repeats.len(), MAX_X12_LOOP_INSTANCES)?;
                tokens += 2;
            } else if id != "ISA" && ch == self.component {
                let parts = fields.last_mut().unwrap().last_mut().unwrap();
                parts.push(String::new());
                resource("x12_components", parts.len(), MAX_ELEMENTS)?;
                tokens += 1;
            } else {
                fields
                    .last_mut()
                    .unwrap()
                    .last_mut()
                    .unwrap()
                    .last_mut()
                    .unwrap()
                    .push(ch);
            }
            resource("x12_elements", fields.len() - 1, MAX_ELEMENTS)?;
            resource("x12_nodes", self.nodes + tokens, MAX_X12_NODES)?;
        }
        if required && !terminated {
            return Err(self.syntax_error("unterminated native X12 segment"));
        }
        let elements = fields.into_iter().skip(1).collect();
        Ok((
            format_edi::Segment {
                id: id.into(),
                elements,
            },
            consumed,
            terminated,
        ))
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
            let (segment, consumed, _) = self.scan(id, true)?;
            let tokens = segment
                .elements
                .iter()
                .map(|repeats| 1 + repeats.iter().map(|parts| 1 + parts.len()).sum::<usize>())
                .sum::<usize>();
            let value = super::source_fields::project(node, &segment)?;
            self.nodes += tokens;
            self.position += consumed;
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
                if self.rest().is_empty() {
                    break;
                }
                if !self.matches(child, particle) {
                    let tag = self.tag();
                    if particle.selection && !tag.is_empty() && !self.known_segments.contains(&tag)
                    {
                        return Err(
                            self.syntax_error("unrecognized segment at native X12 selection")
                        );
                    }
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
