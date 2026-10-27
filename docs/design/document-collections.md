# Nested document collections

`ScopeConstruction::CollectDocuments` preserves document boundaries through
nested source iteration and grouping. Its children produce ordered document
sets, not fields in an enclosing XML element. The corresponding generated
construction is `TargetConstruction::CollectDocuments`.

## Contract

A collector is allowed only at a project target root or directly beneath
another collector. It uses the same nonrepeating per-document group schema as
its children. Every child has an empty target field and is either another
collector or a dynamic-document scope. Ordinary target fields cannot contain
collectors or dynamic document paths.

Collectors use ordinary source iteration or no iteration, with `Repeated`
output. They have at least one child and no field bindings, computed fields,
merge mode or output-path expression. Their existing grouping, filtering,
sorting and window controls retain their normal evaluation contexts.

Each selected context contributes one document set, even when that set is
empty. Only finalization concatenates the sets in context and child order.
Thus an empty parent still counts toward scope positions, while a parent
producing several documents does not consume several context positions.
Ordinary `Position` resolution is unchanged.

Children share the current execution context and limits. A late failure aborts
the result; it never returns an apparently successful partial document set.
Document paths stay attached to their values. Existing CLI publication and
generated XML boundaries retain their path, document-count and byte limits.
XML driver validation resolves each leaf against its real parent source
context, including grouped and relative paths.

## MFD import

The bounded importer recognizes a constructed XML variable with two explicit
parent contexts: a key-grouped outer collection and a fixed-block inner
collection below it. One scalar filename combines the outer group key, the
inner block position and literal strings; one connected repeated group supplies
the document contents. All connected variable inputs must belong to that shape.
Recognition and materialization are transactional: a failed proof leaves the
ordinary import and its warnings intact.

A variable's document-level `cast-in-subtree` is admitted only as an identity:
the filename is already a String and the copied group's complete source and
variable schemas agree. The cast proof requires a local XSD using the supported
primitive scalar types; imported or aliased types and date/time casts are not
inferred from a runtime String tag. Descendant casts, input/output-node functions
and casts that need adaptation remain refused. Target-level processing runs
before the target is wrapped in its collector.

Native MFD export refuses collectors until an exact native representation is
qualified. The GUI preserves them as read-only scope construction rather than
converting them into ordinary fields. This does not admit new owned-source
presets or any previously refused scalar-sequence export.
