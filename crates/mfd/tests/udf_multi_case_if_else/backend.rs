use std::collections::BTreeMap;

use codegen::{Expression, ExpressionNode, Program, ScalarTargetDomain, TargetConstruction};
use ir::{ScalarType, Value};
use mapping::{FunctionParameterId, NodeId, Project};

use super::Tree;

/// Checks the complete lowered Choice tree and optional Marker, then emitter
/// acceptance. This does not compile or execute either generated artifact.
pub(super) fn assert_structure(project: &Project, expected: &Tree) {
    let program = codegen::lower(project).expect("conditional fixture must lower");
    let marker = project.target.child("Marker").is_some();
    assert_eq!(
        program
            .root
            .bindings
            .iter()
            .map(|binding| binding.target_field.as_str())
            .collect::<Vec<_>>(),
        if marker {
            vec!["Choice", "Marker"]
        } else {
            vec!["Choice"]
        }
    );
    assert_eq!(program.root.construction, TargetConstruction::Group);
    assert!(program.root.iteration.is_none());
    assert!(!program.root.repeating);
    assert!(program.root.children.is_empty());
    assert!(program.extra_targets.is_empty());
    assert!(program.failure_rules.is_empty());
    for binding in &program.root.bindings {
        assert_eq!(
            binding.target_domain,
            ScalarTargetDomain::Single(ScalarType::String)
        );
        assert!(!binding.repeating);
    }

    let mut normalizer = Normalizer {
        program: &program,
        frames: vec![Frame {
            expressions: &program.expressions,
            arguments: BTreeMap::new(),
        }],
        remaining: 10_000,
    };
    // Only the actual root binding selects Choice, never an arbitrary If node.
    assert_eq!(
        normalizer.tree(0, program.root.bindings[0].expression, 0),
        *expected
    );
    if marker {
        assert_eq!(
            normalizer.tree(0, program.root.bindings[1].expression, 0),
            Tree::Literal(Value::String("D".into()))
        );
    }

    // Acceptance alone is not an independent oracle for emitted semantics.
    codegen_rust::emit(
        &program,
        &codegen_rust::Options {
            package_name: "udf-multi-case-if-else".into(),
            runtime_dependency: codegen_rust::RuntimeDependency::Version(
                env!("CARGO_PKG_VERSION").into(),
            ),
        },
    )
    .expect("Rust emitter must accept the checked program");
    codegen_csharp::emit(&program).expect("C# emitter must accept the checked program");
}

struct Frame<'a> {
    expressions: &'a [ExpressionNode],
    // A parameter ID refers to an actual expression in its caller's namespace.
    arguments: BTreeMap<FunctionParameterId, (usize, NodeId)>,
}

struct Normalizer<'a> {
    program: &'a Program,
    frames: Vec<Frame<'a>>,
    remaining: usize,
}

impl<'a> Normalizer<'a> {
    fn tree(&mut self, frame: usize, id: NodeId, depth: usize) -> Tree {
        assert!(
            depth < 128,
            "cyclic or excessively deep lowered expression at frame {frame}, node {id}"
        );
        self.remaining = self
            .remaining
            .checked_sub(1)
            .expect("lowered expression traversal exceeded its finite budget");
        let expressions: &'a [ExpressionNode] = self.frames[frame].expressions;
        let mut matches = expressions.iter().filter(|node| node.id == id);
        let node = matches.next().expect("missing lowered expression");
        assert!(matches.next().is_none(), "duplicate lowered expression ID");
        match &node.expression {
            Expression::SourceField { frame: None, path } if path.len() == 1 => {
                assert_eq!(frame, 0, "source field inside a user-function body");
                Tree::Variable(path[0].clone())
            }
            Expression::Const { value } => Tree::Literal(value.clone()),
            Expression::FunctionParameter { parameter } => {
                let (caller, actual) = self.frames[frame]
                    .arguments
                    .get(parameter)
                    .copied()
                    .expect("unbound lowered function parameter ID");
                self.tree(caller, actual, depth + 1)
            }
            Expression::Call { function, args } => Tree::Call(
                function.as_str().into(),
                args.iter()
                    .map(|argument| self.tree(frame, *argument, depth + 1))
                    .collect(),
            ),
            Expression::If {
                condition,
                then,
                else_,
            } => Tree::If(
                Box::new(self.tree(frame, *condition, depth + 1)),
                Box::new(self.tree(frame, *then, depth + 1)),
                Box::new(self.tree(frame, *else_, depth + 1)),
            ),
            Expression::UserFunctionCall { function, args } => {
                let program: &'a Program = self.program;
                let mut matches = program
                    .user_functions
                    .iter()
                    .filter(|candidate| candidate.id == *function);
                let definition = matches.next().expect("missing lowered user function");
                assert!(matches.next().is_none(), "duplicate lowered function ID");
                assert_eq!(definition.parameters.len(), args.len());
                // Inspect even unused actual arguments; Tree substitution does
                // not model eager evaluation or scalar boundary adaptation.
                for argument in args {
                    self.tree(frame, *argument, depth + 1);
                }
                // Ordinals associate arguments with the actual lowered
                // parameter order; body references resolve by parameter ID.
                let arguments: BTreeMap<_, _> = definition
                    .parameters
                    .iter()
                    .zip(args)
                    .map(|(parameter, argument)| (parameter.id, (frame, *argument)))
                    .collect();
                assert_eq!(arguments.len(), definition.parameters.len());
                let callee = self.frames.len();
                self.frames.push(Frame {
                    expressions: &definition.expressions,
                    arguments,
                });
                let result = self.tree(callee, definition.output, depth + 1);
                self.frames.pop().unwrap();
                result
            }
            other => panic!("unexpected lowered expression in conditional fixture: {other:?}"),
        }
    }
}
