use std::collections::BTreeMap;

#[path = "context.rs"]
mod context;
pub(in crate::import) use context::ContextSeal;

use super::super::{Call, Registry, limits::ConstructionBudget};
use super::{Origin, Profile, RawOutput, RegisteredVec};
use crate::import::{
    external_scalar, external_xslt, flextext_parser, function, join, json_parser, json_serializer,
    schema::{self, ComponentFormat, SchemaComponent},
    source_node_function, xml_serializer,
};

#[derive(Clone, Copy)]
pub(in crate::import) struct Normalized<'a> {
    pub(in crate::import) schemas: &'a RegisteredVec<SchemaComponent>,
    pub(in crate::import) sources: &'a [&'a SchemaComponent],
    pub(in crate::import) source_names: &'a [String],
    pub(in crate::import) intermediates: &'a [&'a SchemaComponent],
    pub(in crate::import) targets: &'a [&'a SchemaComponent],
    pub(in crate::import) functions: &'a RegisteredVec<function::FnComponent>,
    pub(in crate::import) calls: &'a RegisteredVec<Call>,
    pub(in crate::import) external_scalar: &'a RegisteredVec<external_scalar::Recipe>,
    pub(in crate::import) external_xslt: &'a RegisteredVec<external_xslt::Recipe>,
    pub(in crate::import) json_serializers: &'a RegisteredVec<json_serializer::Recipe>,
    pub(in crate::import) xml_serializers: &'a RegisteredVec<xml_serializer::Recipe>,
    pub(in crate::import) json_parsers: &'a RegisteredVec<json_parser::Recipe>,
    pub(in crate::import) flextext_parsers: &'a RegisteredVec<flextext_parser::Recipe>,
    pub(in crate::import) joins: &'a join::Registry,
    pub(in crate::import) rules: &'a source_node_function::Rules,
}

enum Kind<'a> {
    Source(&'a SchemaComponent, &'a [String]),
    Function(&'a function::FnComponent),
    DeclaredCall,
    Blocking(&'static str),
}
struct Claim<'a> {
    origin: Origin,
    kind: Kind<'a>,
}

/// An ownership census, not a second implementation of resolver precedence.
/// Every competing route remains present; unsupported domains are blockers.
struct MainProducerView<'a> {
    claims: BTreeMap<u32, Vec<Claim<'a>>>,
}

/// Count consumes the census, not the declared-call profile or resolver order.
pub(in crate::import) struct CountProducerView<'a>(MainProducerView<'a>);
pub(in crate::import) enum CountProducer<'a> {
    Source(&'a SchemaComponent, &'a [String]),
    Function(&'a function::FnComponent),
}
impl<'a> CountProducerView<'a> {
    pub(in crate::import) fn build(
        n: &Normalized<'a>,
        budget: &super::CountConstruction,
    ) -> Result<Self, String> {
        let view = MainProducerView::build(n, budget.budget(), true)?;
        for claims in view.claims.values() {
            for _ in claims {
                crate::import::aggregate::filtered_count::count_visit(
                    crate::import::aggregate::filtered_count::Counter::FinalClaim,
                );
            }
        }
        Ok(Self(view))
    }
    pub(in crate::import) fn get(
        &self,
        key: u32,
        owner: super::RawOwner,
        budget: &super::CountConstruction,
    ) -> Result<CountProducer<'a>, String> {
        budget.claim(1, 0, 0)?;
        let claims = self
            .0
            .claims
            .get(&key)
            .ok_or("no final registered producer")?;
        let [claim] = claims.as_slice() else {
            return Err("competing final registered producers".into());
        };
        if claim.origin != Origin::ActiveMain(owner) {
            return Err("non-active or mismatched final producer origin".into());
        }
        match &claim.kind {
            Kind::Source(source, path) => Ok(CountProducer::Source(source, path)),
            Kind::Function(function) => Ok(CountProducer::Function(function)),
            _ => Err("unsupported final producer domain".into()),
        }
    }
}

fn exact_path<'a>(root: &'a ir::SchemaNode, path: &[String]) -> Option<&'a ir::SchemaNode> {
    let mut node = root;
    for segment in path {
        if matches!(node.xml_namespace, Some(ir::XmlNamespace::Qualified(_)))
            || !node.xml_name_alternatives.is_empty()
            || node.xml_type_alternatives
        {
            return None;
        }
        let ir::SchemaKind::Group { children, .. } = &node.kind else {
            return None;
        };
        let mut matches = children.iter().filter(|child| child.name == *segment);
        node = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
    }
    (!matches!(node.xml_namespace, Some(ir::XmlNamespace::Qualified(_)))
        && node.xml_name_alternatives.is_empty()
        && !node.xml_type_alternatives)
        .then_some(node)
}

impl<'a> MainProducerView<'a> {
    fn add(
        &mut self,
        key: u32,
        origin: Origin,
        kind: Kind<'a>,
        budget: &ConstructionBudget,
    ) -> Result<(), String> {
        budget.claim(2, 0, 0)?;
        self.claims
            .entry(key)
            .or_default()
            .push(Claim { origin, kind });
        Ok(())
    }
    fn blocking(
        &mut self,
        key: u32,
        origin: Origin,
        domain: &'static str,
        budget: &ConstructionBudget,
    ) -> Result<(), String> {
        self.add(key, origin, Kind::Blocking(domain), budget)
    }
    fn build(n: &Normalized<'a>, budget: &ConstructionBudget, count: bool) -> Result<Self, String> {
        fn at<T>(values: &RegisteredVec<T>, index: usize, count: bool) -> Origin {
            if count {
                values.count_origin_at(index)
            } else {
                values.origin_at(index)
            }
        }
        let schema_origin = |source| {
            if count {
                n.schemas.count_origin_of(source, budget)
            } else {
                n.schemas.origin_of(source, budget)
            }
        };
        let mut view = Self {
            claims: BTreeMap::new(),
        };
        for source in n.sources {
            let origin = schema_origin(*source)?;
            // ALL normalized source ports participate, including input aliases.
            for (&key, path) in &source.ports {
                view.add(key, origin, Kind::Source(source, path), budget)?;
            }
            for query in &source.db_queries {
                for key in query.certificate_computed_outputs() {
                    view.blocking(key, origin, "database computed projection", budget)?;
                }
            }
        }
        for component in n.intermediates {
            let origin = schema_origin(*component)?;
            for &key in &component.output_keys {
                view.blocking(
                    key,
                    origin,
                    "dynamic/transparent intermediate projection",
                    budget,
                )?;
            }
        }
        for key in n.joins.certificate_outputs() {
            view.blocking(
                key,
                Origin::SyntheticOrUnproved,
                "join/hierarchical/refused output",
                budget,
            )?;
        }
        for (index, recipe) in n.external_scalar.iter().enumerate() {
            view.blocking(
                recipe.output,
                at(n.external_scalar, index, count),
                "external scalar",
                budget,
            )?;
        }
        for (index, recipe) in n.external_xslt.iter().enumerate() {
            view.blocking(
                recipe.output,
                at(n.external_xslt, index, count),
                "external XSLT",
                budget,
            )?;
        }
        for (index, recipe) in n.json_serializers.iter().enumerate() {
            view.blocking(
                recipe.output,
                at(n.json_serializers, index, count),
                "JSON serializer",
                budget,
            )?;
        }
        for (index, recipe) in n.xml_serializers.iter().enumerate() {
            view.blocking(
                recipe.output,
                at(n.xml_serializers, index, count),
                "XML serializer",
                budget,
            )?;
        }
        for (index, recipe) in n.json_parsers.iter().enumerate() {
            for &key in recipe.outputs.keys().chain(recipe.object_outputs.keys()) {
                view.blocking(key, at(n.json_parsers, index, count), "JSON parser", budget)?;
            }
        }
        for (index, recipe) in n.flextext_parsers.iter().enumerate() {
            for &key in recipe.outputs.keys() {
                view.blocking(
                    key,
                    at(n.flextext_parsers, index, count),
                    "FlexText/fixed parser",
                    budget,
                )?;
            }
        }
        // Read original collections, before keyed builder maps discard collisions.
        for (index, call) in n.calls.iter().enumerate() {
            for &key in call.outputs.keys() {
                if call.certificate.is_some() {
                    view.add(key, at(n.calls, index, count), Kind::DeclaredCall, budget)?;
                } else {
                    view.blocking(
                        key,
                        at(n.calls, index, count),
                        "local/structured UDF",
                        budget,
                    )?;
                }
            }
        }
        for (index, function) in n.functions.iter().enumerate() {
            for &key in &function.outputs {
                view.add(
                    key,
                    at(n.functions, index, count),
                    Kind::Function(function),
                    budget,
                )?;
            }
            for &key in &function.declared_output_keys {
                if !function.outputs.contains(&key) {
                    view.blocking(
                        key,
                        at(n.functions, index, count),
                        "unrepresented function output",
                        budget,
                    )?;
                }
            }
        }
        // Existing rule collection can discover foreign local scopes by key.
        // No decorator profile is certified yet, even when it looks harmless.
        for (&key, rules) in n.rules {
            if !rules.is_empty() {
                view.blocking(
                    key,
                    Origin::SyntheticOrUnproved,
                    "source-node rule with unproved provenance",
                    budget,
                )?;
            }
        }
        Ok(view)
    }

    fn check(
        &self,
        key: u32,
        raw: &RawOutput,
        role: &str,
        _closure: &super::ArgumentClosure,
        budget: &ConstructionBudget,
    ) -> Result<(), String> {
        let refuse = |detail: &str| {
            format!(
                "declared-library post-normalization ownership certificate refused {role} `{key}`: {detail}"
            )
        };
        let claims = self
            .claims
            .get(&key)
            .ok_or_else(|| refuse("no registered producer"))?;
        let [claim] = claims.as_slice() else {
            return Err(refuse(
                "competing normalized producer claims; no precedence winner is certified",
            ));
        };
        if let Kind::Blocking(domain) = claim.kind {
            return Err(refuse(domain));
        }
        if claim.origin != Origin::ActiveMain(raw.pin.owner)
            || !raw.pin.output
            || !raw.pin.owner.contains_pin(raw.pin.range)
        {
            return Err(refuse(
                "producer is promoted, synthetic, or belongs to a different raw owner/scope",
            ));
        }
        let supported = match (&claim.kind, &raw.profile) {
            (Kind::DeclaredCall, Profile::DeclaredCall) => true,
            (
                Kind::Source(source, path),
                Profile::XmlField {
                    path: expected,
                    attribute,
                },
            ) => {
                source.format == ComponentFormat::Xml
                    && source.is_source
                    && !source.schema.repeating
                    && !source.is_variable
                    && !source.is_pass_through
                    && source.options.external_source.is_none()
                    && !source.options.local_xml_file_set
                    && source.compute_when_key.is_none()
                    && !source.ports.values().any(|path| {
                        path.iter().any(|segment| {
                            matches!(
                                segment.as_str(),
                                schema::SOURCE_DOCUMENT_PATH_PORT
                                    | schema::SOURCE_INPUT_DOCUMENT_PATH_PORT
                            )
                        })
                    })
                    && source.db_queries.is_empty()
                    && source.dynamic_json.is_none()
                    && *path == expected
                    && source.unambiguous_declared_output(key)
                    && exact_path(&source.schema, path).is_some_and(|node| {
                        node.attribute == *attribute
                            && if role == "scope driver" {
                                matches!(node.kind, ir::SchemaKind::Group { .. })
                            } else {
                                !node.repeating
                                    && matches!(node.kind, ir::SchemaKind::Scalar { .. })
                            }
                    })
            }
            (Kind::Function(function), Profile::Function { inputs, outputs }) => {
                // Reserve checked materializer slots/text, not unrelated values
                // elsewhere in the mapping. Ownership never compares values.
                budget.claim(
                    function.inputs.len().saturating_mul(4).saturating_add(16),
                    function
                        .name
                        .len()
                        .saturating_add(function.inputs.len().saturating_mul(16))
                        .saturating_add(
                            function
                                .constant
                                .as_ref()
                                .map_or(0, |(value, ty)| value.len().saturating_add(ty.len())),
                        ),
                    0,
                )?;
                role != "scope driver"
                    && function.inputs == *inputs
                    && function.outputs == *outputs
                    && function.raw_single_output_valid
                    && function.db_where.is_none()
                    && function.recursive.is_none()
                    && matches!(function.library.as_str(), "core" | "lang")
                    && (function.kind == 2 && function.raw_literal_valid
                        || function.kind == 5
                            && function::map_component_name(function).is_some()
                            && !matches!(
                                function.name.as_str(),
                                "sleep" | "create-guid" | "random" | "resolve-filepath"
                            ))
            }
            (Kind::Blocking(domain), _) => return Err(refuse(domain)),
            _ => false,
        };
        if supported {
            Ok(())
        } else {
            Err(refuse("unproved scalar port/path/driver profile"))
        }
    }
}

impl MainProducerView<'_> {
    fn boundary<'a>(
        n: &Normalized<'a>,
        boundary: &super::Boundary,
        budget: &ConstructionBudget,
    ) -> Result<&'a SchemaComponent, String> {
        let refuse = || {
            format!(
                "declared-library post-normalization ownership certificate refused declared output `{}` receiving target `{}`: unproved scalar boundary or scope driver",
                boundary.output, boundary.key
            )
        };
        let path = boundary.path.as_ref().ok_or_else(refuse)?;
        let mut selected = None;
        for target in n.targets {
            if n.schemas.origin_of(*target, budget)? == Origin::ActiveMain(boundary.pin.owner)
                && selected.replace(*target).is_some()
            {
                return Err(refuse());
            }
        }
        let target = selected.ok_or_else(refuse)?;
        if boundary.pin.output
            || !boundary.pin.owner.contains_pin(boundary.pin.range)
            || target.format != ComponentFormat::Xml
            || target.is_variable
            || target.is_pass_through
            || target.has_target_node_controls
            || !target.unambiguous_declared_input(boundary.key)
            || target.ports.get(&boundary.key) != Some(path)
            || !exact_path(&target.schema, path).is_some_and(|node| {
                !node.repeating && node.is_scalar() && node.attribute == boundary.attribute
            })
        {
            return Err(refuse());
        }
        for length in 1..path.len() {
            let prefix = &path[..length];
            if exact_path(&target.schema, prefix).is_some_and(|node| node.repeating)
                && !boundary
                    .scopes
                    .iter()
                    .any(|(path, key, _)| path == prefix && target.ports.get(key) == Some(path))
            {
                return Err(refuse());
            }
        }
        Ok(target)
    }
}

pub(in crate::import) fn certify(
    registry: &Registry,
    calls: &[Call],
    normalized: Normalized<'_>,
    structure: roxmltree::Node<'_, '_>,
) -> Result<ContextSeal, String> {
    if !calls.iter().any(|call| call.certificate.is_some()) {
        return Ok(ContextSeal::default());
    }
    let budget = registry
        .construction
        .as_ref()
        .ok_or("declared-library certificate has no construction authority")?;
    let view = MainProducerView::build(&normalized, budget, false)?;
    for call in calls {
        let Some(closure) = &call.certificate else {
            continue;
        };
        for (&key, raw) in &closure.arguments {
            view.check(key, raw, "argument feed", closure, budget)?;
        }
        for (&key, raw) in &closure.drivers {
            view.check(key, raw, "scope driver", closure, budget)?;
        }
        for (&key, raw) in &closure.outputs {
            view.check(key, raw, "declared output", closure, budget)?;
        }
    }
    context::build(&view, &normalized, calls, budget, structure)
}

#[cfg(test)]
mod tests {
    use super::super::{RawOwner, RawPin};
    use super::*;

    fn raw(node: roxmltree::Node<'_, '_>) -> RawOutput {
        let pin = node
            .descendants()
            .find(|n| n.has_tag_name("datapoint"))
            .unwrap();
        let range = pin.range();
        RawOutput {
            pin: RawPin {
                owner: RawOwner::of(node),
                range: (range.start, range.end),
                output: true,
            },
            profile: Profile::DeclaredCall,
        }
    }

    #[test]
    fn certificate_uses_immutable_xml_identity_not_identical_names_uids_or_values() {
        let doc = roxmltree::Document::parse(r#"<mapping><component><structure><children><component name="same" uid="1"><targets><datapoint key="10"/></targets></component><component name="same" uid="1"><targets><datapoint key="10"/></targets></component></children></structure></component></mapping>"#).unwrap();
        let nodes = doc
            .descendants()
            .filter(|n| n.attribute("uid") == Some("1"))
            .collect::<Vec<_>>();
        let proof = raw(nodes[0]);
        let budget = ConstructionBudget::default();
        let mut view = MainProducerView {
            claims: BTreeMap::new(),
        };
        view.add(
            10,
            Origin::ActiveMain(RawOwner::of(nodes[1])),
            Kind::DeclaredCall,
            &budget,
        )
        .unwrap();
        assert!(
            view.check(10, &proof, "declared output", &Default::default(), &budget)
                .is_err()
        );
        view.claims.clear();
        view.add(
            10,
            Origin::ActiveMain(proof.pin.owner),
            Kind::DeclaredCall,
            &budget,
        )
        .unwrap();
        view.check(10, &proof, "declared output", &Default::default(), &budget)
            .unwrap();
        view.add(
            10,
            Origin::ActiveMain(proof.pin.owner),
            Kind::DeclaredCall,
            &budget,
        )
        .unwrap();
        assert!(
            view.check(10, &proof, "declared output", &Default::default(), &budget)
                .unwrap_err()
                .contains("competing normalized")
        );
    }

    #[test]
    fn certificate_inventory_budget_is_preallocation_and_sticky() {
        let budget = ConstructionBudget::for_test(2, 0);
        let mut view = MainProducerView {
            claims: BTreeMap::new(),
        };
        view.blocking(10, Origin::SyntheticOrUnproved, "unproved", &budget)
            .unwrap();
        assert!(
            view.blocking(11, Origin::SyntheticOrUnproved, "unproved", &budget)
                .is_err()
        );
        assert_eq!(view.claims.len(), 1);
        assert!(!view.claims.contains_key(&11));
        assert!(budget.claim(0, 0, 0).is_err());
    }

    #[test]
    fn every_unproved_resolver_domain_remains_a_blocking_claim() {
        let doc = roxmltree::Document::parse(
            r#"<component><targets><datapoint key="10"/></targets></component>"#,
        )
        .unwrap();
        let proof = raw(doc.root_element());
        for domain in [
            "join",
            "hierarchical alias",
            "refused join",
            "database computed projection",
            "external scalar",
            "external XSLT",
            "JSON parser",
            "XML serializer",
            "JSON serializer",
            "FlexText/fixed parser",
            "dynamic intermediate",
            "local UDF",
            "source-node rule",
        ] {
            let budget = ConstructionBudget::default();
            let mut view = MainProducerView {
                claims: BTreeMap::new(),
            };
            view.blocking(10, Origin::SyntheticOrUnproved, domain, &budget)
                .unwrap();
            assert!(
                view.check(10, &proof, "argument feed", &Default::default(), &budget)
                    .unwrap_err()
                    .contains(domain)
            );
        }
    }

    #[test]
    fn checked_constant_text_is_reserved_before_graph_materialization() {
        let doc = roxmltree::Document::parse(r#"<component name="constant" library="core" kind="2"><targets><datapoint key="10"/></targets><data><constant value="abc" datatype="string"/></data></component>"#).unwrap();
        let node = doc.root_element();
        let function = function::read(&node);
        let mut proof = raw(node);
        proof.profile = Profile::Function {
            inputs: Vec::new(),
            outputs: vec![10],
        };
        for text in [16, 17] {
            let budget = ConstructionBudget::for_test(18, text);
            let mut view = MainProducerView {
                claims: BTreeMap::new(),
            };
            view.add(
                10,
                Origin::ActiveMain(proof.pin.owner),
                Kind::Function(&function),
                &budget,
            )
            .unwrap();
            let result = view.check(10, &proof, "argument feed", &Default::default(), &budget);
            assert_eq!(result.is_ok(), text == 17);
            if text == 16 {
                assert!(budget.claim(0, 0, 0).is_err());
            }
        }
    }
}
