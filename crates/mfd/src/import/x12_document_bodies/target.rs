use super::*;
use mapping::{IterationOutput, ScopeSequence};

struct Driver {
    collection: Vec<String>,
    owner: Vec<String>,
    filter: Option<NodeId>,
}

fn entries<'a, 'input>(node: roxmltree::Node<'a, 'input>) -> Vec<roxmltree::Node<'a, 'input>> {
    node.children()
        .filter(|n| n.has_tag_name("entry"))
        .collect()
}
fn payload<'a, 'input>(
    mut node: roxmltree::Node<'a, 'input>,
    scalar: bool,
    format: X12DocumentBodyFormat,
) -> Result<Option<roxmltree::Node<'a, 'input>>, MfdError> {
    if format == X12DocumentBodyFormat::Xml {
        return Ok(Some(node));
    }
    for _ in 0..8 {
        let name = node.attribute("name").unwrap_or("");
        if node.attribute("type") != Some("json-property")
            && ((!scalar && name == "object")
                || (scalar && matches!(name, "string" | "number" | "boolean" | "null")))
        {
            return Ok(Some(node));
        }
        let children = entries(node);
        if children.is_empty() {
            return Ok(None);
        }
        let [child] = children.as_slice() else {
            return Err(refuse("JSON target has ambiguous value alternatives"));
        };
        if node.attribute("type") != Some("json-property")
            && !matches!(name, "root" | "array" | "item")
        {
            return Err(refuse("unqualified JSON target wrapper"));
        }
        node = *child;
    }
    Err(refuse("JSON target wrapper depth exceeded"))
}

impl Builder<'_> {
    fn driver(&mut self, feed: u32, anchor: &[String]) -> Result<Driver, MfdError> {
        self.driver_at(feed, anchor, 0)
    }

    fn driver_at(
        &mut self,
        feed: u32,
        anchor: &[String],
        depth: usize,
    ) -> Result<Driver, MfdError> {
        if depth > 128 {
            return Err(refuse("cyclic or excessively deep group driver"));
        }
        if let Some(path) = self.ports.get(&feed).cloned() {
            let node = schema_at(self.message, &path)
                .ok_or_else(|| refuse("unknown structured driver"))?;
            if !matches!(node.kind, SchemaKind::Group { .. }) {
                return Err(refuse("scalar source requires a mapped driver expression"));
            }
            let owner = self.owner(&path)?;
            let relative = owner
                .strip_prefix(anchor)
                .ok_or_else(|| refuse("group driver crosses source owner"))?
                .to_vec();
            // Keep the active frame on the repeated owner. Iterating its
            // singleton child would introduce an unpositioned legacy frame and
            // make subsequent owner-relative scalar reads refer to that child.
            // The child still controls construction: absent children yield no
            // target, without evaluating any mapped detail expressions.
            let filter = if path != owner {
                let count = self.alloc(Node::Aggregate {
                    function: mapping::AggregateOp::Count,
                    collection: path[owner.len()..].to_vec(),
                    value: vec![],
                    expression: None,
                    arg: None,
                });
                let zero = self.constant(Value::Int(0));
                Some(self.call("greater_than", vec![count, zero]))
            } else {
                None
            };
            return Ok(Driver {
                collection: relative,
                owner,
                filter,
            });
        }
        let f = self
            .functions
            .get(&feed)
            .ok_or_else(|| refuse("unknown group driver"))?
            .clone();
        if f.parsed.kind == 3 {
            if f.inputs.len() != 2 {
                return Err(refuse("filter requires data and predicate"));
            }
            let driver = self.driver_at(self.feed(f.inputs[0])?, anchor, depth + 1)?;
            let predicate = self.scalar(self.feed(f.inputs[1])?, &driver.owner)?;
            let no = self.no();
            let condition = if predicate.always {
                predicate.value
            } else {
                self.alloc(Node::If {
                    condition: predicate.present,
                    then: predicate.value,
                    else_: no,
                })
            };
            let condition = if let Some(prior) = driver.filter {
                self.call("and", vec![prior, condition])
            } else {
                condition
            };
            return Ok(Driver {
                filter: Some(condition),
                ..driver
            });
        }
        if self.volatile(feed, &mut BTreeSet::new())? {
            return Err(refuse("volatile group drivers are not admitted"));
        }
        let owner = self
            .external_owner(feed, anchor)?
            .unwrap_or_else(|| anchor.to_vec());
        let value = self.scalar(feed, &owner)?;
        let condition = self.call("exists", vec![value.value]);
        Ok(Driver {
            collection: owner[anchor.len()..].to_vec(),
            owner,
            filter: Some(condition),
        })
    }

    pub(super) fn target_scope(
        &mut self,
        entry: roxmltree::Node<'_, '_>,
        schema: &SchemaNode,
        path: &[String],
        anchor: &[String],
        format: X12DocumentBodyFormat,
    ) -> Result<Scope, MfdError> {
        let original_entry = entry;
        let entry = payload(entry, false, format)?
            .ok_or_else(|| refuse("target group lacks its value wrapper"))?;
        if !matches!(schema.kind, SchemaKind::Group { .. }) {
            return Err(refuse("group target has scalar schema"));
        }
        self.check_entry(entry)?;
        let mut current = anchor.to_vec();
        let mut scope = Scope {
            target_field: schema.name.clone(),
            ..Default::default()
        };
        let mut driver_input = entry.attribute("inpkey");
        if format == X12DocumentBodyFormat::Json {
            // A repeated JSON value may carry its driver on the array wrapper
            // instead of the object payload. Unwrapping must not discard that
            // connector or turn an empty source collection into a singleton.
            for wrapper in entry
                .ancestors()
                .skip(1)
                .take_while(|node| Some(*node) != original_entry.parent())
            {
                let Some(input) = wrapper.attribute("inpkey") else {
                    continue;
                };
                self.check_entry(wrapper)?;
                if !schema.repeating
                    || wrapper.attribute("name") != Some("array")
                    || wrapper.attribute("type") == Some("json-property")
                {
                    return Err(refuse("unsupported JSON target wrapper driver"));
                }
                if driver_input.replace(input).is_some() {
                    return Err(refuse(
                        "multiple JSON target drivers require separate admission",
                    ));
                }
            }
        }
        if let Some(input) = driver_input {
            let input = number(Some(input))?;
            self.consumed_target_inputs.insert(input);
            let driver = self.driver(self.feed(Some(input))?, anchor)?;
            scope.iteration = ScopeIteration::Source(driver.collection);
            scope.filter = driver.filter;
            current = driver.owner;
            if !schema.repeating {
                scope.iteration_output = IterationOutput::MappedSequence;
            }
        }
        let mut positions = BTreeMap::<String, usize>::new();
        let mut saw_child = false;
        for child in entries(entry) {
            self.check_entry(child)?;
            let name = child
                .attribute("name")
                .ok_or_else(|| refuse("unnamed target child"))?;
            let target = schema
                .child(name)
                .ok_or_else(|| refuse(format!("target {name} is absent from its schema")))?;
            let mut child_path = path.to_vec();
            child_path.push(name.into());
            let next = match &target.kind {
                SchemaKind::Scalar { ty } => {
                    let Some(value) = payload(child, true, format)? else {
                        continue;
                    };
                    self.check_entry(value)?;
                    let Some(input) = value.attribute("inpkey") else {
                        continue;
                    };
                    let input = number(Some(input))?;
                    self.consumed_target_inputs.insert(input);
                    let feed = self.feed(Some(input))?;
                    if format == X12DocumentBodyFormat::Xml {
                        if let Some(owner) = self.external_owner(feed, &current)? {
                            let expr = self.scalar(feed, &owner)?;
                            let expr = self.cast(expr, *ty)?;
                            Some(Scope {
                                target_field: name.into(),
                                iteration: ScopeIteration::Source(owner[current.len()..].to_vec()),
                                construction: ScopeConstruction::Scalar { value: expr.value },
                                filter: (!expr.always).then_some(expr.present),
                                iteration_output: if target.repeating {
                                    IterationOutput::Repeated
                                } else {
                                    IterationOutput::MappedSequence
                                },
                                ..Default::default()
                            })
                        } else {
                            if saw_child && target.repeating {
                                return Err(refuse(
                                    "repeating scalar after a child requires explicit target-order lowering",
                                ));
                            }
                            let expr = self.reduced(feed, &current, *ty)?;
                            if saw_child {
                                // A non-iterating scalar child uses the same
                                // source frame and preserves Null as a value.
                                // It runs in declaration order after earlier
                                // children, rather than before them as a binding.
                                Some(Scope {
                                    target_field: name.into(),
                                    construction: ScopeConstruction::Scalar { value: expr.value },
                                    ..Default::default()
                                })
                            } else {
                                scope.bindings.push(Binding {
                                    target_field: name.into(),
                                    node: expr.value,
                                });
                                None
                            }
                        }
                    } else {
                        if saw_child {
                            return Err(refuse(
                                "JSON scalar after a group requires explicit target-order lowering",
                            ));
                        }
                        let expr = self.reduced(feed, &current, *ty)?;
                        scope.bindings.push(Binding {
                            target_field: name.into(),
                            node: expr.value,
                        });
                        None
                    }
                }
                SchemaKind::Group { .. } => {
                    if payload(child, false, format)?.is_none() {
                        continue;
                    }
                    let next = self.target_scope(child, target, &child_path, &current, format)?;
                    if next.bindings.is_empty() && next.children.is_empty() {
                        continue;
                    }
                    Some(next)
                }
                SchemaKind::ScalarUnion { .. } => {
                    return Err(refuse("scalar-union target requires separate admission"));
                }
            };
            if let Some(mut next) = next {
                saw_child = true;
                if let Some(&index) = positions.get(name) {
                    let prior = &mut scope.children[index];
                    let mut first = prior.clone();
                    first.target_field.clear();
                    next.target_field.clear();
                    let mut parts = match first.iteration {
                        ScopeIteration::Concatenate(ref sequence) => {
                            sequence.iter().cloned().collect::<Vec<_>>()
                        }
                        _ => vec![first],
                    };
                    parts.push(next);
                    if parts.len() > 16 {
                        return Err(refuse("too many target clones"));
                    }
                    let first = parts.remove(0);
                    *prior = Scope {
                        target_field: name.into(),
                        iteration: ScopeIteration::Concatenate(ScopeSequence::new(first, parts)),
                        iteration_output: if target.repeating {
                            IterationOutput::Repeated
                        } else {
                            IterationOutput::MappedSequence
                        },
                        ..Default::default()
                    };
                } else {
                    positions.insert(name.into(), scope.children.len());
                    scope.children.push(next);
                }
            }
        }
        if schema.repeating && matches!(scope.iteration, ScopeIteration::None) {
            scope.iteration = ScopeIteration::Source(vec![]);
        }
        Ok(scope)
    }
    fn check_entry(&self, entry: roxmltree::Node<'_, '_>) -> Result<(), MfdError> {
        if entry.attributes().any(|a| {
            !matches!(
                a.name(),
                "name" | "inpkey" | "outkey" | "ns" | "type" | "expanded" | "clone"
            )
        }) || entry
            .children()
            .filter(|n| n.is_element())
            .any(|n| !n.has_tag_name("entry"))
            || entry.attribute("clone").is_some_and(|v| v != "1")
        {
            return Err(refuse("unsupported target entry metadata"));
        }
        Ok(())
    }
}
