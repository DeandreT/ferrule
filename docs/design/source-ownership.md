# Explicit source ownership: contributor capability matrix

An explicit source reference names an endpoint and a path within it. Reading,
iterating and generating code for that reference each require an admitted
operation; saving the reference does not authorize any of them.

## Ownership rules

`SourceOwner` is either Primary or one exact, nonempty named endpoint. Do not
trim names, resolve aliases or borrow a same-path value from another endpoint.
`OwnedSourceRef` paths are endpoint-local: `[]` selects the root; an absent
frame differs from an explicit root frame. An inherited selector, a generated
item, a function input and a `JoinId` each have a different identity.

Validate the wire form separately from each executable capability, including
hand-built Programs. Check potentially reached branches, calls and required
paths without changing legacy laziness. Native export checks the whole design
before publication. The editor preserves saved owners, IDs and wires.

## Native editor authoring

An ambiguous source pin authors an `OwnedSourceField`: the chosen endpoint,
absolute local path and innermost repetition (`None` outside repetition).
Ambiguity includes a named alias that any input could define and a primary
path beginning with a named alias. Once any owned form is present, all
addressable pins use owned reads. Blank/duplicate aliases and per-item dynamic
inputs are unaddressable; other legacy projects keep `SourceField`.

Owned fields appear as canvas nodes. Ordinary constructed scopes can choose
owner-qualified iteration; positions can choose an owner and collection.
Showing a legacy form never converts it. Per-item document outputs and
specialized scopes stay read-only.

**Lookup from one input** creates a disconnected needle with valid local
collection, key and return fields. Changing the endpoint or collection keeps
valid fields, defaults others to the first available field, and leaves the
needle and wires alone. **Use an exact input** converts a saved legacy Lookup
only on request; an invalid saved path is not repaired on display.

The [aggregate editor](owned-collection-aggregates.md#native-editor) offers
Count and direct-field Sum, Average, Minimum and Maximum. Lookup and these
aggregates use static endpoints (at most 4,096 schema nodes and 64 levels
each). Dynamic inputs, document sets, unavailable owners, functions, grouped
scopes and their descendants stay read-only for these operations. Other owned
operators, including CollectionFind, have no such authoring permission.

The same check applies to creation, assignment, conversion and needle edits.
It walks potential branches, consumers and grouping ancestry: adding grouping
cannot invalidate an existing descendant, and controls skip unsuitable
defaults. An excluded child does not disable an ordinary sibling. Computed
properties, filter/map and failures are outside this inspector subset;
presence conditions have their own admission. Rejected edits leave graphs,
wires and history intact. Typed node/wire commands and scope snapshots
preserve Undo/Redo, node identity and layout on primary and named canvases.

For generated entry points, see the [capabilities and presets](../code-generation.md#owned-generation-capabilities-and-presets).

## Scalar target construction

A scalar target scope evaluates one expression in its receiving context and
creates no source frame, so it is a common Basic context rather than a
specialized construction. Its owned reads resolve exactly as they would in a
binding of the same scope: an enclosing owned or ordinary iteration supplies
the active frames, and the scalar target domain applies the ordinary exact
numeric adaptation. Exact whole-group copies remain a closed special
construction. See [scalar target generation](../code-generation.md#owned-reads-in-scalar-targets).

## Group-member occurrence identity

An owned By, AdjacentBy, StartingWith, EndingWith or IntoBlocks group carries a private grouped occurrence at its exact owner and
absolute collection path. It exposes the first surviving ordered member for
scalar reads, a compact group ordinal for positions, and its complete ordered
members for nested collection iteration. A nested member creates an ordinary
occurrence with its own position; other owners and incoming ancestor frames are
preserved. An explicit empty frame at a group-member root reads the immutable
endpoint root. The latest exact-owner frame governs selection, so a nested
ordinary repeated-root member retains its active-item pin behavior.

Group provenance comes from an authenticated source-walk terminal observation,
including root and identity selections. Wrapper labels and pointer equality do
not establish an owner. A group keeps its first member's intermediate prefix;
enumerating members does not reconstruct each member's original crossed-parent
frames. Wrapper-origin redirects preserve the endpoint-local path even when a
legacy named alias names an endpoint root.

Member-retention predicates use candidate contexts and retain a whole group when
any member matches. Parent windows select retained groups after grouping; bindings
and children use the resulting grouped context. These distinct roles also govern
dependency discovery. See [the generated group-by contract](../code-generation.md#explicit-owned-group-by)
for ordering, explicit profile selection and metadata admission.

| Grouping | What establishes a group after candidate selection |
| --- | --- |
| [By](owned-group-by.md) | Equal keys belong to one group, even when their members are not adjacent. |
| [AdjacentBy](owned-group-adjacent-by.md) | A contiguous run of equal keys. Dropping an intervening run does not merge its neighbors. |
| [StartingWith](owned-group-starting-with.md) | Keep the first false prefix; each subsequent true boundary starts a group. |
| [EndingWith](owned-group-ending-with.md) | A true boundary closes its group; keep the trailing members. |
| [IntoBlocks](owned-group-into-blocks.md) | Evaluate a positive size once in the parent after window bounds. Keep a partial last block; dropping one never refills it. |

## Parent-context scalar sequence selection

[ItemAt](owned-sequence-item-at.md) finishes Generate, Tokenize,
TokenizeByLength or TokenizeRegex before evaluating its index once, even for
an empty sequence. Both phases keep the receiving frames; no item frame is
created. Its consumer permission is separate from target and failure controls.

## Failure-rule endpoint walks

An [owned failure rule](owned-failure-sources.md) walks the declared owner:
absent or empty collections yield no rows; an unsupplied owner fails. Rule
order, selection and lazy messages follow the legacy contract.

## Owned path-hierarchy input collections

`OwnedPathHierarchyPlan` pairs an `OwnedSourceRef` input collection with the
legacy separator and generated directory/file/name fields. It is a separate
`owned_path_hierarchy` construction beside the unchanged legacy plan, so older
readers reject it. Its strict reader rejects unknown fields, malformed owners
and legacy path arrays, and keeps the output-structure rules.

The collection is an owned read in the receiving context and creates no source
frame. A same-owner active frame pins its prefix; otherwise the immutable
endpoint root is read, including an endpoint whose root is the collection. The
operator evaluates no expression and admits only Basic contexts. Interpreter,
generated Rust and generated C# share the legacy builder's ordering, bounds and
error categories. Native export and GUI authoring remain closed.

## Owned adjacency-tree row collections

`OwnedAdjacencyTreePlan` pairs an `OwnedSourceRef` row collection with the
legacy row-relative key and parent paths, the generated target key and children
fields, and the optional root expression. It is a separate
`owned_adjacency_tree` construction beside the unchanged legacy plan, so older
readers reject it. Its strict reader rejects unknown fields, malformed owners
and legacy path arrays, and keeps the row and output rules.

The rows are an owned read in the receiving context and create no source frame.
A same-owner active frame pins its prefix; otherwise the immutable endpoint root
is read, including an endpoint whose root is the collection. Key and parent
paths are read from each selected row, never resolved as endpoints. The root
expression keeps the receiving (parent) context and is evaluated after the rows.
The construction admits only Basic contexts. Interpreter, generated Rust and
generated C# share the legacy builder's ordering, bounds and error categories.
Native export and GUI authoring remain closed.

## Recursive collection roots

[RecursiveCollect](owned-recursive-collect.md) selects only its root by
endpoint. Child, descent and value paths are local to each group; the item
is private. Only Basic parent-context ItemAt consumes this owned producer.

## Generated-item predicates

[Exists](owned-sequence-exists.md) uses the four scalar producers admitted
by ItemAt. A private item frame supplies the predicate's ordinary reads;
owned reads keep their receiving pins and positions. Scanning stops at
the first `true`; empty input never evaluates the predicate.

## Owned recursive filtering

[RecursiveFilter](owned-recursive-filter.md) takes its only owner from the
source selector. Child and item paths stay local to the visited group; the
predicate can read its current item and innermost group position.

## Owned search

[Lookup and CollectionFind](owned-lookup-find.md) walk the collection anchored
by its latest same-owner prefix. A lookup's needle runs once in the parent;
its key and return path are item-relative. Find evaluates its predicate and
return expression in the item context. The first match wins; a miss is `Null`.

## Owned collection aggregates

[Collection aggregates](owned-collection-aggregates.md) use the search walk.
A value path or expression reads each selected item; a separator or index
runs once in the parent after the items, even for an empty collection.

## Owned join identity

An owned inner join names each source and each condition's left collection by
owner plus endpoint-relative collection. It shares the scope's `join` key with
the legacy plan, but every collection is an owned object, so a reader that only
knows collection arrays refuses it, and one plan never mixes the two forms.
Duplicate, singleton and left-stage checks compare the whole identity: a
primary `Rows` and a named `Rows` are two sources, while the same owner's `Rows`
twice is still a duplicate. The scope's `JoinId` stays independent of these
identities. Validation resolves each source against its own endpoint and each
key path under the exact collection it names. `.mfd` export and GUI editing
refuse owned joins; the GUI shows a saved one read-only.

## Owned join tuples

[Join members](owned-joins.md#members-and-tuples) keep their own endpoint
and raw source position. Tuples retain left-major order and duplicates;
null-like keys never match. `join_position` is the flattened tuple
position and changes through filtering, sorting and compaction.

## Owned joined aggregates

A [joined aggregate](owned-joins.md#joined-aggregates) runs its owned plan
in the receiving context. Only this join's members are active in the tuple
expression; the argument runs once back in the parent. A group contributes
its own members while another owner with the same path keeps its own walk.

## Dynamic source-field objects

A [dynamic field](owned-dynamic-field.md) evaluates its key in the parent
before accessing the declared owner's open object. A missing property is
`Null`. A root path, an absent frame and an explicit root pin remain distinct.

## Dynamic driver loading

[Dynamic loading](owned-dynamic-loading.md) roots each document under its
own driver row. Nested driver reads and positions cannot borrow another
driver's frame. A legacy alias never selects an owned dynamic source.

## Document-set paths

An ungrouped owned scope enters each member in set order. Nested reads of
that owner stay in the active document. The [document-path expression](owned-document-path.md)
returns its location or `MissingDocumentPath`; another owner or ancestor
cannot supply it. Grouped scopes and other owned walkers refuse document sets.

## Generated-item reductions

A [sequence aggregate](owned-sequence-aggregate.md) shares ItemAt's four
scalar producers. Only the predicate and value expression see the private
item and its raw position. The argument runs in the receiving context,
including for an empty sequence.

## Owned XML element serialization

`OwnedXmlSerialize` pairs an `OwnedSourceRef` element path and optional
endpoint-local frame with the legacy serializer's schema and document options.
It is a separate `owned_xml_serialize` graph node beside the unchanged legacy
`xml_serialize`, so older readers reject it. Its strict reader rejects unknown
fields, malformed owners, legacy paths and empty namespaces, and requires the
declaration and indent options explicitly.

The element is an owned read in the receiving context with the owned field's
frame rules; it creates no source frame and evaluates no expression of its own.
Absence in the selected owner is a typed failure rather than a fallback, and an
uniterated repetition is never reduced to its first item. The schema-known
subtree reaches the unchanged writer. Only Basic target contexts admit it;
failure rules, generated items, functions and other specialized contexts refuse
it. Interpreter, generated Rust and generated C# share the selection, writer
bytes and error categories. Native export and GUI authoring remain closed.

## Owned XML mixed-content atomization

`OwnedXmlMixedContent` pairs an `OwnedSourceRef` mixed group and optional
endpoint-local frame with ordered replacements. It is a separate
`owned_xml_mixed_content` graph node beside the unchanged legacy
`xml_mixed_content`, so older readers reject it. Each replacement names its
direct child element and an explicit `parent` or `occurrence` context; there is
no legacy collection path, because an occurrence's collection is always the
group path plus the element name in the group's own endpoint. Its strict reader
rejects unknown fields, malformed owners, legacy paths and empty elements.

The group is an owned read with the serializer's frame rules, and its absence
is a typed failure rather than a fallback. A parent replacement evaluates in the
receiving context. An occurrence replacement appends one owned occurrence of
the retained element value at that collection, numbered per element name from
one; other owners, outer frames and positions are unchanged, so another
endpoint's read of the same path is never served by it. Only Basic target
contexts admit the node; failure rules, generated items, functions and other
specialized contexts refuse it. Interpreter, generated Rust and generated C#
share the selection, order, laziness and error categories. Native export and GUI
authoring remain closed.

## Owned XML mixed-content construction

An `XmlMixedContent` construction on a scope with an owned source selector
takes its retained text/element order from that selector's exact current
occurrence, or the immutable endpoint root for an endpoint-root selection. The
selector is the construction's only owner, so the wire adds no field. Another
endpoint's frame and the innermost legacy frame never supply the stream, and
mapped direct children come from the item's own bindings and child scopes.
Grouped owned selectors stay closed, because one constructed item would combine
a representative's stream with every member's children; inherited and
legacy-selected constructions inside owned ancestry stay XML operators.
Interpreter, generated Rust and generated C# share the selection and the legacy
construction's ordering, renaming and omission rules. Native export and GUI
authoring remain closed; the GUI shows the saved scope read-only.

<a id="existing-coordinated-issues"></a>
<a id="specialized-follow-on-issues"></a>

The [original ownership design](https://github.com/DeandreT/ferrule/issues/432)
and [operator breakdown](https://github.com/DeandreT/ferrule/issues/452)
give the history. The contracts above describe each operator's supported
shape. For example, an owned dynamic field does not authorize document
loading, and a scoped sequence consumer does not authorize an arbitrary
endpoint as its producer. Legacy native planners retain their own warning
and skip paths; they are not an export strategy for explicit owners.

## Pipeline admission is all-stage, all-target

The engine evaluates every stage and every primary/named stage target,
including disconnected stages and targets not selected for publication.
Selection controls which outputs are published. Filesystem and preview hosts must check capabilities
before acquisition, including called graphs and potential branches, with
structural and identity errors retaining precedence. An unsupported
disconnected stage must not reach a loader, parser or observer.

<a id="acceptance-checklist"></a>

The [portable cohorts](../../conformance/README.md#portable-basic-cohorts)
exercise colliding owners, missing sources, pinned frames, phase order and
typed failures. Interpreter results, compiled Rust/C# calls, native export
and editor authoring are separate evidence.
