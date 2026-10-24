//! Cursor-preserving field projection for the explicit native source reader.
use super::*;

enum Token<'a> {
    Text(&'a str),
    Element,
    Repetition,
    Component,
}
struct Cursor<'a> {
    tokens: Vec<Token<'a>>,
    at: usize,
}
impl Cursor<'_> {
    fn component(&mut self) {
        if matches!(self.tokens.get(self.at), Some(Token::Component)) {
            self.at += 1;
        }
    }
    fn element(&mut self) {
        if matches!(self.tokens.get(self.at), Some(Token::Element)) {
            self.at += 1;
        }
    }
    fn item(&mut self, node: &SchemaNode) -> Result<Instance, X12BoundaryError> {
        match &node.kind {
            SchemaKind::Scalar { .. } => {
                let value = match self.tokens.get(self.at) {
                    Some(Token::Text(value)) => {
                        self.at += 1;
                        Value::String((*value).into())
                    }
                    _ => Value::Null,
                };
                Ok(Instance::Scalar(value))
            }
            SchemaKind::Group { children, .. } => {
                let mut fields = Vec::with_capacity(children.len());
                for (i, child) in children.iter().enumerate() {
                    if i > 0 {
                        self.component();
                    }
                    fields.push((child.name.clone(), self.item(child)?));
                }
                Ok(Instance::Group(fields.into()))
            }
            _ => Err(descriptor("unsupported native X12 field kind")),
        }
    }
    fn field(&mut self, node: &SchemaNode) -> Result<Instance, X12BoundaryError> {
        if !node.repeating {
            return self.item(node);
        }
        let mut items = Vec::new();
        if matches!(
            self.tokens.get(self.at),
            Some(Token::Text(_) | Token::Component)
        ) {
            items.push(self.item(node)?);
            while matches!(self.tokens.get(self.at), Some(Token::Repetition)) {
                self.at += 1;
                items.push(self.item(node)?);
            }
        }
        Ok(Instance::Repeated(items))
    }
}

pub(super) fn project(
    node: &SchemaNode,
    segment: &format_edi::Segment,
) -> Result<Instance, X12BoundaryError> {
    let SchemaKind::Group { children, .. } = &node.kind else {
        return Err(descriptor("native segment is not a group"));
    };
    let mut tokens = Vec::new();
    for (element, repeats) in segment.elements.iter().enumerate() {
        if element > 0 {
            tokens.push(Token::Element);
        }
        for (repeat, components) in repeats.iter().enumerate() {
            if repeat > 0 {
                tokens.push(Token::Repetition);
            }
            for (component, text) in components.iter().enumerate() {
                if component > 0 {
                    tokens.push(Token::Component);
                }
                tokens.push(Token::Text(text));
            }
        }
    }
    let mut cursor = Cursor { tokens, at: 0 };
    let mut fields = Vec::with_capacity(children.len());
    for (i, child) in children.iter().enumerate() {
        if i > 0 {
            cursor.element();
        }
        fields.push((child.name.clone(), cursor.field(child)?));
    }
    Ok(Instance::Group(fields.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undeclared_components_block_scalars_but_reach_a_later_composite() {
        let scalar = |name: &str| SchemaNode::scalar(name, ir::ScalarType::String);
        let schema = SchemaNode::group(
            "N9",
            vec![
                scalar("A"),
                scalar("B"),
                SchemaNode::group("C", vec![scalar("First"), scalar("Second")]),
            ],
        );
        let segment = format_edi::Segment {
            id: "N9".into(),
            elements: vec![
                vec![vec!["x".into(), "y".into()]],
                vec![vec!["later".into()]],
            ],
        };
        let value = project(&schema, &segment).unwrap();
        assert_eq!(
            value.field("A").unwrap().as_scalar(),
            Some(&Value::String("x".into()))
        );
        assert_eq!(value.field("B").unwrap().as_scalar(), Some(&Value::Null));
        let composite = value.field("C").unwrap();
        assert_eq!(
            composite.field("First").unwrap().as_scalar(),
            Some(&Value::Null)
        );
        assert_eq!(
            composite.field("Second").unwrap().as_scalar(),
            Some(&Value::String("y".into()))
        );
    }
}
