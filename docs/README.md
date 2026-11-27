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

## Diagnose a run

- [Trace execution](tracing.md) to inspect the interpreter's decisions.
- [Plan for large inputs](memory-and-limits.md): find the memory and document
  limits that apply to your execution route.

## Contribute

- [Find and claim an issue](../CONTRIBUTING.md) or check the
  [roadmap](../ROADMAP.md) for priorities and compatibility evidence.
- [Find the responsible crate](architecture.md) and use the
  [development checklist](development.md) to verify a change.
