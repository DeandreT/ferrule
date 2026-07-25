use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarContext {
    Main,
    UserDefined,
    NestedUserDefined,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionStyle {
    Graph,
    Legacy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeneratedSequence {
    Tokenize,
    TokenizeByLength,
    TokenizeRegex,
    Generate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeneratedAggregate {
    Count,
    Sum,
    Avg,
    Min,
    Max,
    Join,
}

impl GeneratedAggregate {
    fn component_name(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Sum => "sum",
            Self::Avg => "avg",
            Self::Min => "min",
            Self::Max => "max",
            Self::Join => "string-join",
        }
    }

    const fn has_arg(self) -> bool {
        matches!(self, Self::Join)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GeneratedReduction {
    ItemAt(GeneratedSequence),
    Exists(GeneratedSequence),
    ExistsAtPosition(GeneratedSequence),
    Aggregate(GeneratedSequence, GeneratedAggregate),
    ComputedAggregate(GeneratedSequence, GeneratedAggregate),
    FilteredAggregate(GeneratedSequence, GeneratedAggregate),
    FilteredComputedAggregate(GeneratedSequence, GeneratedAggregate),
}

impl GeneratedSequence {
    fn component_name(self) -> &'static str {
        match self {
            Self::Tokenize => "tokenize",
            Self::TokenizeByLength => "tokenize-by-length",
            Self::TokenizeRegex => "tokenize-regexp",
            Self::Generate => "generate-sequence",
        }
    }

    const fn input_count(self) -> usize {
        match self {
            Self::Tokenize | Self::TokenizeByLength | Self::Generate => 2,
            Self::TokenizeRegex => 3,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ScalarLiteral {
    String(String),
    Integer(i64),
    Decimal(f64),
    Boolean(bool),
}

impl ScalarLiteral {
    pub fn string(value: impl Into<String>) -> Self {
        Self::String(value.into())
    }

    fn datatype(&self) -> &'static str {
        match self {
            Self::String(_) => "string",
            Self::Integer(_) => "integer",
            Self::Decimal(_) => "decimal",
            Self::Boolean(_) => "boolean",
        }
    }

    fn lexical_value(&self) -> String {
        match self {
            Self::String(value) => value.clone(),
            Self::Integer(value) => value.to_string(),
            Self::Decimal(value) => value.to_string(),
            Self::Boolean(value) => value.to_string(),
        }
    }
}

pub struct MfdFixture {
    root: PathBuf,
    design: PathBuf,
}

impl MfdFixture {
    pub fn design(&self) -> &Path {
        &self.design
    }
}

impl Drop for MfdFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub struct ScalarMfdBuilder {
    tag: String,
    function_name: String,
    function_library: String,
    arguments: Vec<ScalarLiteral>,
    output_type: String,
    context: ScalarContext,
    connection_style: ConnectionStyle,
    reverse_components: bool,
    key_offset: u32,
    generated_reduction: Option<GeneratedReduction>,
}

impl ScalarMfdBuilder {
    pub fn new(
        tag: impl Into<String>,
        function_name: impl Into<String>,
        function_library: impl Into<String>,
        arguments: Vec<ScalarLiteral>,
        output_type: impl Into<String>,
    ) -> Self {
        Self {
            tag: tag.into(),
            function_name: function_name.into(),
            function_library: function_library.into(),
            arguments,
            output_type: output_type.into(),
            context: ScalarContext::Main,
            connection_style: ConnectionStyle::Graph,
            reverse_components: false,
            key_offset: 0,
            generated_reduction: None,
        }
    }

    pub fn generated_item_at(
        tag: impl Into<String>,
        sequence: GeneratedSequence,
        arguments: Vec<ScalarLiteral>,
        output_type: impl Into<String>,
    ) -> Self {
        Self {
            tag: tag.into(),
            function_name: "item-at".to_string(),
            function_library: "core".to_string(),
            arguments,
            output_type: output_type.into(),
            context: ScalarContext::UserDefined,
            connection_style: ConnectionStyle::Graph,
            reverse_components: false,
            key_offset: 0,
            generated_reduction: Some(GeneratedReduction::ItemAt(sequence)),
        }
    }

    pub fn generated_exists(
        tag: impl Into<String>,
        sequence: GeneratedSequence,
        arguments: Vec<ScalarLiteral>,
    ) -> Self {
        Self {
            tag: tag.into(),
            function_name: "exists".to_string(),
            function_library: "core".to_string(),
            arguments,
            output_type: "boolean".to_string(),
            context: ScalarContext::UserDefined,
            connection_style: ConnectionStyle::Graph,
            reverse_components: false,
            key_offset: 0,
            generated_reduction: Some(GeneratedReduction::Exists(sequence)),
        }
    }

    pub fn generated_exists_at_position(
        tag: impl Into<String>,
        sequence: GeneratedSequence,
        arguments: Vec<ScalarLiteral>,
    ) -> Self {
        Self {
            tag: tag.into(),
            function_name: "exists".to_string(),
            function_library: "core".to_string(),
            arguments,
            output_type: "boolean".to_string(),
            context: ScalarContext::UserDefined,
            connection_style: ConnectionStyle::Graph,
            reverse_components: false,
            key_offset: 0,
            generated_reduction: Some(GeneratedReduction::ExistsAtPosition(sequence)),
        }
    }

    pub fn generated_aggregate(
        tag: impl Into<String>,
        sequence: GeneratedSequence,
        operation: GeneratedAggregate,
        arguments: Vec<ScalarLiteral>,
        output_type: impl Into<String>,
    ) -> Self {
        Self {
            tag: tag.into(),
            function_name: operation.component_name().to_string(),
            function_library: "core".to_string(),
            arguments,
            output_type: output_type.into(),
            context: ScalarContext::UserDefined,
            connection_style: ConnectionStyle::Graph,
            reverse_components: false,
            key_offset: 0,
            generated_reduction: Some(GeneratedReduction::Aggregate(sequence, operation)),
        }
    }

    pub fn generated_filtered_aggregate(
        tag: impl Into<String>,
        sequence: GeneratedSequence,
        operation: GeneratedAggregate,
        arguments: Vec<ScalarLiteral>,
        output_type: impl Into<String>,
    ) -> Self {
        Self {
            tag: tag.into(),
            function_name: operation.component_name().to_string(),
            function_library: "core".to_string(),
            arguments,
            output_type: output_type.into(),
            context: ScalarContext::UserDefined,
            connection_style: ConnectionStyle::Graph,
            reverse_components: false,
            key_offset: 0,
            generated_reduction: Some(GeneratedReduction::FilteredAggregate(sequence, operation)),
        }
    }

    pub fn generated_computed_aggregate(
        tag: impl Into<String>,
        sequence: GeneratedSequence,
        operation: GeneratedAggregate,
        arguments: Vec<ScalarLiteral>,
        output_type: impl Into<String>,
    ) -> Self {
        Self {
            tag: tag.into(),
            function_name: operation.component_name().to_string(),
            function_library: "core".to_string(),
            arguments,
            output_type: output_type.into(),
            context: ScalarContext::UserDefined,
            connection_style: ConnectionStyle::Graph,
            reverse_components: false,
            key_offset: 0,
            generated_reduction: Some(GeneratedReduction::ComputedAggregate(sequence, operation)),
        }
    }

    pub fn generated_filtered_computed_aggregate(
        tag: impl Into<String>,
        sequence: GeneratedSequence,
        operation: GeneratedAggregate,
        arguments: Vec<ScalarLiteral>,
        output_type: impl Into<String>,
    ) -> Self {
        Self {
            tag: tag.into(),
            function_name: operation.component_name().to_string(),
            function_library: "core".to_string(),
            arguments,
            output_type: output_type.into(),
            context: ScalarContext::UserDefined,
            connection_style: ConnectionStyle::Graph,
            reverse_components: false,
            key_offset: 0,
            generated_reduction: Some(GeneratedReduction::FilteredComputedAggregate(
                sequence, operation,
            )),
        }
    }

    pub fn context(mut self, context: ScalarContext) -> Self {
        self.context = context;
        self
    }

    pub fn connection_style(mut self, style: ConnectionStyle) -> Self {
        self.connection_style = style;
        self
    }

    pub fn reverse_components(mut self, reverse: bool) -> Self {
        self.reverse_components = reverse;
        self
    }

    pub fn key_offset(mut self, offset: u32) -> Self {
        self.key_offset = offset;
        self
    }

    pub fn write(self) -> Result<MfdFixture, std::io::Error> {
        if self.arguments.len() > 64 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "scalar scenarios support at most 64 arguments",
            ));
        }
        if let Some(reduction) = self.generated_reduction {
            let sequence = match reduction {
                GeneratedReduction::ItemAt(sequence)
                | GeneratedReduction::Exists(sequence)
                | GeneratedReduction::ExistsAtPosition(sequence)
                | GeneratedReduction::Aggregate(sequence, _)
                | GeneratedReduction::ComputedAggregate(sequence, _)
                | GeneratedReduction::FilteredAggregate(sequence, _)
                | GeneratedReduction::FilteredComputedAggregate(sequence, _) => sequence,
            };
            let (extra_arguments, scalar_role) = match reduction {
                GeneratedReduction::ItemAt(_) => (1, "index"),
                GeneratedReduction::Exists(_) => (1, "comparison value"),
                GeneratedReduction::ExistsAtPosition(_) => (1, "position"),
                GeneratedReduction::Aggregate(_, operation) => {
                    (usize::from(operation.has_arg()), "aggregate argument")
                }
                GeneratedReduction::ComputedAggregate(_, operation) => (
                    1 + usize::from(operation.has_arg()),
                    "value/aggregate arguments",
                ),
                GeneratedReduction::FilteredAggregate(_, operation) => (
                    1 + usize::from(operation.has_arg()),
                    "comparison/aggregate arguments",
                ),
                GeneratedReduction::FilteredComputedAggregate(_, operation) => (
                    2 + usize::from(operation.has_arg()),
                    "value/comparison/aggregate arguments",
                ),
            };
            let reduction_name = match reduction {
                GeneratedReduction::ItemAt(_) => "item-at",
                GeneratedReduction::Exists(_) => "filtered exists",
                GeneratedReduction::ExistsAtPosition(_) => "position-filtered exists",
                GeneratedReduction::Aggregate(_, operation) => operation.component_name(),
                GeneratedReduction::ComputedAggregate(_, operation) => operation.component_name(),
                GeneratedReduction::FilteredAggregate(_, operation) => operation.component_name(),
                GeneratedReduction::FilteredComputedAggregate(_, operation) => {
                    operation.component_name()
                }
            };
            if self.arguments.len() != sequence.input_count() + extra_arguments {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "{} {reduction_name} scenarios require {} sequence arguments plus {extra_arguments} {scalar_role}",
                        sequence.component_name(),
                        sequence.input_count()
                    ),
                ));
            }
        }
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "ferrule_mfd_scenario_{}_{}_{}",
            sanitize_tag(&self.tag),
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root)?;
        std::fs::write(root.join("source.xsd"), source_schema())?;
        std::fs::write(root.join("target.xsd"), self.target_schema())?;

        let design = root.join("mapping.mfd");
        std::fs::write(&design, self.render())?;
        Ok(MfdFixture { root, design })
    }

    fn target_schema(&self) -> String {
        format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Target"><xs:complexType><xs:sequence>
    <xs:element name="Result" type="xs:{}"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
            escape_attribute(&self.output_type)
        )
    }

    fn render(&self) -> String {
        let ids = Ids::new(self.key_offset);
        let (main, mut definitions) = match self.context {
            ScalarContext::Main => (self.render_main_direct(&ids), Vec::new()),
            ScalarContext::UserDefined => (
                self.render_main_call(&ids, "ScenarioFunction", &ids.inner),
                vec![self.render_definition(
                    &ids.inner,
                    "ScenarioFunction",
                    DefinitionBody::Function,
                )],
            ),
            ScalarContext::NestedUserDefined => (
                self.render_main_call(&ids, "ScenarioFunction", &ids.outer),
                vec![
                    self.render_definition(
                        &ids.outer,
                        "ScenarioFunction",
                        DefinitionBody::Nested {
                            callee: "ScenarioInner",
                            callee_ids: &ids.inner,
                        },
                    ),
                    self.render_definition(&ids.inner, "ScenarioInner", DefinitionBody::Function),
                ],
            ),
        };
        if self.reverse_components {
            definitions.reverse();
        }

        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <mapping version=\"31\">\n{main}{}\n</mapping>\n",
            definitions.join("\n")
        )
    }

    fn render_main_direct(&self, ids: &Ids) -> String {
        let constants = self.constant_components(&ids.main);
        let body = self.render_function_body(
            ids.main.function_uid,
            &ids.main.function_inputs,
            ids.main.function_output,
            &ids.main.constant_outputs,
        );
        let mut components = Vec::with_capacity(constants.len() + body.components.len() + 2);
        components.push(source_component(
            ids.main.source_uid,
            ids.main.source_output,
        ));
        components.extend(constants);
        components.extend(body.components);
        components.push(target_component(ids.main.target_uid, ids.main.target_input));
        self.maybe_reverse(&mut components);

        let mut edges = body.edges;
        edges.push((body.output, ids.main.target_input));
        wrapper_component("main", &components, &edges, self.connection_style, None)
    }

    fn render_main_call(&self, ids: &Ids, name: &str, definition: &FunctionIds) -> String {
        let argument_count = self.arguments.len();
        let constants = self.constant_components(&ids.main);
        let call = udf_call_component(
            name,
            ids.main.function_uid,
            &ids.main.function_inputs[..argument_count],
            ids.main.function_output,
            &definition.parameter_uids[..argument_count],
            definition.output_uid,
        );
        let mut components = Vec::with_capacity(constants.len() + 3);
        components.push(source_component(
            ids.main.source_uid,
            ids.main.source_output,
        ));
        components.extend(constants);
        components.push(call);
        components.push(target_component(ids.main.target_uid, ids.main.target_input));
        self.maybe_reverse(&mut components);

        let mut edges = self.constant_edges(&ids.main);
        edges.push((ids.main.function_output, ids.main.target_input));
        wrapper_component("main", &components, &edges, self.connection_style, None)
    }

    fn render_definition(&self, ids: &FunctionIds, name: &str, body: DefinitionBody<'_>) -> String {
        let argument_count = self.arguments.len();
        let mut components = parameter_components(ids, &self.arguments);
        let body = match body {
            DefinitionBody::Function => self.render_function_body(
                ids.function_uid,
                &ids.function_inputs,
                ids.function_output,
                &ids.parameter_outputs,
            ),
            DefinitionBody::Nested { callee, callee_ids } => RenderedBody {
                components: vec![udf_call_component(
                    callee,
                    ids.function_uid,
                    &ids.function_inputs[..argument_count],
                    ids.function_output,
                    &callee_ids.parameter_uids[..argument_count],
                    callee_ids.output_uid,
                )],
                edges: ids
                    .parameter_outputs
                    .iter()
                    .take(argument_count)
                    .copied()
                    .zip(ids.function_inputs.iter().take(argument_count).copied())
                    .collect(),
                output: ids.function_output,
            },
        };
        components.extend(body.components);
        components.push(output_component(
            ids.output_uid,
            ids.output_input,
            &self.output_type,
        ));
        self.maybe_reverse(&mut components);

        let mut edges = body.edges;
        edges.push((body.output, ids.output_input));
        wrapper_component(
            name,
            &components,
            &edges,
            self.connection_style,
            Some("fixture"),
        )
    }

    fn constant_components(&self, ids: &MainIds) -> Vec<String> {
        self.arguments
            .iter()
            .enumerate()
            .map(|(index, value)| {
                constant_component(ids.constant_uids[index], ids.constant_outputs[index], value)
            })
            .collect()
    }

    fn constant_edges(&self, ids: &MainIds) -> Vec<(u32, u32)> {
        ids.constant_outputs
            .iter()
            .copied()
            .zip(ids.function_inputs.iter().copied())
            .collect()
    }

    fn render_function_body(
        &self,
        function_uid: u32,
        function_inputs: &[u32],
        function_output: u32,
        argument_outputs: &[u32],
    ) -> RenderedBody {
        let argument_count = self.arguments.len();
        let Some(reduction) = self.generated_reduction else {
            return RenderedBody {
                components: vec![function_component(
                    &self.function_name,
                    &self.function_library,
                    function_uid,
                    &function_inputs[..argument_count],
                    function_output,
                )],
                edges: argument_outputs
                    .iter()
                    .take(argument_count)
                    .copied()
                    .zip(function_inputs.iter().take(argument_count).copied())
                    .collect(),
                output: function_output,
            };
        };

        let sequence = match reduction {
            GeneratedReduction::ItemAt(sequence)
            | GeneratedReduction::Exists(sequence)
            | GeneratedReduction::ExistsAtPosition(sequence)
            | GeneratedReduction::Aggregate(sequence, _)
            | GeneratedReduction::ComputedAggregate(sequence, _)
            | GeneratedReduction::FilteredAggregate(sequence, _)
            | GeneratedReduction::FilteredComputedAggregate(sequence, _) => sequence,
        };
        let sequence_inputs = sequence.input_count();
        if let GeneratedReduction::FilteredComputedAggregate(_, operation) = reduction {
            return self.render_generated_filtered_computed_aggregate(
                function_uid,
                function_inputs,
                function_output,
                argument_outputs,
                sequence,
                operation,
            );
        }
        if let GeneratedReduction::ComputedAggregate(_, operation) = reduction {
            return self.render_generated_computed_aggregate(
                function_uid,
                function_inputs,
                function_output,
                argument_outputs,
                sequence,
                operation,
            );
        }
        if let GeneratedReduction::FilteredAggregate(_, operation) = reduction {
            return self.render_generated_filtered_aggregate(
                function_uid,
                function_inputs,
                function_output,
                argument_outputs,
                sequence,
                operation,
            );
        }
        if let GeneratedReduction::Aggregate(_, operation) = reduction {
            return self.render_generated_aggregate(
                function_uid,
                function_inputs,
                function_output,
                argument_outputs,
                sequence,
                operation,
            );
        }
        if matches!(
            reduction,
            GeneratedReduction::Exists(_) | GeneratedReduction::ExistsAtPosition(_)
        ) {
            return self.render_generated_exists(
                function_uid,
                function_inputs,
                function_output,
                argument_outputs,
                sequence,
                matches!(reduction, GeneratedReduction::ExistsAtPosition(_)),
            );
        }
        let item_sequence_input = function_output + 1;
        let item_index_input = function_output + 2;
        let item_output = function_output + 3;
        let mut edges = argument_outputs
            .iter()
            .take(sequence_inputs)
            .copied()
            .zip(function_inputs.iter().take(sequence_inputs).copied())
            .collect::<Vec<_>>();
        edges.extend([
            (function_output, item_sequence_input),
            (argument_outputs[sequence_inputs], item_index_input),
        ]);
        RenderedBody {
            components: vec![
                function_component(
                    sequence.component_name(),
                    "core",
                    function_uid,
                    &function_inputs[..sequence_inputs],
                    function_output,
                ),
                function_component(
                    "item-at",
                    "core",
                    function_uid + 1,
                    &[item_sequence_input, item_index_input],
                    item_output,
                ),
            ],
            edges,
            output: item_output,
        }
    }

    fn render_generated_exists(
        &self,
        function_uid: u32,
        function_inputs: &[u32],
        function_output: u32,
        argument_outputs: &[u32],
        sequence: GeneratedSequence,
        compare_position: bool,
    ) -> RenderedBody {
        if compare_position {
            return self.render_generated_position_exists(
                function_uid,
                function_inputs,
                function_output,
                argument_outputs,
                sequence,
            );
        }
        let sequence_inputs = sequence.input_count();
        let equal_item_input = function_output + 1;
        let equal_expected_input = function_output + 2;
        let equal_output = function_output + 3;
        let filter_sequence_input = function_output + 4;
        let filter_predicate_input = function_output + 5;
        let filter_true_output = function_output + 6;
        let filter_false_output = function_output + 7;
        let exists_input = function_output + 8;
        let exists_output = function_output + 9;
        let mut edges = argument_outputs
            .iter()
            .take(sequence_inputs)
            .copied()
            .zip(function_inputs.iter().take(sequence_inputs).copied())
            .collect::<Vec<_>>();
        edges.extend([
            (function_output, equal_item_input),
            (argument_outputs[sequence_inputs], equal_expected_input),
            (function_output, filter_sequence_input),
            (equal_output, filter_predicate_input),
            (filter_true_output, exists_input),
        ]);
        RenderedBody {
            components: vec![
                function_component(
                    sequence.component_name(),
                    "core",
                    function_uid,
                    &function_inputs[..sequence_inputs],
                    function_output,
                ),
                function_component(
                    "equal",
                    "core",
                    function_uid + 1,
                    &[equal_item_input, equal_expected_input],
                    equal_output,
                ),
                filter_component(
                    function_uid + 2,
                    filter_sequence_input,
                    filter_predicate_input,
                    filter_true_output,
                    filter_false_output,
                ),
                function_component(
                    "exists",
                    "core",
                    function_uid + 3,
                    &[exists_input],
                    exists_output,
                ),
            ],
            edges,
            output: exists_output,
        }
    }

    fn render_generated_position_exists(
        &self,
        function_uid: u32,
        function_inputs: &[u32],
        function_output: u32,
        argument_outputs: &[u32],
        sequence: GeneratedSequence,
    ) -> RenderedBody {
        let sequence_inputs = sequence.input_count();
        let position_context_input = function_output + 1;
        let position_output = function_output + 2;
        let equal_position_input = function_output + 3;
        let equal_expected_input = function_output + 4;
        let equal_output = function_output + 5;
        let filter_sequence_input = function_output + 6;
        let filter_predicate_input = function_output + 7;
        let filter_true_output = function_output + 8;
        let filter_false_output = function_output + 9;
        let exists_input = function_output + 10;
        let exists_output = function_output + 11;
        let mut edges = argument_outputs
            .iter()
            .take(sequence_inputs)
            .copied()
            .zip(function_inputs.iter().take(sequence_inputs).copied())
            .collect::<Vec<_>>();
        edges.extend([
            (function_output, position_context_input),
            (position_output, equal_position_input),
            (argument_outputs[sequence_inputs], equal_expected_input),
            (function_output, filter_sequence_input),
            (equal_output, filter_predicate_input),
            (filter_true_output, exists_input),
        ]);
        RenderedBody {
            components: vec![
                function_component(
                    sequence.component_name(),
                    "core",
                    function_uid,
                    &function_inputs[..sequence_inputs],
                    function_output,
                ),
                function_component(
                    "position",
                    "core",
                    function_uid + 1,
                    &[position_context_input],
                    position_output,
                ),
                function_component(
                    "equal",
                    "core",
                    function_uid + 2,
                    &[equal_position_input, equal_expected_input],
                    equal_output,
                ),
                filter_component(
                    function_uid + 3,
                    filter_sequence_input,
                    filter_predicate_input,
                    filter_true_output,
                    filter_false_output,
                ),
                function_component(
                    "exists",
                    "core",
                    function_uid + 4,
                    &[exists_input],
                    exists_output,
                ),
            ],
            edges,
            output: exists_output,
        }
    }

    fn render_generated_aggregate(
        &self,
        function_uid: u32,
        function_inputs: &[u32],
        function_output: u32,
        argument_outputs: &[u32],
        sequence: GeneratedSequence,
        operation: GeneratedAggregate,
    ) -> RenderedBody {
        let sequence_inputs = sequence.input_count();
        let aggregate_sequence_input = function_output + 1;
        let aggregate_arg_input = function_output + 2;
        let aggregate_output = function_output + 3;
        let mut edges = argument_outputs
            .iter()
            .take(sequence_inputs)
            .copied()
            .zip(function_inputs.iter().take(sequence_inputs).copied())
            .collect::<Vec<_>>();
        edges.push((function_output, aggregate_sequence_input));
        if operation.has_arg() {
            edges.push((argument_outputs[sequence_inputs], aggregate_arg_input));
        }
        RenderedBody {
            components: vec![
                function_component(
                    sequence.component_name(),
                    "core",
                    function_uid,
                    &function_inputs[..sequence_inputs],
                    function_output,
                ),
                aggregate_component(
                    function_uid + 1,
                    operation,
                    aggregate_sequence_input,
                    operation.has_arg().then_some(aggregate_arg_input),
                    aggregate_output,
                ),
            ],
            edges,
            output: aggregate_output,
        }
    }

    fn render_generated_computed_aggregate(
        &self,
        function_uid: u32,
        function_inputs: &[u32],
        function_output: u32,
        argument_outputs: &[u32],
        sequence: GeneratedSequence,
        operation: GeneratedAggregate,
    ) -> RenderedBody {
        let sequence_inputs = sequence.input_count();
        let multiply_item_input = function_output + 1;
        let multiply_factor_input = function_output + 2;
        let multiply_output = function_output + 3;
        let aggregate_sequence_input = function_output + 4;
        let aggregate_arg_input = function_output + 5;
        let aggregate_output = function_output + 6;
        let mut edges = argument_outputs
            .iter()
            .take(sequence_inputs)
            .copied()
            .zip(function_inputs.iter().take(sequence_inputs).copied())
            .collect::<Vec<_>>();
        edges.extend([
            (function_output, multiply_item_input),
            (argument_outputs[sequence_inputs], multiply_factor_input),
            (multiply_output, aggregate_sequence_input),
        ]);
        if operation.has_arg() {
            edges.push((argument_outputs[sequence_inputs + 1], aggregate_arg_input));
        }
        RenderedBody {
            components: vec![
                function_component(
                    sequence.component_name(),
                    "core",
                    function_uid,
                    &function_inputs[..sequence_inputs],
                    function_output,
                ),
                function_component(
                    "multiply",
                    "core",
                    function_uid + 1,
                    &[multiply_item_input, multiply_factor_input],
                    multiply_output,
                ),
                aggregate_component(
                    function_uid + 2,
                    operation,
                    aggregate_sequence_input,
                    operation.has_arg().then_some(aggregate_arg_input),
                    aggregate_output,
                ),
            ],
            edges,
            output: aggregate_output,
        }
    }

    fn render_generated_filtered_computed_aggregate(
        &self,
        function_uid: u32,
        function_inputs: &[u32],
        function_output: u32,
        argument_outputs: &[u32],
        sequence: GeneratedSequence,
        operation: GeneratedAggregate,
    ) -> RenderedBody {
        let sequence_inputs = sequence.input_count();
        let multiply_item_input = function_output + 1;
        let multiply_factor_input = function_output + 2;
        let multiply_output = function_output + 3;
        let equal_item_input = function_output + 4;
        let equal_expected_input = function_output + 5;
        let equal_output = function_output + 6;
        let filter_sequence_input = function_output + 7;
        let filter_predicate_input = function_output + 8;
        let filter_true_output = function_output + 9;
        let filter_false_output = function_output + 10;
        let aggregate_sequence_input = function_output + 11;
        let aggregate_arg_input = function_output + 12;
        let aggregate_output = function_output + 13;
        let mut edges = argument_outputs
            .iter()
            .take(sequence_inputs)
            .copied()
            .zip(function_inputs.iter().take(sequence_inputs).copied())
            .collect::<Vec<_>>();
        edges.extend([
            (function_output, multiply_item_input),
            (argument_outputs[sequence_inputs], multiply_factor_input),
            (function_output, equal_item_input),
            (argument_outputs[sequence_inputs + 1], equal_expected_input),
            (multiply_output, filter_sequence_input),
            (equal_output, filter_predicate_input),
            (filter_true_output, aggregate_sequence_input),
        ]);
        if operation.has_arg() {
            edges.push((argument_outputs[sequence_inputs + 2], aggregate_arg_input));
        }
        RenderedBody {
            components: vec![
                function_component(
                    sequence.component_name(),
                    "core",
                    function_uid,
                    &function_inputs[..sequence_inputs],
                    function_output,
                ),
                function_component(
                    "multiply",
                    "core",
                    function_uid + 1,
                    &[multiply_item_input, multiply_factor_input],
                    multiply_output,
                ),
                function_component(
                    "equal",
                    "core",
                    function_uid + 2,
                    &[equal_item_input, equal_expected_input],
                    equal_output,
                ),
                filter_component(
                    function_uid + 3,
                    filter_sequence_input,
                    filter_predicate_input,
                    filter_true_output,
                    filter_false_output,
                ),
                aggregate_component(
                    function_uid + 4,
                    operation,
                    aggregate_sequence_input,
                    operation.has_arg().then_some(aggregate_arg_input),
                    aggregate_output,
                ),
            ],
            edges,
            output: aggregate_output,
        }
    }

    fn render_generated_filtered_aggregate(
        &self,
        function_uid: u32,
        function_inputs: &[u32],
        function_output: u32,
        argument_outputs: &[u32],
        sequence: GeneratedSequence,
        operation: GeneratedAggregate,
    ) -> RenderedBody {
        let sequence_inputs = sequence.input_count();
        let equal_item_input = function_output + 1;
        let equal_expected_input = function_output + 2;
        let equal_output = function_output + 3;
        let filter_sequence_input = function_output + 4;
        let filter_predicate_input = function_output + 5;
        let filter_true_output = function_output + 6;
        let filter_false_output = function_output + 7;
        let aggregate_sequence_input = function_output + 8;
        let aggregate_arg_input = function_output + 9;
        let aggregate_output = function_output + 10;
        let mut edges = argument_outputs
            .iter()
            .take(sequence_inputs)
            .copied()
            .zip(function_inputs.iter().take(sequence_inputs).copied())
            .collect::<Vec<_>>();
        edges.extend([
            (function_output, equal_item_input),
            (argument_outputs[sequence_inputs], equal_expected_input),
            (function_output, filter_sequence_input),
            (equal_output, filter_predicate_input),
            (filter_true_output, aggregate_sequence_input),
        ]);
        if operation.has_arg() {
            edges.push((argument_outputs[sequence_inputs + 1], aggregate_arg_input));
        }
        RenderedBody {
            components: vec![
                function_component(
                    sequence.component_name(),
                    "core",
                    function_uid,
                    &function_inputs[..sequence_inputs],
                    function_output,
                ),
                function_component(
                    "equal",
                    "core",
                    function_uid + 1,
                    &[equal_item_input, equal_expected_input],
                    equal_output,
                ),
                filter_component(
                    function_uid + 2,
                    filter_sequence_input,
                    filter_predicate_input,
                    filter_true_output,
                    filter_false_output,
                ),
                aggregate_component(
                    function_uid + 3,
                    operation,
                    aggregate_sequence_input,
                    operation.has_arg().then_some(aggregate_arg_input),
                    aggregate_output,
                ),
            ],
            edges,
            output: aggregate_output,
        }
    }

    fn maybe_reverse(&self, components: &mut [String]) {
        if self.reverse_components {
            components.reverse();
        }
    }
}

enum DefinitionBody<'a> {
    Function,
    Nested {
        callee: &'a str,
        callee_ids: &'a FunctionIds,
    },
}

struct RenderedBody {
    components: Vec<String>,
    edges: Vec<(u32, u32)>,
    output: u32,
}

struct Ids {
    main: MainIds,
    outer: FunctionIds,
    inner: FunctionIds,
}

impl Ids {
    fn new(offset: u32) -> Self {
        Self {
            main: MainIds::new(100 + offset),
            outer: FunctionIds::new(10_000 + offset),
            inner: FunctionIds::new(20_000 + offset),
        }
    }
}

struct MainIds {
    source_uid: u32,
    source_output: u32,
    constant_uids: Vec<u32>,
    constant_outputs: Vec<u32>,
    function_uid: u32,
    function_inputs: Vec<u32>,
    function_output: u32,
    target_uid: u32,
    target_input: u32,
}

impl MainIds {
    fn new(base: u32) -> Self {
        let argument_slots = 64;
        Self {
            source_uid: base,
            source_output: base + 1,
            constant_uids: (0..argument_slots).map(|index| base + 10 + index).collect(),
            constant_outputs: (0..argument_slots)
                .map(|index| base + 100 + index)
                .collect(),
            function_uid: base + 200,
            function_inputs: (0..argument_slots)
                .map(|index| base + 300 + index)
                .collect(),
            function_output: base + 400,
            target_uid: base + 500,
            target_input: base + 501,
        }
    }
}

struct FunctionIds {
    parameter_uids: Vec<u32>,
    parameter_outputs: Vec<u32>,
    function_uid: u32,
    function_inputs: Vec<u32>,
    function_output: u32,
    output_uid: u32,
    output_input: u32,
}

impl FunctionIds {
    fn new(base: u32) -> Self {
        let argument_slots = 64;
        Self {
            parameter_uids: (0..argument_slots).map(|index| base + index).collect(),
            parameter_outputs: (0..argument_slots)
                .map(|index| base + 100 + index)
                .collect(),
            function_uid: base + 200,
            function_inputs: (0..argument_slots)
                .map(|index| base + 300 + index)
                .collect(),
            function_output: base + 400,
            output_uid: base + 500,
            output_input: base + 501,
        }
    }
}

fn source_schema() -> &'static str {
    r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Source"><xs:complexType><xs:sequence>
    <xs:element name="Seed" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#
}

fn source_component(uid: u32, output: u32) -> String {
    format!(
        r#"<component name="source" library="xml" uid="{uid}" kind="14"><data>
  <root><entry name="Source"><entry name="Seed" outkey="{output}"/></entry></root>
  <document schema="source.xsd" instanceroot="{{}}Source"/>
</data></component>"#
    )
}

fn target_component(uid: u32, input: u32) -> String {
    format!(
        r#"<component name="target" library="xml" uid="{uid}" kind="14"><properties XSLTDefaultOutput="1"/><data>
  <root><entry name="Target"><entry name="Result" inpkey="{input}"/></entry></root>
  <document schema="target.xsd" instanceroot="{{}}Target"/>
</data></component>"#
    )
}

fn constant_component(uid: u32, output: u32, value: &ScalarLiteral) -> String {
    format!(
        r#"<component name="constant" library="core" uid="{uid}" kind="2">
  <targets><datapoint pos="0" key="{output}"/></targets>
  <data><constant value="{}" datatype="{}"/></data>
</component>"#,
        escape_attribute(&value.lexical_value()),
        value.datatype()
    )
}

fn function_component(name: &str, library: &str, uid: u32, inputs: &[u32], output: u32) -> String {
    let sources = datapoints("sources", inputs);
    format!(
        r#"<component name="{}" library="{}" uid="{uid}" kind="5">
  {sources}<targets><datapoint pos="0" key="{output}"/></targets>
</component>"#,
        escape_attribute(name),
        escape_attribute(library)
    )
}

fn filter_component(
    uid: u32,
    value_input: u32,
    predicate_input: u32,
    true_output: u32,
    false_output: u32,
) -> String {
    format!(
        r#"<component name="filter" library="core" uid="{uid}" kind="3">
  <sources><datapoint pos="0" key="{value_input}"/><datapoint pos="1" key="{predicate_input}"/></sources>
  <targets><datapoint pos="0" key="{true_output}"/><datapoint pos="1" key="{false_output}"/></targets>
</component>"#
    )
}

fn aggregate_component(
    uid: u32,
    operation: GeneratedAggregate,
    sequence_input: u32,
    arg_input: Option<u32>,
    output: u32,
) -> String {
    let mut inputs = format!(r#"<datapoint/><datapoint pos="1" key="{sequence_input}"/>"#);
    if let Some(arg_input) = arg_input {
        let _ = write!(inputs, r#"<datapoint pos="2" key="{arg_input}"/>"#);
    }
    format!(
        r#"<component name="{}" library="core" uid="{uid}" kind="5">
  <sources>{inputs}</sources><targets><datapoint pos="0" key="{output}"/></targets>
</component>"#,
        operation.component_name()
    )
}

fn parameter_components(ids: &FunctionIds, arguments: &[ScalarLiteral]) -> Vec<String> {
    arguments
        .iter()
        .enumerate()
        .map(|(index, argument)| {
            let uid = ids.parameter_uids[index];
            let output = ids.parameter_outputs[index];
            format!(
                r#"<component name="arg{index}" library="core" uid="{uid}" kind="6">
  <targets><datapoint pos="0" key="{output}"/></targets>
  <data><input datatype="{}"/><parameter usageKind="input" name="arg{index}"/></data>
</component>"#,
                argument.datatype()
            )
        })
        .collect()
}

fn output_component(uid: u32, input: u32, output_type: &str) -> String {
    format!(
        r#"<component name="result" library="core" uid="{uid}" kind="7">
  <sources><datapoint pos="0" key="{input}"/></sources>
  <data><output datatype="{}"/><parameter usageKind="output" name="result"/></data>
</component>"#,
        escape_attribute(output_type)
    )
}

fn udf_call_component(
    name: &str,
    uid: u32,
    inputs: &[u32],
    output: u32,
    parameter_uids: &[u32],
    output_uid: u32,
) -> String {
    let input_entries = inputs
        .iter()
        .zip(parameter_uids)
        .enumerate()
        .map(|(index, (input, parameter_uid))| {
            format!(r#"<entry name="arg{index}" inpkey="{input}" componentid="{parameter_uid}"/>"#)
        })
        .collect::<String>();
    format!(
        r#"<component name="{}" library="fixture" uid="{uid}" kind="19"><data>
  <root>{input_entries}</root>
  <root rootindex="1"><entry name="result" outkey="{output}" componentid="{output_uid}"/></root>
</data></component>"#,
        escape_attribute(name)
    )
}

fn datapoints(tag: &str, keys: &[u32]) -> String {
    if keys.is_empty() {
        return String::new();
    }
    let points = keys
        .iter()
        .enumerate()
        .map(|(position, key)| format!(r#"<datapoint pos="{position}" key="{key}"/>"#))
        .collect::<String>();
    format!("<{tag}>{points}</{tag}>")
}

fn wrapper_component(
    name: &str,
    components: &[String],
    edges: &[(u32, u32)],
    style: ConnectionStyle,
    library: Option<&str>,
) -> String {
    let library = library.map_or_else(String::new, |library| {
        format!(r#" library="{}" inline="1""#, escape_attribute(library))
    });
    let connections = render_connections(edges, style);
    let (inside_structure, after_structure) = match style {
        ConnectionStyle::Graph => (connections.as_str(), ""),
        ConnectionStyle::Legacy => ("", connections.as_str()),
    };
    format!(
        r#"  <component name="{}"{library}>
    <structure><children>
{}
    </children>{inside_structure}</structure>{after_structure}
  </component>"#,
        escape_attribute(name),
        indent_components(components)
    )
}

fn indent_components(components: &[String]) -> String {
    components
        .iter()
        .map(|component| {
            component
                .lines()
                .map(|line| format!("      {line}\n"))
                .collect::<String>()
        })
        .collect()
}

fn render_connections(edges: &[(u32, u32)], style: ConnectionStyle) -> String {
    match style {
        ConnectionStyle::Graph => {
            let mut by_source = BTreeMap::<u32, Vec<u32>>::new();
            for (source, target) in edges {
                by_source.entry(*source).or_default().push(*target);
            }
            let mut xml = String::from("<graph><vertices>");
            for (source, targets) in by_source {
                let _ = write!(xml, r#"<vertex vertexkey="{source}"><edges>"#);
                for target in targets {
                    let _ = write!(xml, r#"<edge vertexkey="{target}"/>"#);
                }
                xml.push_str("</edges></vertex>");
            }
            xml.push_str("</vertices></graph>");
            xml
        }
        ConnectionStyle::Legacy => {
            let mut xml = String::from("<connections>");
            for (source, target) in edges {
                let _ = write!(xml, r#"<edge from="{source}" to="{target}"/>"#);
            }
            xml.push_str("</connections>");
            xml
        }
    }
}

fn escape_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn sanitize_tag(tag: &str) -> String {
    tag.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect()
}
