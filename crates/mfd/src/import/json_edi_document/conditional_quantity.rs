//! A bounded native integer quantity expression with separate local and writer reductions.
use super::*;

#[derive(Clone, Copy)]
struct Quantity {
    guard: u32,
    condition: u32,
    quantity: u32,
    divisor: u32,
}

impl Builder {
    fn quantity_signature(&self, feed: u32) -> Option<Quantity> {
        self.parameter_object.as_ref()?;
        let exact = |feed, name: &str, kind, arity| {
            self.functions.get(&feed).filter(|function| {
                function.parsed.library == "core"
                    && function.parsed.name == name
                    && function.parsed.kind == kind
                    && function.metadata_valid
                    && function.parsed.raw_single_output_valid
                    && function.inputs.len() == arity
                    && function.inputs.iter().all(Option::is_some)
            })
        };
        let branch = exact(feed, "if-else", 4, 3)?;
        let condition = self.input(branch, 0).ok()?;
        let equality = exact(condition, "equal", 5, 2)?;
        let guard = self.input(equality, 0).ok()?;
        let literal = self.functions.get(&self.input(equality, 1).ok()?)?;
        if self.port_kinds.get(&guard).map(String::as_str) != Some("string")
            || literal.parsed.library != "core"
            || literal.parsed.kind != 2
            || !literal.literal_valid
            || !literal.metadata_valid
            || !literal.inputs.is_empty()
            || !literal
                .parsed
                .constant
                .as_ref()
                .is_some_and(|(_, ty)| ty == "string")
        {
            return None;
        }
        let floor = exact(self.input(branch, 1).ok()?, "floor", 5, 1)?;
        let divide = exact(self.input(floor, 0).ok()?, "divide", 5, 2)?;
        let quantity = self.input(divide, 0).ok()?;
        let divisor = self.input(divide, 1).ok()?;
        if self.input(branch, 2).ok()? != quantity
            || quantity == divisor
            || !self.integer_ports.contains(&quantity)
            || !self.integer_ports.contains(&divisor)
        {
            return None;
        }
        let owner = self.ports.get(&guard)?.split_last()?.1;
        if owner.is_empty()
            || [quantity, divisor].iter().any(|port| {
                self.port_kinds.get(port).map(String::as_str) != Some("number")
                    || self
                        .ports
                        .get(port)
                        .and_then(|path| path.split_last())
                        .map(|(_, parent)| parent)
                        != Some(owner)
            })
        {
            return None;
        }
        Some(Quantity {
            guard,
            condition,
            quantity,
            divisor,
        })
    }

    fn quantity_for_guard(&mut self, plan: Quantity) -> Result<NodeId, MfdError> {
        let guard_path = self.ports[&plan.guard].clone();
        let quantity_path = self.ports[&plan.quantity].clone();
        let raw_quantity = self.alloc(Node::SourceField {
            path: vec!["Value".into()],
            frame: Some(quantity_path.clone()),
        })?;
        let quantity = self.numeric_source_value(plan.quantity, raw_quantity)?;
        let raw_divisor = self.alloc(Node::SourceField {
            path: vec!["Value".into()],
            frame: Some(self.ports[&plan.divisor].clone()),
        })?;
        let divisor = self.numeric_source_value(plan.divisor, raw_divisor)?;
        let quotient = self.call("mapforce_integer_quotient_floor", vec![quantity, divisor])?;
        let quotient = self.call("string", vec![quotient])?;
        let divisors = self.alloc(Node::Aggregate {
            function: AggregateOp::LastPresentText,
            collection: self.sibling_path(plan.divisor, &quantity_path)?,
            value: vec![],
            expression: Some(quotient),
            arg: None,
        })?;
        let divided = self.alloc(Node::Aggregate {
            function: AggregateOp::LastPresentText,
            collection: self.sibling_path(plan.quantity, &guard_path)?,
            value: vec![],
            expression: Some(divisors),
            arg: None,
        })?;
        let quantity_text = self.call("string", vec![quantity])?;
        let undivided = self.alloc(Node::Aggregate {
            function: AggregateOp::LastPresentText,
            collection: self.sibling_path(plan.quantity, &guard_path)?,
            value: vec![],
            expression: Some(quantity_text),
            arg: None,
        })?;
        let condition = self.compile(plan.condition, Some(plan.guard), &guard_path)?;
        self.alloc(Node::If {
            condition,
            then: divided,
            else_: undivided,
        })
    }

    pub(super) fn conditional_quantity_value(
        &mut self,
        feed: u32,
        active_leaf: Option<u32>,
        anchor: &[String],
    ) -> Result<Option<NodeId>, MfdError> {
        let sum = self.functions.get(&feed).filter(|function| {
            function.parsed.library == "core"
                && function.parsed.name == "sum"
                && function.parsed.kind == 5
                && function.metadata_valid
                && function.parsed.raw_single_output_valid
                && matches!(function.inputs.as_slice(), [None, Some(_)])
        });
        let input = sum.map(|sum| self.input(sum, 1)).transpose()?;
        let Some(plan) = self.quantity_signature(input.unwrap_or(feed)) else {
            return Ok(None);
        };
        let selected = self.quantity_for_guard(plan)?;
        if input.is_none() && active_leaf == Some(plan.guard) {
            return Ok(Some(selected));
        }
        let expression = if input.is_some() {
            self.call("decimal_from_nullable_text", vec![selected])?
        } else {
            selected
        };
        self.alloc(Node::Aggregate {
            function: if input.is_some() {
                AggregateOp::DecimalSum
            } else {
                AggregateOp::FirstNonemptyText
            },
            collection: self.relative_source(&self.ports[&plan.guard], anchor)?,
            value: vec![],
            expression: Some(expression),
            arg: None,
        })
        .map(Some)
    }
}
