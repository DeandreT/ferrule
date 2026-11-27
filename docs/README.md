# Ferrule documentation

## Create or run a mapping

- [Create your first visual mapping](getting-started.md): connect fields in the
  native editor, preview a sample, and run the mapping to write an output file.
- [Run a saved mapping from the CLI](running-mappings.md): validate a project,
  pass parameters, and select an output. To map bytes your application already
  holds, use the [Rust payload API](running-mappings.md#rust-payload-host).
- [Generate a Rust or C# library](code-generation.md) from a supported mapping.
  The [example hosts](../examples/codegen/README.md) show how to call each library.
- [Try the warehouse X12 examples](../examples/warehouse-x12/README.md): read a
  940 shipping order into JSON or write a 945 shipment confirmation from JSON.
  Each example includes input and expected output.

## Work with formats and projects

- [Check format support](formats.md) for input and output directions, format
  options, and limits.
- [Import or export a `.mfd` design](mfd-interop.md) and interpret
  compatibility warnings.
- [Connect mappings in a pipeline](mapping-pipelines.md) to pass one stage's
  output to another.
- [Understand what a project file stores](project-files.md) or
  [bundle a project with its local resources](offline-bundles.md).
- For a live HTTP response, use the [opt-in JSON request host](rest-json-host.md).
  For an application that supplies its own bytes, see the [bounded document profile](design/bounded-document-payloads.md).

## Diagnose a run

- [Trace execution](tracing.md) to inspect the interpreter's decisions.
- [Navigate the canvas](canvas-presentation.md) or use the
  [editor keyboard controls](qualification/keyboard-only-editor.md#canvas-keys).
- [Plan for large inputs](memory-and-limits.md): find the memory and document
  limits that apply to your execution route.

## Contribute

- [Find and claim an issue](../CONTRIBUTING.md) or see the
  [product roadmap](../ROADMAP.md) for longer-term goals.
- [Find the responsible crate](architecture.md) and use the
  [development checklist](development.md) to verify a change.
- To update a published format or workspace table, edit its
  [shared reference source](reference/README.md). For compatibility claims,
  use the [conformance inventory](../conformance/README.md).
- Maintainers: [CI runner operations](ovh-ci.md),
  [Linux desktop packaging](linux-packaging.md), and [Pages deployment](pages-deployment.md).
