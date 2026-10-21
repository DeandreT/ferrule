# Owned XML serialization corpus

This self-authored corpus checks `Node::OwnedXmlSerialize` in the interpreter,
generated Rust and generated C#. See
[the code-generation profile](../../docs/code-generation.md#explicit-owned-xml-serialization)
and [the ownership design](../../docs/design/source-ownership.md#owned-xml-element-serialization).

- [`projects.rs`](projects.rs) builds each model and the legacy control. The
  primary endpoint and the named `audit` and `Header` endpoints share one
  schema, so only the owner distinguishes their elements; `Header` is also
  spelled like the primary root field being serialized.
- [`cases.json`](cases.json) lists each case's primary input, supplied named
  inputs and the complete expected outcome of every call: the exact serialized
  XML text of every target field, or a typed error. `All` returns the primary
  target followed by named targets; a target name selects one output.

Inputs use a compact JSON encoding: an object is an ordered group, an array is
a repeated collection, and strings, integers and `null` are scalars. Errors are
typed recipes such as
`{"kind": "MissingElement", "owner": null, "path": ["Header"], "node": 1}`,
where a `null` owner is the primary endpoint and `node` is the graph node the
failure is attributed to.

| Model | Coverage |
|---|---|
| `X01-endpoints` | Colliding primary and named elements with different attributes, text and repeated children; compact, indented, declaration and namespace options; unsupplied and missing owners; empty and null children; writer type failures |
| `X02-nested` | Pinned and unpinned reads in an owned order iteration, a foreign endpoint root inside it, a nested line iteration reading its enclosing frame, an absent second-row element, empty iterations and an unsupplied iterated endpoint |
| `X03-frames` | Explicit empty root pins inside an active row, an inactive pinned frame and an uniterated repetition |
| `X04-grouped` | Owned By groups: representative and pinned reads use the first member, member iteration reads each member, and an absent first-member element fails |
| `X05-failure-walk` | An owned failure walk over `audit` runs before the primary element is read |

The CLI harness
[`basic_owned_xml_serialize.rs`](../../crates/cli/tests/code_generation/basic_owned_xml_serialize.rs)
runs every call through the interpreter, checks that the models lower, validate
and emit with the explicit profile, and checks the refusals of older profiles,
closed contexts and directly constructed Programs. Its ignored test builds the
generated Rust and C# hosts and compares every call:

```sh
cargo test -p cli --test code_generation --features codegen-tests basic_xml_serialize469
cargo test -p cli --test code_generation --features codegen-tests \
  basic_xml_serialize469_generated -- --ignored
```

Forbidden XML 1.0 characters in serialized text are covered by the interpreter
tests only: the generated C# node writer does not reject them.
