# Owned XML mixed-content construction corpus

This self-authored corpus checks `XmlMixedContent` constructions on scopes
with an owned source selector in the interpreter, generated Rust and generated
C#. See
[the code-generation profile](../../docs/code-generation.md#explicit-owned-xml-mixed-content-construction)
and [the ownership design](../../docs/design/source-ownership.md#owned-xml-mixed-content-construction).

- [`projects.rs`](projects.rs) builds each model and the legacy control. The
  primary endpoint and the named `notes` and `Para` endpoints share one schema,
  so only the owner distinguishes their paragraphs. Every constructed
  paragraph renames `b` to upper-cased `Strong` values (dropping `skip`) and
  `i` to `Em` values holding the `n` attribute.
- [`cases.json`](cases.json) lists each case's inputs and the complete expected
  target structures of every call, including the constructed ordered streams,
  or a typed error.

Inputs and expected structures use the compact encoding of
[the atomization corpus](../basic-owned-xml-mixed-content/README.md): a `~`
member lists a group's ordered text nodes and `{name: text}` elements, and each
element retains the next same-named field occurrence of that group. In an
expected target group the elements name the renamed target fields. Scope-level
failures carry a `null` node.

| Model | Coverage |
|---|---|
| `C01-endpoints` | Colliding primary, `notes` and `Para` paragraphs; renamed children, a filtered child leaving an exhausted occurrence, a Null child value, text-only paragraphs, absent paragraphs, and unsupplied owners |
| `C02-nested` | Paragraphs constructed inside owned rows reading their row's `id` through the enclosing frame; rows without paragraphs, text-only paragraphs and empty iterations |
| `C03-root-selection` | An endpoint-root selection inside a primary paragraph attaches the named root, never the enclosing paragraph; a child reading another endpoint only when reached; an unsupplied root selection |

The CLI harness
[`basic_owned_xml_mixed_construction.rs`](../../crates/cli/tests/code_generation/basic_owned_xml_mixed_construction.rs)
runs every call through the interpreter, writes one constructed document's
mixed XML bytes, checks that the models lower, validate and emit with the
explicit profile, and checks older-profile, grouped, inherited and dynamic-owner
refusals. Its ignored test builds the generated Rust and C# hosts and compares
every call:

```sh
cargo test -p cli --test code_generation --features codegen-tests basic_xml_mixed470
cargo test -p cli --test code_generation --features codegen-tests \
  basic_xml_mixed470_generated -- --ignored
```

A Null mapped value is a writer type failure for a string target, so the
written-bytes check uses a paragraph whose `i` carries its attribute.
