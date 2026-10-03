//! Construct an exact primary-root Project only from a complete profile proof.
use super::proof::{QualifiedRootViewPlan, Rejection};
use ir::{ScalarType, SchemaKind, Value};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope};
use std::collections::BTreeMap;

pub(super) fn project(
    proof: &QualifiedRootViewPlan,
    source_options: &FormatOptions,
) -> Result<Project, Rejection> {
    let plan = proof.as_plan();
    let reject = |detail: &str| Rejection {
        code: "composition",
        detail: detail.into(),
    };
    let expected = FormatOptions {
        xml_document: true,
        xml_allow_inactive_root_type_members: true,
        xml_root_view_read_policy: true,
        ..FormatOptions::default()
    };
    if source_options != &expected {
        return Err(reject(
            "the exact explicit String root source policy is required",
        ));
    }
    for boundary in [&plan.source, &plan.target] {
        let path = std::path::Path::new(&boundary.instance_path);
        if path.extension().and_then(|ext| ext.to_str()) != Some("xml") {
            return Err(reject("the closed root profile requires local .xml paths"));
        }
    }
    if plan.source.canonical_type != plan.target.canonical_type || plan.feeds.is_empty() {
        return Err(reject(
            "one exact shared selected type and nonempty proved feeds required",
        ));
    }
    for schema in [&plan.source.schema, &plan.target.schema] {
        if !ir::primary_root_schema_is_supported(schema)
            || !ir::xml_inactive_root_type_members_are_supported(schema)
        {
            return Err(reject("closed exact primary root metadata required"));
        }
        let SchemaKind::Group { children, .. } = &schema.kind else {
            return Err(reject("physical group root required"));
        };
        if children.iter().any(|field| {
            !field.attribute
                || !matches!(
                    field.kind,
                    SchemaKind::Scalar {
                        ty: ScalarType::String
                    }
                )
        }) {
            return Err(reject(
                "only proved physical String attributes are composed",
            ));
        }
    }
    let mut nodes = BTreeMap::from([
        (
            0,
            Node::SourceRootXmlTypeEquals {
                canonical_expanded_type: plan.source.canonical_type.clone(),
            },
        ),
        (1, Node::Const { value: Value::Null }),
        (
            2,
            Node::Const {
                value: Value::String(plan.target.canonical_type.clone()),
            },
        ),
    ]);
    let mut source_nodes = BTreeMap::new();
    let mut bindings = Vec::new();
    for feed in &plan.feeds {
        let [source_name] = feed.source.path.as_slice() else {
            return Err(reject("only direct source attributes required"));
        };
        let [target_name] = feed.target.path.as_slice() else {
            return Err(reject("only direct target attributes required"));
        };
        let source = plan
            .source
            .schema
            .child(source_name)
            .ok_or_else(|| reject("source leaf has no exact physical declaration"))?;
        let target = plan
            .target
            .schema
            .child(target_name)
            .ok_or_else(|| reject("target leaf has no exact physical declaration"))?;
        if source.kind != target.kind {
            return Err(reject("exact scalar domains must agree"));
        }
        let source_node = *source_nodes
            .entry((feed.source.key, feed.source.path.clone()))
            .or_insert_with(|| {
                let id = u32::try_from(nodes.len()).expect("bounded proved feed count");
                nodes.insert(
                    id,
                    Node::SourceRootField {
                        path: feed.source.path.clone(),
                        required: source.xml_attribute_required,
                    },
                );
                id
            });
        let id = u32::try_from(nodes.len()).expect("bounded proved feed count");
        nodes.insert(
            id,
            Node::If {
                condition: 0,
                then: source_node,
                else_: 1,
            },
        );
        bindings.push(Binding {
            target_field: target_name.clone(),
            node: id,
        });
    }
    bindings.push(Binding {
        target_field: ir::XML_TYPE_FIELD.into(),
        node: 2,
    });
    let project = Project {
        source: plan.source.schema.clone(),
        target: plan.target.schema.clone(),
        source_path: Some(plan.source.instance_path.clone()),
        target_path: Some(plan.target.instance_path.clone()),
        source_options: source_options.clone(),
        target_options: FormatOptions::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: BTreeMap::new(),
        graph: Graph { nodes },
        root: Scope {
            bindings,
            ..Scope::default()
        },
    };
    if !engine::validate(&project).is_empty() {
        return Err(reject("composed project fails ordinary engine validation"));
    }
    Ok(project)
}
