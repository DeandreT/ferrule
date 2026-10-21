# Owned XML mixed-content corpus

This self-authored corpus checks `Node::OwnedXmlMixedContent` in the
interpreter, generated Rust and generated C#. See
[the code-generation profile](../../docs/code-generation.md#explicit-owned-xml-mixed-content)
and [the ownership design](../../docs/design/source-ownership.md#owned-xml-mixed-content-atomization).

- [`projects.rs`](projects.rs) builds each model and the legacy control. The
  primary endpoint and the named `notes` and `Para` endpoints share one schema,
  so only the owner distinguishes their paragraphs; `Para` is also spelled like
  the primary root field holding the paragraph.
- [`cases.json`](cases.json) lists each case's primary input, supplied named
  inputs and the complete expected outcome of every call: the exact atomized
  string of every target field, or a typed error. `All` returns the primary
  target followed by named targets; a target name selects one output.

Inputs use a compact JSON encoding: an object is an ordered group, an array is
a repeated collection, and strings, integers and `null` are scalars. A `~`
member lists a mixed group's ordered content: strings are text nodes and
`{name: text}` objects are elements. The harness expands it like the XML
readers: each element retains the next same-named field occurrence of the
group as its value (or its text when the group has no such field), and the
retained stream follows the group's fields. Errors are typed recipes such as
`{"kind": "MissingElement", "owner": null, "path": ["Para"], "node": 1}`,
where a `null` owner is the primary endpoint and `node` is the graph node the
failure is attributed to.

| Model | Coverage |
|---|---|
| `M01-endpoints` | Colliding primary, `notes` and `Para` paragraphs with different text and children; occurrence numbering and values; parent reads; a missing primary paragraph beside a same-named endpoint; unsupplied owners; text-only, empty and whitespace content; Null replacements; unmatched elements |
| `M02-nested` | Pinned and unpinned paragraphs in an owned row iteration, row and occurrence positions together, an absent second-row paragraph, empty iterations, and an occurrence reading another endpoint only when reached |
| `M03-frames` | An explicit empty root pin inside active rows, an inactive pinned frame, an uniterated repetition, and another endpoint's read crossing its own repetition inside an occurrence |
| `M04-lazy` | Unmatched rules never evaluated, a failure raised at the second occurrence, and a reached parent rule reading a supplied or unsupplied endpoint |

The CLI harness
[`basic_owned_xml_mixed_content.rs`](../../crates/cli/tests/code_generation/basic_owned_xml_mixed_content.rs)
runs every call through the interpreter, checks that the models lower, validate
and emit with the explicit profile, and checks the refusals of older profiles,
closed contexts and directly constructed Programs. Its ignored test builds the
generated Rust and C# hosts and compares every call:

```sh
cargo test -p cli --test code_generation --features codegen-tests basic_xml_mixed468
cargo test -p cli --test code_generation --features codegen-tests \
  basic_xml_mixed468_generated -- --ignored
```
