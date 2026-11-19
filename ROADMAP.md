# Product direction

Ferrule's goal is to let a mapper design a transformation visually, run it
from the CLI and embed it in an application. The
[format guide](docs/formats.md) describes what works today. Use
[open issues](https://github.com/DeandreT/ferrule/issues) for current work.

| Area | Direction |
| --- | --- |
| Visual authoring | Make advanced layouts and schemas usable in the editor. Let a mapper inspect the source row, field, and connection behind a preview value. |
| Interchange | Preserve field ownership, values, order and failures when importing or exporting a design. Keep a repairable draft distinct from an executable mapping. See [MFD interoperability](docs/mfd-interop.md). |
| Connectors and pipelines | Extend [formats](docs/formats.md) and [stage graphs](docs/mapping-pipelines.md) with explicit rules for I/O, cancellation and publication. |
| Generated libraries | Extend the [Rust and C# profiles](docs/code-generation.md) while preserving caller interfaces, errors and output across backends. |
| Packaging | Make mappings and [their resources](docs/offline-bundles.md) reproducible without depending on the authoring environment. |

Compatibility is specific to an operation, format, backend and workflow.
Import, export and re-import, execution in another application, and execution
of a generated library answer different questions. The
[conformance inventory](conformance/README.md) records those boundaries and
remaining gaps; it does not establish a whole-product parity percentage.

The inventory's open boundaries include complete XML Schema derivation,
correlated JSON constraints, arbitrary workbook layouts, additional SQL
drivers and mutations, and taxonomy or formula execution for XBRL. A bounded
reader or writer does not cover interactive authoring, external execution or
debugging. See the [format profiles](docs/formats.md), [editor controls](docs/canvas-presentation.md)
and [generated-library contracts](docs/code-generation.md) for the features
available on each route.
