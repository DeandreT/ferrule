# Owned dynamic source-field reads

A runtime-named source property can be read from the open object of exactly one
explicitly owned endpoint and frame. The owned form sits beside the legacy
`dynamic_source_field` node, whose wire, outward fallback and admission are
unchanged. It reads properties of one source object; it does not load
documents or author target properties.

## Wire form

```json
{"kind":"owned_dynamic_source_field",
 "object":{"owner":{"kind":"named","name":"Lookup"},"path":["Rows","Props"]},
 "frame":["Rows"],"key":7}
```

`object` is an [`OwnedSourceRef`](source-ownership.md#ownership-rules) whose
path is absolute within its endpoint; the empty path selects an endpoint root
that is itself an open object. `frame` is optional: when present it must prefix
the object path and is either the explicit endpoint-root pin `[]` or a
repeating collection of the same owner. `key` names the node that computes the
property name. The body is strict: unknown or duplicate keys, a missing object
or key, a legacy path array in place of the object and empty or malformed named
owners are refused. Readers that predate the kind refuse it rather than
reinterpreting it as the legacy node. The legacy body still ignores unknown
keys, so an owned-looking key never grants ownership.

## Selection and lookup

Validation requires a declared static owner and an object that is an open group
whose runtime-named properties are non-repeating scalars. A dynamic named owner
is refused.

Evaluation follows a fixed order:

1. The key is evaluated in the receiving context, before any source access.
   A key that is not a string reads nothing and yields `Null`, even when the
   owner is unsupplied.
2. The object is selected exactly like an owned scalar field. Without a frame,
   the latest active frame of the same owner whose collection prefixes the
   object path supplies the item; otherwise the immutable endpoint root is
   used. An explicit frame selects the latest exact same-owner occurrence, and
   a missing one is a typed `InactiveSourceFrame` failure. The explicit root
   pin reads a declared nonrepeating root, or the root of an active grouped
   member container. A grouped member container exposes its first surviving
   member.
3. The remaining object path is traversed strictly. An absent group or a null
   scalar selects no object; crossing an uniterated repetition is a typed
   failure and is never repaired by its first item, another frame or another
   endpoint.
4. The property is looked up on the selected object only. An absent or
   structural property is `Null`; a present scalar, including JSON null and
   XML nil, is returned unchanged. A declared child of the open object is read
   like any other property.

An unsupplied owner is a typed `SourceUnavailable` failure attributed to the
owned read. Errors raised while evaluating the key keep their own node.
Untaken conditional branches evaluate neither the key nor the owner.

## Contexts

The owned read and its key are Basic parent-context expressions: the key may
read exact owned fields of any owner, and the read creates no source frame.
Failure-rule predicates and messages, isolated user-function bodies and every
closed capability context refuse it, in the interpreter and in every generation
profile. Inside a SequenceExists predicate over a pure scalar producer the
interpreter keeps the receiving owner frames, as for every owned read; the
generation profile does not compose SequenceExists and refuses that predicate
as a generated-item context. Legacy dynamic reads stay closed inside owned
scopes.

## Generation profile

`codegen::lower_with_basic_owned_dynamic_field` adds the read to the
[Lookup/CollectionFind profile](owned-lookup-find.md#generation-profile).
The owner remains optional until reached. Presets without this capability,
ordinary generation and MFD export refuse it; the GUI preserves saved reads
and key wires read-only.

## Qualification

The [corpus](../../conformance/basic-owned-dynamic-field/README.md) distinguishes
same-named objects and properties on two endpoints, including typed failures.
The interpreter checks the calls; the compiled-host check is ignored by default:

```sh
cargo test -p cli --test code_generation --features codegen-tests \
  owned_dynamic467_generated_rust_and_csharp_match_expected -- --ignored
```
