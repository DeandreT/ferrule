use super::*;

pub(super) struct ParameterObject {
    pub source: mapping::NamedSource,
    pub members: BTreeMap<u32, String>,
}

pub(super) fn source_name(parameter: &ImportedJsonParameterProjection) -> String {
    format!("__ferrule_parameter_object_{}", parameter.input_port)
}

impl Builder {
    pub(super) fn bind_parameter_object(
        &mut self,
        filename: u32,
        parameter: &ImportedJsonParameterProjection,
    ) -> Result<mapping::JsonParameterInvocationPlan, MfdError> {
        fn visit(
            builder: &Builder,
            parameter: &ImportedJsonParameterProjection,
            feed: u32,
            active: &mut BTreeSet<u32>,
            drivers: &mut BTreeSet<u32>,
            groups: &mut BTreeMap<u32, (String, u32)>,
        ) -> Result<(), MfdError> {
            if builder.ports.contains_key(&feed) || builder.filename_port == Some(feed) {
                return Ok(());
            }
            if let Some(path) = parameter.ports.get(&feed) {
                if path.len() != 3 || path[..2] != ["Documents", "Root"] {
                    return Err(refuse(
                        "outer occurrence driver must be a root String member",
                    ));
                }
                drivers.insert(feed);
                return Ok(());
            }
            if !active.insert(feed) {
                return Err(refuse("cyclic parameter occurrence filename"));
            }
            if let Some(call) = builder.udf_outputs.get(&feed) {
                for input in call.inputs.values() {
                    visit(
                        builder,
                        parameter,
                        builder.feed(*input)?,
                        active,
                        drivers,
                        groups,
                    )?;
                }
            } else {
                let function = builder
                    .functions
                    .get(&feed)
                    .ok_or_else(|| refuse("unknown parameter filename owner"))?;
                if function.parsed.library == "core"
                    && function.parsed.name == "group-by"
                    && parameter.ports.contains_key(&builder.input(function, 0)?)
                {
                    let source = builder.input(function, 0)?;
                    let key = builder.input(function, 1)?;
                    let literal = builder
                        .functions
                        .get(&key)
                        .ok_or_else(|| refuse("parameter object group needs a literal key"))?;
                    let text = literal
                        .parsed
                        .constant
                        .as_ref()
                        .filter(|(_, ty)| ty == "string")
                        .map(|(text, _)| text.clone())
                        .ok_or_else(|| refuse("parameter group key must be String"))?;
                    if function.parsed.kind != 5
                        || !function.metadata_valid
                        || !matches!(function.parsed.output_pins.as_slice(), [None, Some(port)] if *port == feed)
                        || function.inputs.len() != 2
                        || parameter.ports[&source] != ["Documents", "Root"]
                        || literal.parsed.kind != 2
                        || literal.parsed.library != "core"
                        || !literal.literal_valid
                        || !literal.metadata_valid
                        || !literal.inputs.is_empty()
                    {
                        return Err(refuse("unqualified constant parameter object group"));
                    }
                    groups.insert(feed, (text, function.inputs[0].unwrap()));
                } else if function.parsed.kind != 6 {
                    for input in function.inputs.iter().flatten() {
                        visit(
                            builder,
                            parameter,
                            builder.feed(*input)?,
                            active,
                            drivers,
                            groups,
                        )?;
                    }
                }
            }
            active.remove(&feed);
            Ok(())
        }
        let mut drivers = BTreeSet::new();
        let mut groups = BTreeMap::new();
        visit(
            self,
            parameter,
            filename,
            &mut BTreeSet::new(),
            &mut drivers,
            &mut groups,
        )?;
        if drivers.len() != 1 || groups.len() != 1 {
            return Err(refuse(
                "requires one String occurrence driver and one constant parameter group",
            ));
        }
        let driver = *drivers.first().unwrap();
        let member = parameter.ports[&driver][2].clone();
        let mapping::JsonSourceShape::Object { properties } = &parameter.shape else {
            return Err(refuse("parameter root must be an object"));
        };
        let members = parameter
            .ports
            .iter()
            .filter(|(_, path)| {
                path.len() == 3
                    && properties.iter().any(|property| {
                        property.name == path[2]
                            && property.shape == mapping::JsonSourceShape::String
                    })
            })
            .map(|(port, path)| (*port, path[2].clone()))
            .collect::<BTreeMap<_, _>>();
        if !members.contains_key(&driver) {
            return Err(refuse("occurrence driver must have a typed String output"));
        }
        let binding = format!("__ferrule_parameter_driver_{driver}");
        let name = source_name(parameter);
        fn contains_name(schema: &SchemaNode, name: &str) -> bool {
            schema.name == name
                || match &schema.kind {
                    SchemaKind::Group { children, .. } => {
                        children.iter().any(|child| contains_name(child, name))
                    }
                    _ => false,
                }
        }
        if contains_name(&self.source_schema, &name) {
            return Err(refuse(
                "parameter source name is shadowed by physical source data",
            ));
        }
        if self.functions.values().any(|function| {
            function
                .parsed
                .input_parameter_name
                .as_ref()
                .is_some_and(|parameter| parameter.name == binding || parameter.name == name)
        }) {
            return Err(refuse(
                "parameter object binding collides with a declared input",
            ));
        }
        let root = parameter
            .ports
            .iter()
            .find_map(|(port, path)| (path == &["Documents", "Root"]).then_some(*port))
            .ok_or_else(|| refuse("missing parameter root identity"))?;
        for (input, feed) in &self.feeds {
            if *feed == root && !groups.values().any(|(_, allowed)| allowed == input) {
                return Err(refuse(
                    "parameter root has an unsupported structural consumer",
                ));
            }
        }
        let plan = mapping::JsonParameterInvocationPlan {
            parameter: parameter.parameter_name.clone(),
            key_member: member,
            iteration: mapping::JsonParameterIteration::RootStringOccurrences,
            key_parameter: binding.clone(),
            projection: parameter.shape.clone(),
        };
        plan.validate().map_err(|error| refuse(error.to_string()))?;
        let node = self.alloc(Node::RuntimeParameter {
            name: binding,
            ty: ScalarType::String,
            preview: None,
        })?;
        self.parameter_key = Some((driver, node));
        for (output, (text, _)) in &groups {
            let node = self.constant(Value::String(text.clone()))?;
            self.parameter_constants.insert(*output, node);
        }
        self.parameter_object = Some(ParameterObject {
            source: mapping::NamedSource {
                name: name.clone(),
                path: String::new(),
                schema: parameter
                    .shape
                    .value_schema(&name)
                    .map_err(|error| refuse(error.to_string()))?,
                options: Default::default(),
                dynamic_path: None,
            },
            members,
        });
        self.propagate_parameter_dependencies(
            std::iter::once(driver).chain(groups.keys().copied()),
        );
        Ok(plan)
    }

    pub(super) fn propagate_parameter_dependencies(
        &mut self,
        seeds: impl IntoIterator<Item = u32>,
    ) {
        let mut users = BTreeMap::<_, Vec<_>>::new();
        for (output, function) in &self.functions {
            for input in function.inputs.iter().flatten() {
                if let Some(feed) = self.feeds.get(input) {
                    users.entry(*feed).or_default().push(*output);
                }
            }
        }
        for (output, call) in &self.udf_outputs {
            for input in call.inputs.values() {
                if let Some(feed) = self.feeds.get(input) {
                    users.entry(*feed).or_default().push(*output);
                }
            }
        }
        let mut pending = seeds.into_iter().collect::<Vec<_>>();
        self.parameter_dependent_feeds
            .extend(pending.iter().copied());
        while let Some(feed) = pending.pop() {
            for user in users.get(&feed).into_iter().flatten() {
                if self.parameter_dependent_feeds.insert(*user) {
                    pending.push(*user);
                }
            }
        }
    }

    pub(super) fn parameter_field(&mut self, feed: u32) -> Result<Option<NodeId>, MfdError> {
        let Some(parameter) = &self.parameter_object else {
            return Ok(None);
        };
        let Some(member) = parameter.members.get(&feed).cloned() else {
            return Ok(None);
        };
        let frame = vec![parameter.source.name.clone(), member];
        self.alloc(Node::SourceField {
            path: vec!["Value".into()],
            frame: Some(frame),
        })
        .map(Some)
    }

    pub(super) fn reduce_parameter_field(
        &mut self,
        feed: u32,
        value: NodeId,
        retain_empty: bool,
    ) -> Result<NodeId, MfdError> {
        let parameter = self
            .parameter_object
            .as_ref()
            .ok_or_else(|| refuse("missing parameter owner"))?;
        let collection = vec![
            parameter.source.name.clone(),
            parameter.members[&feed].clone(),
        ];
        let value = self.call("string", vec![value])?;
        let arg = if retain_empty {
            Some(self.constant(Value::Int(1))?)
        } else {
            None
        };
        self.alloc(Node::Aggregate {
            function: if retain_empty {
                AggregateOp::ItemAt
            } else {
                AggregateOp::FirstNonemptyText
            },
            collection,
            value: vec![],
            expression: Some(value),
            arg,
        })
    }

    pub(super) fn parameter_decimal_binding(&mut self, feed: u32) -> Result<NodeId, MfdError> {
        let raw = self
            .parameter_field(feed)?
            .ok_or_else(|| refuse("missing parameter field"))?;
        let number = self.call("decimal_from_nullable_text", vec![raw])?;
        let selected = self.reduce_parameter_field(feed, number, false)?;
        let present = self.call("exists", vec![selected])?;
        let converted = self.call("decimal_from_nullable_text", vec![selected])?;
        let absent = self.constant(Value::Null)?;
        self.alloc(Node::If {
            condition: present,
            then: converted,
            else_: absent,
        })
    }
}
