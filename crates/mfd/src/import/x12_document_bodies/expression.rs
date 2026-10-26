use super::*;

impl Builder<'_> {
    fn sequence_presence_guard(&self, function: &Function) -> Option<u32> {
        if function.parsed.kind != 4 || function.inputs.len() != 3 || function.inputs[2].is_some() {
            return None;
        }
        let selected = self.feed(function.inputs[1]).ok()?;
        let condition = self.functions.get(&self.feed(function.inputs[0]).ok()?)?;
        (condition.parsed.library == "db"
            && condition.parsed.name == "is-not-null"
            && condition.inputs.len() == 1
            && self.feed(condition.inputs[0]).ok() == Some(selected))
        .then_some(selected)
    }

    fn filtered_source_sequence(&self, feed: u32) -> bool {
        // JSON object construction coerces this sequence before formatting.
        // XML keeps its eager per-occurrence conversion, including later errors.
        if self.format != X12DocumentBodyFormat::Json {
            return false;
        }
        let Some(function) = self.functions.get(&feed) else {
            return false;
        };
        if function.parsed.kind != 4 || function.inputs.len() != 3 || function.inputs[2].is_some() {
            return false;
        }
        let Ok(value) = self.feed(function.inputs[1]) else {
            return false;
        };
        let Ok(condition) = self.feed(function.inputs[0]) else {
            return false;
        };
        let Some(condition) = self.functions.get(&condition) else {
            return false;
        };
        self.ports.contains_key(&value)
            && condition.parsed.library == "core"
            && condition.parsed.name == "equal"
            && condition.inputs.len() == 2
            && condition.inputs.iter().any(|input| {
                self.feed(*input)
                    .ok()
                    .and_then(|feed| self.literal_text(feed))
                    .is_some()
            })
    }

    pub(super) fn alloc(&mut self, node: Node) -> NodeId {
        let id = self.next;
        self.next += 1;
        self.graph.nodes.insert(id, node);
        id
    }
    pub(super) fn constant(&mut self, value: Value) -> NodeId {
        self.alloc(Node::Const { value })
    }
    pub(super) fn call(&mut self, function: &str, args: Vec<NodeId>) -> NodeId {
        self.alloc(Node::Call {
            function: function.into(),
            args,
        })
    }
    pub(super) fn yes(&mut self) -> NodeId {
        self.constant(Value::Bool(true))
    }
    pub(super) fn no(&mut self) -> NodeId {
        self.constant(Value::Bool(false))
    }
    pub(super) fn missing(&mut self) -> Expr {
        let value = self.constant(Value::Null);
        let present = self.no();
        Expr {
            value,
            present,
            always: false,
        }
    }
    pub(super) fn present_value(&mut self, value: NodeId) -> Expr {
        let present = self.yes();
        Expr {
            value,
            present,
            always: true,
        }
    }
    pub(super) fn guard(&mut self, value: NodeId, present: NodeId, always: bool) -> Expr {
        let value = if always {
            value
        } else {
            let absent = self.constant(Value::Null);
            self.alloc(Node::If {
                condition: present,
                then: value,
                else_: absent,
            })
        };
        Expr {
            value,
            present,
            always,
        }
    }
    pub(super) fn feed(&self, input: Option<u32>) -> Result<u32, MfdError> {
        input
            .and_then(|key| self.feeds.get(&key).copied())
            .ok_or_else(|| refuse("unconnected required function input"))
    }
    pub(super) fn dependencies(
        &self,
        feed: u32,
        seen: &mut BTreeSet<u32>,
    ) -> Result<BTreeSet<u32>, MfdError> {
        if self.ports.contains_key(&feed) {
            return Ok(BTreeSet::from([feed]));
        }
        if !seen.insert(feed) || seen.len() > 128 {
            return Err(refuse("cyclic or excessively deep expression"));
        }
        let f = self
            .functions
            .get(&feed)
            .ok_or_else(|| refuse(format!("unknown source output {feed}")))?;
        let mut result = BTreeSet::new();
        if f.parsed.kind != 6 {
            for input in f.inputs.iter().flatten() {
                result.extend(self.dependencies(self.feed(Some(*input))?, seen)?);
            }
        }
        seen.remove(&feed);
        Ok(result)
    }
    pub(super) fn volatile(&self, feed: u32, seen: &mut BTreeSet<u32>) -> Result<bool, MfdError> {
        if self.ports.contains_key(&feed) {
            return Ok(false);
        }
        if !seen.insert(feed) || seen.len() > 128 {
            return Err(refuse("cyclic or excessively deep volatile expression"));
        }
        let f = self
            .functions
            .get(&feed)
            .ok_or_else(|| refuse("unknown volatile owner"))?;
        if matches!(
            f.parsed.name.as_str(),
            "now" | "create-guid" | "auto-number"
        ) {
            seen.remove(&feed);
            return Ok(true);
        }
        let mut result = false;
        if f.parsed.kind != 6 {
            for input in f.inputs.iter().flatten() {
                result |= self.volatile(self.feed(Some(*input))?, seen)?;
            }
        }
        seen.remove(&feed);
        Ok(result)
    }
    pub(super) fn owner(&self, path: &[String]) -> Result<Vec<String>, MfdError> {
        let mut current = self.message;
        let mut prefix = Vec::new();
        let mut owner = Vec::new();
        for part in path {
            current = current
                .child(part)
                .ok_or_else(|| refuse("source port differs from physical schema"))?;
            prefix.push(part.clone());
            if current.repeating {
                owner = prefix.clone();
            }
        }
        Ok(owner)
    }
    pub(super) fn external_owner(
        &self,
        feed: u32,
        anchor: &[String],
    ) -> Result<Option<Vec<String>>, MfdError> {
        if let Some(function) = self.functions.get(&feed)
            && (self.sequence_presence_guard(function).is_some()
                || (function.parsed.library == "core"
                    && function.parsed.name == "format-dateTime"
                    && function
                        .inputs
                        .first()
                        .and_then(|input| self.feed(*input).ok())
                        .is_some_and(|input| self.filtered_source_sequence(input))))
        {
            return Ok(None);
        }
        let mut selected: Option<Vec<String>> = None;
        for key in self.dependencies(feed, &mut BTreeSet::new())? {
            let owner = self.owner(&self.ports[&key])?;
            if anchor.starts_with(&owner) {
                continue;
            }
            if !owner.starts_with(anchor) {
                return Err(refuse("expression crosses independent source owners"));
            }
            if let Some(prior) = &selected
                && !prior.starts_with(&owner)
                && !owner.starts_with(prior)
            {
                return Err(refuse("expression combines independent sequences"));
            }
            if selected.as_ref().is_none_or(|p| owner.len() > p.len()) {
                selected = Some(owner);
            }
        }
        Ok(selected)
    }
    fn source(&mut self, feed: u32, anchor: &[String]) -> Result<Expr, MfdError> {
        let path = self.ports[&feed].clone();
        if !matches!(
            schema_at(self.message, &path).map(|n| &n.kind),
            Some(SchemaKind::Scalar { .. })
        ) {
            return Err(refuse("structured source used as a scalar"));
        }
        let owner = self.owner(&path)?;
        if !anchor.starts_with(&owner) {
            return Err(refuse("scalar needs an explicit sequence owner"));
        }
        let frame = (!owner.is_empty()).then_some(owner.clone());
        let local = path[owner.len()..].to_vec();
        let raw = if owner.is_empty() && self.nullable_input {
            // Pin the immutable projected message, not the current row or a
            // same-named nested group. The existing owned read preserves Null
            // for missing fields; required fields still raise below.
            self.alloc(Node::OwnedSourceField(mapping::OwnedSourceField {
                source: mapping::OwnedSourceRef {
                    owner: mapping::SourceOwner::Primary,
                    path: local,
                },
                frame: Some(vec![]),
            }))
        } else {
            self.alloc(Node::SourceField { path: local, frame })
        };
        let present = self.call("exists", vec![raw]);
        if (owner.len() + 1..=path.len()).all(|end| self.required.contains(&path[..end])) {
            let message = self.constant(Value::String("required source field is absent".into()));
            let missing = self.alloc(Node::Raise {
                message: Some(message),
            });
            let value = self.alloc(Node::If {
                condition: present,
                then: raw,
                else_: missing,
            });
            Ok(self.present_value(value))
        } else {
            Ok(Expr {
                value: raw,
                present,
                always: false,
            })
        }
    }
    fn literal_text(&self, feed: u32) -> Option<&str> {
        self.functions
            .get(&feed)
            .filter(|f| f.parsed.kind == 2)
            .and_then(|f| f.parsed.constant.as_ref())
            .filter(|(_, ty)| ty == "string")
            .map(|(v, _)| v.as_str())
    }
    fn input(&mut self, f: &Function, index: usize, anchor: &[String]) -> Result<Expr, MfdError> {
        self.scalar(self.feed(f.inputs.get(index).copied().flatten())?, anchor)
    }
    fn nullable_scalar(&mut self, feed: u32, anchor: &[String]) -> Result<Expr, MfdError> {
        // Admission belongs to this consumer's input expression, not to a file
        // or target containing some unrelated nullable scalar component.
        let previous = self.nullable_input;
        self.nullable_input = true;
        let result = self.scalar(feed, anchor);
        self.nullable_input = previous;
        result
    }
    fn nullable_argument(&mut self, f: &Function, anchor: &[String]) -> Result<Expr, MfdError> {
        self.nullable_scalar(self.feed(f.inputs.first().copied().flatten())?, anchor)
    }
    fn all_present(&mut self, args: &[Expr]) -> (NodeId, bool) {
        if args.iter().all(|a| a.always) {
            return (self.yes(), true);
        }
        let mut present = self.yes();
        for arg in args {
            if !arg.always {
                present = self.call("and", vec![present, arg.present]);
            }
        }
        (present, false)
    }
    pub(super) fn scalar(&mut self, feed: u32, anchor: &[String]) -> Result<Expr, MfdError> {
        let key = (feed, anchor.to_vec(), self.nullable_input);
        if let Some(value) = self.cache.get(&key) {
            return Ok(*value);
        }
        if !self.active.insert(key.clone()) || self.active.len() > 128 {
            return Err(refuse("cyclic or excessively deep scalar expression"));
        }
        let result = self.scalar_inner(feed, anchor);
        self.active.remove(&key);
        let result = result?;
        self.cache.insert(key, result);
        Ok(result)
    }
    fn scalar_inner(&mut self, feed: u32, anchor: &[String]) -> Result<Expr, MfdError> {
        if self.ports.contains_key(&feed) {
            return self.source(feed, anchor);
        }
        let f = self
            .functions
            .get(&feed)
            .ok_or_else(|| refuse("unknown scalar output"))?
            .clone();
        if f.parsed.kind == 2 {
            let (value, ty) = f
                .parsed
                .constant
                .as_ref()
                .ok_or_else(|| refuse("missing constant"))?;
            let value = match ty.as_str() {
                "string" => Value::String(value.clone()),
                "decimal" => value
                    .parse::<i64>()
                    .map(Value::Int)
                    .unwrap_or(Value::decimal(
                        ir::Decimal96::parse_exact(value).map_err(|e| refuse(e.to_string()))?,
                    )),
                _ => return Err(refuse("unsupported constant type")),
            };
            let value = self.constant(value);
            return Ok(self.present_value(value));
        }
        if f.parsed.kind == 6 {
            let p = f
                .parsed
                .input_parameter_name
                .as_ref()
                .filter(|p| !p.optional)
                .ok_or_else(|| refuse("unsupported optional parameter"))?;
            if f.inputs != [None] || f.parsed.input_type != Some(ScalarType::String) {
                return Err(refuse("requires a simple String parameter"));
            }
            let value = self.alloc(Node::RuntimeParameter {
                name: p.name.clone(),
                ty: ScalarType::String,
                preview: f.parsed.input_preview.clone(),
            });
            return Ok(self.present_value(value));
        }
        if f.parsed.kind == 23 {
            let input_feed = self.feed(f.inputs[0])?;
            // Presence evaluates the map result as well as its value. Until a
            // single-evaluation binding is available, never duplicate volatility.
            if self.volatile(input_feed, &mut BTreeSet::new())? {
                return Err(refuse("volatile value-map input is not admitted"));
            }
            let input = self.nullable_scalar(input_feed, anchor)?;
            let map = f
                .parsed
                .valuemap
                .as_ref()
                .ok_or_else(|| refuse("missing value-map data"))?;
            let value = self.alloc(Node::ValueMap {
                input: input.value,
                input_type: map.input_type,
                table: map.table.clone(),
                default: None,
            });
            let present = self.call("exists", vec![value]);
            return Ok(Expr {
                value,
                present,
                always: false,
            });
        }
        if f.parsed.kind == 4 {
            if f.inputs.len() != 3 {
                return Err(refuse("requires one conditional branch"));
            }
            if let Some(input) = self.sequence_presence_guard(&f)
                && let Some(owner) = self.external_owner(input, anchor)?
            {
                if self.volatile(input, &mut BTreeSet::new())? {
                    return Err(refuse("volatile sequence presence guard is not admitted"));
                }
                let value = self.scalar(input, &owner)?;
                let text = self.call("string", vec![value.value]);
                let text = self.guard(text, value.present, value.always);
                let value = self.alloc(Node::Aggregate {
                    function: mapping::AggregateOp::LastPresentText,
                    collection: owner[anchor.len()..].to_vec(),
                    value: vec![],
                    expression: Some(text.value),
                    arg: None,
                });
                let present = self.call("exists", vec![value]);
                return Ok(Expr {
                    value,
                    present,
                    always: false,
                });
            }
            let condition_feed = self.feed(f.inputs[0])?;
            if self.volatile(condition_feed, &mut BTreeSet::new())? {
                return Err(refuse("volatile conditional presence is not admitted"));
            }
            let condition = self.scalar(condition_feed, anchor)?;
            let then = self.input(&f, 1, anchor)?;
            let otherwise = if let Some(input) = f.inputs[2] {
                self.scalar(self.feed(Some(input))?, anchor)?
            } else {
                self.missing()
            };
            let choose = if condition.always {
                condition.value
            } else {
                let no = self.no();
                self.alloc(Node::If {
                    condition: condition.present,
                    then: condition.value,
                    else_: no,
                })
            };
            let present = self.alloc(Node::If {
                condition: choose,
                then: then.present,
                else_: otherwise.present,
            });
            let value = self.alloc(Node::If {
                condition: choose,
                then: then.value,
                else_: otherwise.value,
            });
            return Ok(self.guard(
                value,
                present,
                condition.always && then.always && otherwise.always,
            ));
        }
        match (f.parsed.library.as_str(), f.parsed.name.as_str()) {
            ("lang", "now") if f.inputs.is_empty() => {
                let value = self.alloc(Node::RuntimeValue {
                    value: mapping::RuntimeValue::CurrentDateTime,
                });
                Ok(self.present_value(value))
            }
            ("lang", "create-guid") if f.inputs.is_empty() => {
                let value = self.call("create_guid", vec![]);
                Ok(self.present_value(value))
            }
            ("lang", "format-guid-string") if f.inputs.len() == 1 => {
                let input = self.nullable_argument(&f, anchor)?;
                let value = self.call("format_guid_string", vec![input.value]);
                Ok(self.guard(value, input.present, input.always))
            }
            ("db", "is-not-null") if f.inputs.len() == 1 => {
                let input = self.nullable_argument(&f, anchor)?;
                // Read the value, not only its presence flag: reached required
                // fields must still raise, and missing values yield present false.
                let value = self.call("exists", vec![input.value]);
                Ok(self.present_value(value))
            }
            ("core", "auto-number") if f.inputs == [None, None, None, None] => {
                let start = self.constant(Value::Int(1));
                let step = self.constant(Value::Int(1));
                let value = self.alloc(Node::InvocationCounter {
                    key: format!("{}:{}", self.target, f.uid),
                    start,
                    step,
                    restart: None,
                });
                Ok(self.present_value(value))
            }
            ("db", "substitute-null") if f.inputs.len() == 2 => {
                let left = self.input(&f, 0, anchor)?;
                let right = self.input(&f, 1, anchor)?;
                let present = if left.always || right.always {
                    self.yes()
                } else {
                    self.call("or", vec![left.present, right.present])
                };
                let value = self.alloc(Node::If {
                    condition: left.present,
                    then: left.value,
                    else_: right.value,
                });
                Ok(self.guard(value, present, left.always || right.always))
            }
            ("core", "format-dateTime")
                if (2..=3).contains(&f.inputs.len())
                    && f.inputs.get(2).is_none_or(Option::is_none) =>
            {
                let picture_feed = self.feed(f.inputs[1])?;
                if self.literal_text(picture_feed).is_none() {
                    return Err(refuse("dateTime picture must be a literal"));
                }
                let input_feed = self.feed(f.inputs[0])?;
                let input = if self.filtered_source_sequence(input_feed) {
                    if let Some(owner) = self.external_owner(input_feed, anchor)? {
                        if self.volatile(input_feed, &mut BTreeSet::new())? {
                            return Err(refuse("volatile first-date selection is not admitted"));
                        }
                        let item = self.scalar(input_feed, &owner)?;
                        let value = self.alloc(Node::CollectionFind {
                            collection: owner[anchor.len()..].to_vec(),
                            predicate: item.present,
                            value: item.value,
                        });
                        let present = self.call("exists", vec![value]);
                        Expr {
                            value,
                            present,
                            always: false,
                        }
                    } else {
                        self.input(&f, 0, anchor)?
                    }
                } else {
                    self.input(&f, 0, anchor)?
                };
                let picture = self.scalar(picture_feed, anchor)?;
                let datetime = self.call("coerce_datetime", vec![input.value]);
                let value = self.call(
                    "format_datetime_milliseconds",
                    vec![datetime, picture.value],
                );
                Ok(self.guard(value, input.present, input.always))
            }
            ("core", "format-number") if f.inputs.len() == 4 && f.inputs[2..] == [None, None] => {
                if self.literal_text(self.feed(f.inputs[1])?) != Some("#") {
                    return Err(refuse("only the exact native integer picture is admitted"));
                }
                let input = self.input(&f, 0, anchor)?;
                let lexical = self.call("string", vec![input.value]);
                let decimal = self.call("decimal_from_nullable_text", vec![lexical]);
                let value = self.call("format_decimal_integer", vec![decimal]);
                Ok(self.guard(value, input.present, input.always))
            }
            ("core", "equal") if f.inputs.len() == 2 => {
                if !f.inputs.iter().any(|i| {
                    self.feed(*i)
                        .ok()
                        .and_then(|key| self.literal_text(key))
                        .is_some()
                }) {
                    return Err(refuse("equality requires a String literal operand"));
                }
                let args = [self.input(&f, 0, anchor)?, self.input(&f, 1, anchor)?];
                let (present, always) = self.all_present(&args);
                let value = self.call("equal", args.iter().map(|a| a.value).collect());
                Ok(self.guard(value, present, always))
            }
            ("core", "boolean") if f.inputs.len() == 1 => {
                if !self.dependencies(feed, &mut BTreeSet::new())?.is_empty()
                    || self.volatile(feed, &mut BTreeSet::new())?
                {
                    return Err(refuse(
                        "only source-free pure boolean conversion is admitted",
                    ));
                }
                let input = self.input(&f, 0, anchor)?;
                let value = self.call("boolean", vec![input.value]);
                Ok(self.guard(value, input.present, input.always))
            }
            ("core", "concat") if (2..=16).contains(&f.inputs.len()) => {
                let args = (0..f.inputs.len())
                    .map(|i| self.input(&f, i, anchor))
                    .collect::<Result<Vec<_>, _>>()?;
                let (present, always) = self.all_present(&args);
                let values = args
                    .iter()
                    .map(|a| self.call("string", vec![a.value]))
                    .collect();
                let value = self.call("concat", values);
                Ok(self.guard(value, present, always))
            }
            _ => Err(refuse(format!(
                "unsupported scalar {}:{}",
                f.parsed.library, f.parsed.name
            ))),
        }
    }
    pub(super) fn cast(&mut self, expr: Expr, ty: ScalarType) -> Result<Expr, MfdError> {
        let value = match ty {
            ScalarType::String => self.call("string", vec![expr.value]),
            ScalarType::Float => {
                let text = self.call("string", vec![expr.value]);
                self.call("decimal_from_nullable_text", vec![text])
            }
            ScalarType::Int => expr.value,
            ScalarType::Bool => self.call("boolean", vec![expr.value]),
        };
        Ok(self.guard(value, expr.present, expr.always))
    }
    pub(super) fn reduced(
        &mut self,
        feed: u32,
        anchor: &[String],
        ty: ScalarType,
    ) -> Result<Expr, MfdError> {
        if let Some(owner) = self.external_owner(feed, anchor)? {
            if self.volatile(feed, &mut BTreeSet::new())? {
                return Err(refuse("volatile sequence reduction is not admitted"));
            }
            let expr = self.scalar(feed, &owner)?;
            let expr = self.cast(expr, ty)?;
            let text = self.call("string", vec![expr.value]);
            let text = self.guard(text, expr.present, expr.always);
            let value = self.alloc(Node::Aggregate {
                function: mapping::AggregateOp::FirstNonemptyText,
                collection: owner[anchor.len()..].to_vec(),
                value: vec![],
                expression: Some(text.value),
                arg: None,
            });
            let present = self.call("exists", vec![value]);
            self.cast(
                Expr {
                    value,
                    present,
                    always: false,
                },
                ty,
            )
        } else {
            let expr = self.scalar(feed, anchor)?;
            self.cast(expr, ty)
        }
    }
}
