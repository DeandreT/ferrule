use std::collections::BTreeSet;

use super::*;

const WIDTHS: [usize; 16] = [2, 10, 2, 10, 2, 15, 2, 15, 6, 4, 1, 5, 9, 1, 1, 1];

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Syntax {
    element: char,
    component: char,
    segment: char,
    repetition: Option<char>,
}

impl Syntax {
    pub(super) fn selected(value: &Json, version: &str) -> Result<Option<Self>, X12BoundaryError> {
        if value.is_null() {
            return Ok(None);
        }
        let character = |name: &str| {
            let text = value[name]
                .as_str()
                .ok_or_else(|| descriptor("invalid separator"))?;
            let mut chars = text.chars();
            let value = chars.next().ok_or_else(|| descriptor("empty separator"))?;
            if chars.next().is_some() {
                return Err(descriptor("separator must be one character"));
            }
            Ok(value)
        };
        let syntax = Self {
            element: character("element")?,
            component: character("component")?,
            segment: character("segment")?,
            repetition: if version == "00401" {
                None
            } else if value.get("repetition").is_some() {
                Some(character("repetition")?)
            } else {
                None
            },
        };
        syntax.check()?;
        Ok(Some(syntax))
    }

    pub(super) fn output(profile: &Profile) -> Self {
        profile.syntax.unwrap_or(Self {
            element: '*',
            component: ':',
            segment: '~',
            repetition: (profile.version != "00401").then_some('^'),
        })
    }

    fn check(self) -> Result<(), X12BoundaryError> {
        let characters = [
            Some(self.element),
            Some(self.component),
            Some(self.segment),
            self.repetition,
        ];
        let mut seen = BTreeSet::new();
        for character in characters.into_iter().flatten() {
            // The native writer emits an LF terminator without a second line break.
            let terminator = character == self.segment && character == '\n';
            if !(character.is_ascii_punctuation() || terminator) || !seen.insert(character) {
                return Err(X12BoundaryError::Syntax {
                    segment: 0,
                    reason: "distinct ASCII punctuation separators required; only the segment terminator may be LF",
                });
            }
        }
        Ok(())
    }

    pub(super) fn native(self) -> format_edi::x12::Separators {
        format_edi::x12::Separators {
            element: self.element,
            component: self.component,
            segment: self.segment,
            repetition: self.repetition,
            release: None,
        }
    }
}

struct Segment<'a> {
    id: &'a str,
    elements: Vec<&'a str>,
    original: &'a str,
}
impl Segment<'_> {
    fn field(&self, index: usize) -> &str {
        self.elements.get(index).copied().unwrap_or("")
    }
}

pub(super) struct Document<'a> {
    pub(super) syntax: Syntax,
    segments: Vec<Segment<'a>>,
}

impl<'a> Document<'a> {
    pub(super) fn read(text: &'a str, profile: &Profile) -> Result<Self, X12BoundaryError> {
        let prefix = text.as_bytes().get(..106).ok_or(X12BoundaryError::Syntax {
            segment: 0,
            reason: "complete fixed-width ISA required at offset zero",
        })?;
        if !prefix.starts_with(b"ISA") || !prefix.iter().all(u8::is_ascii) {
            return Err(X12BoundaryError::Syntax {
                segment: 0,
                reason: "complete ASCII ISA required at offset zero; no BOM or leading whitespace",
            });
        }
        let syntax = Syntax {
            element: char::from(prefix[3]),
            component: char::from(prefix[104]),
            segment: char::from(prefix[105]),
            repetition: (profile.version != "00401").then_some(char::from(prefix[82])),
        };
        syntax.check()?;
        if profile.syntax.is_some_and(|selected| {
            selected.element != syntax.element
                || selected.component != syntax.component
                || selected.segment != syntax.segment
                || selected.repetition.is_some() && selected.repetition != syntax.repetition
        }) {
            return Err(X12BoundaryError::Syntax {
                segment: 0,
                reason: "wire separators disagree with selected syntax",
            });
        }
        let mut offset = 3;
        for width in WIDTHS {
            if prefix[offset] != prefix[3] {
                return Err(X12BoundaryError::Envelope {
                    segment: 0,
                    path: "ISA".into(),
                    reason: "ISA element widths are invalid",
                });
            }
            offset += width + 1;
        }
        let mut segments = Vec::new();
        let mut remaining = text;
        while !remaining.is_empty() {
            if !segments.is_empty() {
                remaining = remaining.trim_start_matches(['\r', '\n', ' ', '\t']);
            }
            if remaining.is_empty() {
                break;
            }
            let index = segments.len();
            let end = remaining
                .find(syntax.segment)
                .ok_or(X12BoundaryError::Syntax {
                    segment: index,
                    reason: "unterminated segment",
                })?;
            let raw = &remaining[..end];
            resource("x12_segments", index + 1, MAX_X12_SEGMENTS)?;
            if raw.chars().any(char::is_control) {
                return Err(X12BoundaryError::Syntax {
                    segment: index,
                    reason: "control character inside a segment",
                });
            }
            // Count before collecting to bound punctuation-driven allocations.
            resource(
                "x12_elements",
                raw.matches(syntax.element).count(),
                MAX_ELEMENTS,
            )?;
            let mut parts = raw.split(syntax.element);
            let id = parts.next().unwrap_or_default();
            if segment_id(id) != Some(id) {
                return Err(X12BoundaryError::Syntax {
                    segment: index,
                    reason: "invalid segment identifier",
                });
            }
            let elements: Vec<_> = parts.collect();
            for element in &elements {
                resource(
                    "x12_components",
                    element.matches(syntax.component).count() + 1,
                    MAX_ELEMENTS,
                )?;
                if let Some(repetition) = syntax.repetition {
                    resource(
                        "x12_repetitions",
                        element.matches(repetition).count() + 1,
                        MAX_X12_LOOP_INSTANCES,
                    )?;
                }
            }
            segments.push(Segment {
                id,
                elements,
                original: &remaining[..end + 1],
            });
            remaining = &remaining[end + 1..];
        }
        Ok(Self { syntax, segments })
    }

    fn declared(profile: &Profile) -> BTreeSet<&str> {
        SchemaCursor::root(&profile.schema)
            .segments()
            .filter_map(SchemaCursor::segment_id)
            .collect()
    }

    pub(super) fn native_input(&self, profile: &Profile) -> Result<String, X12BoundaryError> {
        let ids = Self::declared(profile);
        let mut text = String::new();
        for segment in &self.segments {
            if !profile.lenient || ids.contains(segment.id) {
                text.push_str(segment.original);
                input_size(text.len())?;
            }
        }
        Ok(text)
    }

    pub(super) fn validate(&self, profile: &Profile) -> Result<(), X12BoundaryError> {
        self.validate_with_name_limit(profile, MAX_X12_MATERIALIZED_NAME_BYTES)
    }

    pub(super) fn validate_with_name_limit(
        &self,
        profile: &Profile,
        name_limit: usize,
    ) -> Result<(), X12BoundaryError> {
        self.envelopes(profile)?;
        let declared = Self::declared(profile);
        let selected: Vec<_> = self
            .segments
            .iter()
            .enumerate()
            .filter(|(_, segment)| !profile.lenient || declared.contains(segment.id))
            .collect();
        let mut reader = Reader {
            document: self,
            profile,
            selected,
            position: 0,
            budget: Budget::default(),
            name_limit,
        };
        reader.node(
            SchemaCursor::root(&profile.schema),
            &mut Vec::new(),
            &mut Vec::new(),
            0,
        )?;
        if reader.position != reader.selected.len() {
            return Err(reader.failure("unconsumed declared segment or fixed qualifier mismatch"));
        }
        Ok(())
    }

    fn envelopes(&self, profile: &Profile) -> Result<(), X12BoundaryError> {
        // Three explicit owners, each closed only by its own trailer. Equal
        // counts/control text never authorize a transition to a different owner.
        let mut interchange: Option<(usize, &Segment<'_>, usize)> = None;
        let mut group: Option<(&Segment<'_>, usize)> = None;
        let mut transaction: Option<(usize, &Segment<'_>)> = None;
        let mut interchanges: usize = 0;
        for (index, segment) in self.segments.iter().enumerate() {
            let path = owner_path(
                profile,
                if segment.id == "ISA" {
                    interchanges
                } else {
                    interchanges.saturating_sub(1)
                },
                interchange.map_or(0, |(_, _, count)| count),
                group.map_or(0, |(_, count)| count),
                segment.id,
            );
            let fail = |reason| X12BoundaryError::Envelope {
                segment: index,
                path: path.clone(),
                reason,
            };
            let width = |count| {
                if segment.elements.len() == count {
                    Ok(())
                } else {
                    Err(fail("envelope element count is invalid"))
                }
            };
            match segment.id {
                "ISA" => {
                    if interchange.is_some()
                        || group.is_some()
                        || transaction.is_some()
                        || profile.envelope != EnvelopeProfile::MapForce && interchanges != 0
                    {
                        return Err(fail("unexpected interchange header"));
                    }
                    width(16)?;
                    for (value, width) in segment.elements.iter().zip(WIDTHS) {
                        if value.len() != width
                            || !value.bytes().all(|b| (b' '..=b'~').contains(&b))
                        {
                            return Err(fail("ISA requires fixed printable ASCII widths"));
                        }
                    }
                    if segment.field(11) != profile.version
                        || profile.version == "00401" && segment.field(10) != "U"
                    {
                        return Err(X12BoundaryError::UnsupportedProfile {
                            segment: index,
                            reason: "ISA version/standards identifier disagrees with selected profile",
                        });
                    }
                    if segment.field(15) != self.syntax.component.to_string()
                        || self
                            .syntax
                            .repetition
                            .is_some_and(|r| segment.field(10) != r.to_string())
                    {
                        return Err(fail("later ISA syntax differs from first interchange"));
                    }
                    if !digits(segment.field(12), 9, 9)
                        || segment.field(5).trim().is_empty()
                        || segment.field(7).trim().is_empty()
                        || !date(segment.field(8), true)
                        || !clock(segment.field(9), 4, 4)
                        || !matches!(segment.field(13), "0" | "1")
                        || !matches!(segment.field(14), "P" | "T" | "I")
                    {
                        return Err(fail(
                            "invalid ISA identity, control, date, time or indicator",
                        ));
                    }
                    interchanges += 1;
                    interchange = Some((index, segment, 0));
                }
                "GS" => {
                    if interchange.is_none() || group.is_some() || transaction.is_some() {
                        return Err(fail("group header has no open interchange owner"));
                    }
                    width(8)?;
                    if segment.field(0).trim().is_empty()
                        || segment.field(1).trim().is_empty()
                        || segment.field(2).trim().is_empty()
                        || !digits(segment.field(5), 1, 9)
                        || !date(segment.field(3), false)
                        || !clock(segment.field(4), 4, 8)
                    {
                        return Err(fail("invalid GS identity, control, date or time"));
                    }
                    if profile.envelope != EnvelopeProfile::MapForce
                        && segment.field(7) != format!("{}0", profile.version)
                    {
                        return Err(X12BoundaryError::UnsupportedProfile {
                            segment: index,
                            reason: "GS version disagrees with selected profile",
                        });
                    }
                    group = Some((segment, 0));
                }
                "ST" => {
                    if group.is_none() || transaction.is_some() {
                        return Err(fail("transaction header has no open group owner"));
                    }
                    let expected =
                        st_width(&profile.schema).ok_or_else(|| descriptor("missing ST schema"))?;
                    if !(2..=expected).contains(&segment.elements.len()) {
                        return Err(fail("transaction envelope element count is invalid"));
                    }
                    if !digits(segment.field(0), 3, 3) || !digits(segment.field(1), 4, 9) {
                        return Err(fail("invalid ST identity or control"));
                    }
                    transaction = Some((index, segment));
                }
                "SE" => {
                    let (start, header) =
                        transaction.ok_or_else(|| fail("transaction trailer has no open owner"))?;
                    width(2)?;
                    if !count(segment.field(0), index - start + 1)
                        || segment.field(1) != header.field(1)
                    {
                        return Err(fail(
                            "transaction count or control does not match its owner",
                        ));
                    }
                    transaction = None;
                    group
                        .as_mut()
                        .ok_or_else(|| fail("transaction has no group"))?
                        .1 += 1;
                }
                "GE" => {
                    let (header, transactions) =
                        group.ok_or_else(|| fail("group trailer has no open owner"))?;
                    width(2)?;
                    if transaction.is_some()
                        || transactions == 0
                        || profile.envelope == EnvelopeProfile::Single && transactions != 1
                        || !count(segment.field(0), transactions)
                        || !control_equal(segment.field(1), header.field(5), profile.envelope)
                    {
                        return Err(fail("group count/control or owner transition is invalid"));
                    }
                    group = None;
                    interchange
                        .as_mut()
                        .ok_or_else(|| fail("group has no interchange"))?
                        .2 += 1;
                }
                "IEA" => {
                    let (_, header, groups) =
                        interchange.ok_or_else(|| fail("interchange trailer has no open owner"))?;
                    width(2)?;
                    if group.is_some()
                        || transaction.is_some()
                        || groups == 0
                        || profile.envelope == EnvelopeProfile::Single && groups != 1
                        || !count(segment.field(0), groups)
                        || !control_equal(segment.field(1), header.field(12), profile.envelope)
                    {
                        return Err(fail(
                            "interchange count/control or owner transition is invalid",
                        ));
                    }
                    interchange = None;
                }
                _ if transaction.is_none() => {
                    return Err(fail(
                        "body or acknowledgment segment outside an open transaction",
                    ));
                }
                _ => {}
            }
        }
        if interchanges == 0 || interchange.is_some() || group.is_some() || transaction.is_some() {
            return Err(X12BoundaryError::Envelope {
                segment: self.segments.len(),
                path: String::new(),
                reason: "unclosed or absent interchange",
            });
        }
        Ok(())
    }
}

fn st_width(schema: &SchemaNode) -> Option<usize> {
    SchemaCursor::root(schema)
        .segments()
        .find(|node| node.segment_id() == Some("ST"))
        .map(|node| children(node.schema()).len())
}

fn owner_path(
    profile: &Profile,
    interchange: usize,
    group: usize,
    transaction: usize,
    id: &str,
) -> String {
    let mut parts = Vec::new();
    let mut node = &profile.schema;
    if profile.envelope == EnvelopeProfile::MapForce
        && let Some(child) = children(node).first()
    {
        parts.push(format!("{}[{interchange}]", child.name));
        node = child;
    }
    if profile.envelope != EnvelopeProfile::Single && !matches!(id, "ISA" | "IEA") {
        if let Some(child) = children(node).get(1) {
            parts.push(format!("{}[{group}]", child.name));
            node = child;
        }
        if !matches!(id, "GS" | "GE")
            && let Some(child) = children(node).get(1)
        {
            parts.push(format!("{}[{transaction}]", child.name));
        }
    }
    parts.push(id.to_owned());
    parts.join("/")
}

fn digits(value: &str, minimum: usize, maximum: usize) -> bool {
    (minimum..=maximum).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_digit())
}
fn count(value: &str, expected: usize) -> bool {
    digits(value, 1, 9) && value.parse::<usize>() == Ok(expected)
}
fn control_equal(left: &str, right: &str, profile: EnvelopeProfile) -> bool {
    digits(left, 1, 9)
        && if profile == EnvelopeProfile::MapForce {
            left.parse::<u64>().ok() == right.parse::<u64>().ok()
        } else {
            left == right
        }
}
fn date(value: &str, short: bool) -> bool {
    if !digits(value, if short { 6 } else { 8 }, if short { 6 } else { 8 }) {
        return false;
    }
    let year: u32 = if short {
        2000 + value[..2].parse::<u32>().unwrap_or(0)
    } else {
        value[..4].parse().unwrap_or(0)
    };
    let at = value.len() - 4;
    let month: usize = value[at..at + 2].parse().unwrap_or(0);
    let day: u32 = value[at + 2..].parse().unwrap_or(0);
    let days = [
        0,
        31,
        if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) {
            29
        } else {
            28
        },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    year > 0 && month > 0 && month < days.len() && day > 0 && day <= days[month]
}
fn clock(value: &str, minimum: usize, maximum: usize) -> bool {
    digits(value, minimum, maximum)
        && value.len() != 5
        && value[..2].parse::<u8>().is_ok_and(|n| n < 24)
        && value[2..4].parse::<u8>().is_ok_and(|n| n < 60)
        && (value.len() < 6 || value[4..6].parse::<u8>().is_ok_and(|n| n < 60))
}

struct Reader<'a, 'd> {
    document: &'a Document<'d>,
    profile: &'a Profile,
    selected: Vec<(usize, &'a Segment<'d>)>,
    position: usize,
    budget: Budget,
    name_limit: usize,
}
impl Reader<'_, '_> {
    fn failure(&self, reason: &'static str) -> X12BoundaryError {
        X12BoundaryError::Syntax {
            segment: self
                .selected
                .get(self.position)
                .map_or(self.document.segments.len(), |(index, _)| *index),
            reason,
        }
    }
    fn matches(&self, node: SchemaCursor<'_>) -> bool {
        let schema = node.schema();
        let Some((_, segment)) = self.selected.get(self.position) else {
            return false;
        };
        if let Some(id) = node.segment_id() {
            return id == segment.id
                && children(schema).iter().enumerate().all(|(index, element)| {
                    match &element.kind {
                        SchemaKind::Scalar { .. } => element
                            .fixed
                            .as_deref()
                            .is_none_or(|fixed| fixed == segment.field(index)),
                        SchemaKind::Group { children, .. } => {
                            children.iter().enumerate().all(|(at, child)| {
                                child.fixed.as_deref().is_none_or(|fixed| {
                                    segment
                                        .field(index)
                                        .split(self.document.syntax.component)
                                        .nth(at)
                                        .unwrap_or("")
                                        == fixed
                                })
                            })
                        }
                        _ => false,
                    }
                });
        }
        for child in node.children() {
            if self.matches(child) {
                return true;
            }
            if !child.schema().repeating {
                break;
            }
        }
        false
    }
    fn node(
        &mut self,
        node: SchemaCursor<'_>,
        path: &mut Vec<String>,
        owner: &mut Vec<String>,
        depth: usize,
    ) -> Result<(), X12BoundaryError> {
        let schema = node.schema();
        self.budget.node(depth)?;
        if node.segment_id().is_some() {
            if !self.matches(node) {
                return Err(self.failure("missing declared segment or fixed qualifier mismatch"));
            }
            let segment = self.selected[self.position].1;
            if segment.elements.len() > children(schema).len() {
                return Err(self.failure("undeclared elements would be lost by native parsing"));
            }
            for (index, element) in children(schema).iter().enumerate() {
                // Native read_segment clones every declared field name, including
                // absent positions and empty repeated element collections.
                self.budget.name(&element.name, self.name_limit)?;
                path.push(element.name.clone());
                owner.push(element.name.clone());
                let result = self.element(
                    element,
                    segment.elements.get(index).copied(),
                    path,
                    owner,
                    depth + 1,
                    match (segment.id, index) {
                        ("ISA", 10) => self.document.syntax.repetition,
                        ("ISA", 15) => Some(self.document.syntax.component),
                        _ => None,
                    },
                );
                path.pop();
                owner.pop();
                result?;
            }
            self.position += 1;
            return Ok(());
        }
        for child_node in node.children() {
            let child = child_node.schema();
            // The container slot exists even when its repeating collection is
            // empty. Charge once for this occurrence, not once per schema.
            self.budget.name(&child.name, self.name_limit)?;
            path.push(child.name.clone());
            if child.repeating {
                self.budget.node(depth + 1)?;
                let mut count = 0;
                while self.matches(child_node) {
                    if self.profile.is_acknowledgment(path) {
                        return Err(self.failure(
                            "acknowledgment content is outside the MapForce source profile",
                        ));
                    }
                    self.budget.loops(1)?;
                    owner.push(format!("{}[{count}]", child.name));
                    let before = self.position;
                    self.node(child_node, path, owner, depth + 1)?;
                    owner.pop();
                    if self.position == before {
                        return Err(self.failure("loop made no progress"));
                    }
                    count += 1;
                }
                cardinality(child, count, owner)?;
            } else {
                owner.push(child.name.clone());
                self.node(child_node, path, owner, depth + 1)?;
                owner.pop();
            }
            path.pop();
        }
        Ok(())
    }
    fn element(
        &mut self,
        schema: &SchemaNode,
        raw: Option<&str>,
        path: &[String],
        owner: &mut Vec<String>,
        depth: usize,
        allowed_reserved: Option<char>,
    ) -> Result<(), X12BoundaryError> {
        self.budget.node(depth)?;
        if schema.repeating {
            let mut count = 0;
            if let Some(raw) = raw {
                let separator = self.document.syntax.repetition.ok_or_else(|| {
                    self.failure("repeating element has no active repetition separator")
                })?;
                for part in raw.split(separator) {
                    self.budget.loops(1)?;
                    self.budget.node(depth + 1)?;
                    self.single(schema, part, path, owner, depth + 1, allowed_reserved)?;
                    count += 1;
                }
            }
            cardinality(schema, count, owner)
        } else {
            let raw = raw.unwrap_or("");
            if self
                .document
                .syntax
                .repetition
                .is_some_and(|r| raw.contains(r))
                && !allowed_reserved.is_some_and(|r| raw == r.to_string())
            {
                return Err(self.failure("undeclared repeating element would be truncated"));
            }
            self.single(schema, raw, path, owner, depth, allowed_reserved)
        }
    }
    fn single(
        &mut self,
        schema: &SchemaNode,
        raw: &str,
        path: &[String],
        owner: &mut Vec<String>,
        depth: usize,
        allowed_reserved: Option<char>,
    ) -> Result<(), X12BoundaryError> {
        match &schema.kind {
            SchemaKind::Scalar { .. } => {
                if raw.contains(self.document.syntax.component)
                    && !(allowed_reserved == Some(self.document.syntax.component)
                        && raw == self.document.syntax.component.to_string())
                {
                    return Err(self.failure("undeclared composite syntax"));
                }
                if schema.fixed.as_deref().is_some_and(|fixed| fixed != raw)
                    || schema
                        .string_length_range
                        .is_some_and(|range| !range.contains_str(raw))
                {
                    return Err(X12BoundaryError::Value {
                        path: owner.join("/"),
                        reason: "fixed value or string length mismatch",
                    });
                }
                if let Some(constraint) = self.profile.constraints.get(path) {
                    let length = raw.chars().count();
                    if length < constraint.min_chars() as usize
                        || length > constraint.max_chars() as usize
                        || !raw.is_empty()
                            && !constraint.allowed_values().is_empty()
                            && !constraint.allowed_values().iter().any(|value| value == raw)
                    {
                        return Err(X12BoundaryError::Value {
                            path: owner.join("/"),
                            reason: "lexical length or code-list constraint failed",
                        });
                    }
                }
                Ok(())
            }
            SchemaKind::Group { children, .. } => {
                let parts: Vec<_> = raw.split(self.document.syntax.component).collect();
                if parts.len() > children.len() {
                    return Err(self.failure("undeclared composite components"));
                }
                for (index, child) in children.iter().enumerate() {
                    // Native read_nested_component materializes missing slots.
                    self.budget.name(&child.name, self.name_limit)?;
                    let mut path = path.to_vec();
                    path.push(child.name.clone());
                    owner.push(child.name.clone());
                    self.budget.node(depth + 1)?;
                    self.single(
                        child,
                        parts.get(index).copied().unwrap_or(""),
                        &path,
                        owner,
                        depth + 1,
                        None,
                    )?;
                    owner.pop();
                }
                Ok(())
            }
            _ => Err(descriptor("unsupported scalar union in X12 descriptor")),
        }
    }
}

pub(super) fn cardinality(
    schema: &SchemaNode,
    count: usize,
    owner: &[String],
) -> Result<(), X12BoundaryError> {
    if schema
        .item_count_range
        .is_some_and(|range| !range.contains_len(count))
    {
        return Err(X12BoundaryError::Value {
            path: owner.join("/"),
            reason: "declared item count interval failed",
        });
    }
    Ok(())
}
