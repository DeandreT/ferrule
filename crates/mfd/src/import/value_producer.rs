//! One registry for specialized scalar lowering and its input/context contract.
//!
//! Source fields, source-node functions, database computed columns, and resolved
//! join fields are source-backed boundaries, not connected scalar consumers.
//! Join admission accepts plain source inputs or its separately bounded typed
//! key expressions. Transparent schema variables and ordinary functions are
//! handled by the ordinary feed traversal.

use mapping::NodeId;

use super::graph::GraphBuilder;

#[derive(Clone, Copy)]
pub(super) enum SpecializedProducer {
    ExternalScalar(usize),
    ExternalXslt(usize),
    JsonSerializer(usize),
    XmlSerializer(usize),
    JsonParser(usize),
    FlextextParser(usize),
    Udf { call: usize, component: u32 },
}

#[derive(Clone, Copy)]
pub(super) enum InputContext {
    Unanchored(&'static str),
    Structural(&'static str),
}

impl SpecializedProducer {
    pub(super) fn input_pins(self, builder: &GraphBuilder<'_>) -> Vec<u32> {
        match self {
            Self::ExternalScalar(index) => builder.external_scalar_recipes[index]
                .input_pins()
                .collect(),
            Self::ExternalXslt(index) => builder.external_xslt_aggregates[index]
                .input_pins()
                .collect(),
            Self::JsonSerializer(index) => builder.json_serializers[index].input_pins().collect(),
            Self::XmlSerializer(index) => builder.xml_serializers[index].input_pins().collect(),
            Self::JsonParser(index) => builder.json_parsers[index].input_pins().collect(),
            Self::FlextextParser(index) => builder.flextext_parsers[index].input_pins().collect(),
            Self::Udf { call, .. } => builder.udf_calls[call].input_pins().collect(),
        }
    }

    pub(super) fn input_contexts(self, builder: &GraphBuilder<'_>) -> Vec<(u32, InputContext)> {
        let uniform = |context| {
            self.input_pins(builder)
                .into_iter()
                .map(|pin| (pin, context))
                .collect()
        };
        match self {
            Self::ExternalScalar(_) => {
                uniform(InputContext::Unanchored("external scalar function"))
            }
            Self::ExternalXslt(_) => uniform(InputContext::Structural("XSLT aggregate")),
            Self::JsonSerializer(_) => uniform(InputContext::Unanchored("JSON serializer")),
            Self::XmlSerializer(_) => uniform(InputContext::Structural("XML serializer")),
            Self::JsonParser(_) | Self::FlextextParser(_) => {
                uniform(InputContext::Unanchored("scalar parser"))
            }
            // Structural catalog pins must preserve even a directly wired
            // filter; scalar arguments keep their ordinary scalar handling.
            Self::Udf { call, .. } => builder.udf_calls[call]
                .scalar_input_pins()
                .map(|pin| (pin, InputContext::Unanchored("scalar UDF")))
                .chain(
                    builder.udf_calls[call]
                        .structured_input_pins()
                        .map(|pin| (pin, InputContext::Structural("structured UDF input"))),
                )
                .collect(),
        }
    }

    pub(super) fn materialize(self, builder: &mut GraphBuilder<'_>, key: u32) -> Option<NodeId> {
        match self {
            Self::ExternalScalar(_) => builder.external_scalar_node(key),
            Self::ExternalXslt(_) => builder.external_xslt_aggregate_node(key),
            Self::JsonSerializer(_) => builder.json_serializer_node(key),
            Self::XmlSerializer(_) => builder.xml_serializer_node(key),
            Self::JsonParser(_) => builder.json_parser_node(key),
            Self::FlextextParser(_) => builder.flextext_parser_node(key),
            Self::Udf { call, component } => builder.udf_output_node(key, call, component),
        }
    }
}

impl GraphBuilder<'_> {
    pub(super) fn specialized_value_producer(&self, key: u32) -> Option<SpecializedProducer> {
        use SpecializedProducer as P;
        self.external_scalar_recipes
            .iter()
            .position(|recipe| recipe.output == key)
            .map(P::ExternalScalar)
            .or_else(|| {
                self.external_xslt_aggregates
                    .iter()
                    .position(|recipe| recipe.output == key)
                    .map(P::ExternalXslt)
            })
            .or_else(|| {
                self.json_serializers
                    .iter()
                    .position(|recipe| recipe.output == key)
                    .map(P::JsonSerializer)
            })
            .or_else(|| {
                self.xml_serializers
                    .iter()
                    .position(|recipe| recipe.output == key)
                    .map(P::XmlSerializer)
            })
            .or_else(|| {
                self.json_parsers
                    .iter()
                    .position(|recipe| {
                        recipe.outputs.contains_key(&key)
                            || recipe.object_outputs.contains_key(&key)
                    })
                    .map(P::JsonParser)
            })
            .or_else(|| {
                self.flextext_parsers
                    .iter()
                    .position(|recipe| recipe.outputs.contains_key(&key))
                    .map(P::FlextextParser)
            })
            .or_else(|| {
                self.udf_by_output
                    .get(&key)
                    .filter(|_| {
                        self.source_abs_path(key).is_none() && self.intermediate_feed(key).is_none()
                    })
                    .map(|&(call, component)| P::Udf { call, component })
            })
    }
}
