//! Original, invented physical-document mappings; no imported project or CLI fixture.
use codegen::{
    Binding, DocumentBoundaryOptions as Options, Expression, ExpressionNode,
    NamedDocumentBoundaryOptions, NamedSourceProgram, NamedTargetProgram, Program, RuntimeValue,
    StaticDocumentBoundaryPolicy, TargetConstruction, TargetScope, X12BoundaryOptions,
    X12EnvelopeProfile,
};
use ir::{JsonAllowedValue, JsonAllowedValues, ScalarType, SchemaNode, Value};
use mapping::{X12Autocomplete, X12Separators};

fn text(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}

fn object(name: &str, fields: &[&str]) -> SchemaNode {
    SchemaNode::group(name, fields.iter().map(|name| text(name)).collect())
        .with_required_fields(fields.iter().map(|name| (*name).into()).collect())
        .unwrap()
}

fn segment(name: &str, count: usize, fixed: &[(usize, &str)]) -> SchemaNode {
    SchemaNode::group(
        name,
        (1..=count)
            .map(|index| {
                let mut field = text(&format!("{name}{index:02}"));
                field.fixed = fixed
                    .iter()
                    .find(|(slot, _)| *slot == index)
                    .map(|(_, value)| (*value).into());
                field
            })
            .collect(),
    )
}

fn interchange(modern: bool, grouped: bool) -> SchemaNode {
    let isa = segment(
        "ISA",
        16,
        &[(12, if modern { "00501" } else { "00401" }), (16, ":")],
    );
    let gs = segment(
        "GS",
        8,
        &[(7, "X"), (8, if modern { "005010" } else { "004010" })],
    );
    let st = segment("ST", 2, &[(1, "940")]);
    let body = segment("W05", 2, &[(1, "N")]);
    let se = segment("SE", 2, &[]);
    let ge = segment("GE", 2, &[]);
    let iea = segment("IEA", 2, &[]);
    let fields = if grouped {
        vec![
            isa,
            SchemaNode::group(
                "Groups",
                vec![
                    gs,
                    SchemaNode::group("Transactions", vec![st, body, se]).repeating(),
                    ge,
                ],
            )
            .repeating(),
            iea,
        ]
    } else {
        vec![isa, gs, st, body, se, ge, iea]
    };
    SchemaNode::group("Interchange", fields)
}

#[derive(Default)]
struct Graph(Vec<ExpressionNode>);
impl Graph {
    fn add(&mut self, expression: Expression) -> u32 {
        let id = self.0.len() as u32 + 1;
        self.0.push(ExpressionNode { id, expression });
        id
    }
    fn constant(&mut self, value: &str) -> u32 {
        self.add(Expression::Const {
            value: Value::String(value.into()),
        })
    }
    fn field(&mut self, path: &[&str]) -> u32 {
        self.add(Expression::SourceField {
            frame: None,
            path: path.iter().map(|name| (*name).into()).collect(),
        })
    }
}

fn scope(name: &str, fields: &[(&str, u32)], children: Vec<TargetScope>) -> TargetScope {
    TargetScope {
        target_field: name.into(),
        repeating: false,
        iteration: None,
        construction: TargetConstruction::Group,
        bindings: fields
            .iter()
            .map(|(name, id)| Binding {
                target_field: (*name).into(),
                expression: *id,
                target_domain: ScalarType::String.into(),
                repeating: false,
            })
            .collect(),
        children,
    }
}

fn named(name: &str, options: Options) -> NamedDocumentBoundaryOptions {
    NamedDocumentBoundaryOptions {
        name: name.into(),
        options,
    }
}

pub fn mixed() -> (Program, StaticDocumentBoundaryPolicy) {
    let mut graph = Graph::default();
    let message = graph.field(&["message"]);
    let reference = graph.field(&["Reference", "W05", "W0502"]);
    let tag = graph.add(Expression::RuntimeParameter {
        name: "tag".into(),
        ty: ScalarType::String,
    });
    let path = graph.add(Expression::RuntimeValue {
        value: RuntimeValue::MappingFilePath,
    });
    let main = graph.add(Expression::RuntimeValue {
        value: RuntimeValue::MainMappingFilePath,
    });
    let clock = graph.add(Expression::RuntimeValue {
        value: RuntimeValue::CurrentDateTime,
    });
    let sender = graph.constant("SEND");
    let receiver = graph.constant("RECV");
    let group = graph.constant("OW");
    let primary = object(
        "Summary",
        &["message", "reference", "tag", "path", "main", "clock"],
    );
    let primary_scope = scope(
        "",
        &[
            ("message", message),
            ("reference", reference),
            ("tag", tag),
            ("path", path),
            ("main", main),
            ("clock", clock),
        ],
        vec![],
    );
    let wire_scope = scope(
        "",
        &[],
        vec![
            scope("ISA", &[("ISA06", sender), ("ISA08", receiver)], vec![]),
            scope(
                "GS",
                &[("GS01", group), ("GS02", sender), ("GS03", receiver)],
                vec![],
            ),
            scope("ST", &[], vec![]),
            scope("W05", &[("W0502", message)], vec![]),
        ],
    );
    let allowed = text("result")
        .with_json_allowed_values(
            JsonAllowedValues::new([JsonAllowedValue::String("ALLOW".into())]).unwrap(),
        )
        .unwrap();
    let alpha = SchemaNode::group("Restricted", vec![allowed])
        .with_required_fields(vec!["result".into()])
        .unwrap();
    let mut unused = SchemaNode::group(
        "Unused",
        vec![SchemaNode::scalar("permit", ScalarType::Bool)],
    );
    unused = unused.with_required_fields(vec!["permit".into()]).unwrap();
    let program = Program {
        xml_boundary: None,
        source: object("Request", &["message"]),
        extra_sources: vec![
            NamedSourceProgram {
                name: "Reference".into(),
                source: interchange(false, false),
                dynamic: None,
            },
            NamedSourceProgram {
                name: "Unused".into(),
                source: unused,
                dynamic: None,
            },
        ],
        target: primary,
        root: primary_scope,
        extra_targets: vec![
            NamedTargetProgram {
                name: "Zeta".into(),
                target: object("Audit", &["tag", "reference"]),
                root: scope("", &[("tag", tag), ("reference", reference)], vec![]),
            },
            NamedTargetProgram {
                name: "Wire".into(),
                target: interchange(true, false),
                root: wire_scope,
            },
            NamedTargetProgram {
                name: "Alpha".into(),
                target: alpha,
                root: scope("", &[("result", message)], vec![]),
            },
        ],
        expressions: graph.0,
        user_functions: vec![],
        failure_rules: vec![],
    };
    let policy = StaticDocumentBoundaryPolicy {
        source: Options::Json,
        target: Options::Json,
        extra_sources: vec![
            named("Reference", Options::X12(X12BoundaryOptions::default())),
            named("Unused", Options::Json),
        ],
        extra_targets: vec![
            named("Zeta", Options::Json),
            named(
                "Wire",
                Options::X12(X12BoundaryOptions {
                    interchange_version: Some("00501".into()),
                    separators: Some(X12Separators {
                        element: '|',
                        component: ':',
                        segment: '!',
                        repetition: Some('^'),
                        release: None,
                    }),
                    autocomplete: Some(X12Autocomplete {
                        request_acknowledgement: true,
                        transaction_set: Some("940".into()),
                    }),
                    ..Default::default()
                }),
            ),
            named("Alpha", Options::Json),
        ],
    };
    (program, policy)
}

/// Both the primary input and the true named output require explicit grouped
/// ownership; the primary JSON summary is not a stand-in for the physical wire.
pub fn grouped() -> (Program, StaticDocumentBoundaryPolicy) {
    let mut graph = Graph::default();
    let control = graph.field(&["ISA", "ISA13"]);
    let schema = interchange(true, true);
    let mut copy = scope("", &[], vec![]);
    copy.construction = TargetConstruction::CopyCurrentSource;
    let program = Program {
        xml_boundary: None,
        source: schema.clone(),
        extra_sources: vec![],
        target: object("Receipt", &["control"]),
        root: scope("", &[("control", control)], vec![]),
        extra_targets: vec![NamedTargetProgram {
            name: "Explicit grouped".into(),
            target: schema,
            root: copy,
        }],
        expressions: graph.0,
        user_functions: vec![],
        failure_rules: vec![],
    };
    let options = Options::X12(X12BoundaryOptions {
        envelope_profile: X12EnvelopeProfile::GroupedTransactions,
        interchange_version: Some("00501".into()),
        ..Default::default()
    });
    (
        program,
        StaticDocumentBoundaryPolicy {
            source: options.clone(),
            target: Options::Json,
            extra_sources: vec![],
            extra_targets: vec![named("Explicit grouped", options)],
        },
    )
}

/// Primary X12, no named endpoints, no context or completion dependency.
pub fn single() -> (Program, StaticDocumentBoundaryPolicy) {
    let schema = interchange(false, false);
    let mut copy = scope("", &[], vec![]);
    copy.construction = TargetConstruction::CopyCurrentSource;
    (
        Program {
            xml_boundary: None,
            source: schema.clone(),
            extra_sources: vec![],
            target: schema,
            root: copy,
            extra_targets: vec![],
            expressions: vec![],
            user_functions: vec![],
            failure_rules: vec![],
        },
        StaticDocumentBoundaryPolicy {
            source: Options::X12(X12BoundaryOptions::default()),
            target: Options::X12(X12BoundaryOptions::default()),
            extra_sources: vec![],
            extra_targets: vec![],
        },
    )
}

pub fn primary_x12() -> (Program, StaticDocumentBoundaryPolicy) {
    let (mut program, policy) = mixed();
    let mut policy = policy;
    std::mem::swap(&mut program.target, &mut program.extra_targets[1].target);
    std::mem::swap(&mut program.root, &mut program.extra_targets[1].root);
    std::mem::swap(&mut policy.target, &mut policy.extra_targets[1].options);
    (program, policy)
}
