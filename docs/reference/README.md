# Shared Technical References

This directory holds the canonical data for technical claims that appear in
both the Markdown guides and the published site. The `reference-docs` crate
renders each claim family into marked regions of those documents:

```sh
cargo run -p reference-docs            # rewrite stale regions
cargo run -p reference-docs -- --check # report stale regions
cargo test -p reference-docs           # freshness and validation test
```

A generated region starts with `<!-- BEGIN GENERATED name ...` and ends with
`<!-- END GENERATED name -->`. Edit the TOML source rather than the region; text
outside the markers remains hand-written and is never changed by the generator.

## Claim Inventory

| Claim family | Surfaces | Canonical source | Ownership |
| --- | --- | --- | --- |
| Format direction, selection, implemented scope, qualified routes, and unsupported profiles | [Supported formats](../formats.md) table; site `docs/formats.html` | [`formats.toml`](formats.toml) | Generated |
| Workspace components and their roles | [Architecture](../architecture.md) workspace layout; site `docs/architecture.html` | [`workspace.toml`](workspace.toml), checked against the root `Cargo.toml` members | Generated |
| Detailed format contracts, including the XSD subset, JSON Schema keyword profile, JSON5 limits, and CSV setup | [Supported formats](../formats.md) narrative | That guide | Curated Markdown; the site links to it |
| Code generation routes and the portable subset | [Code generation](../code-generation.md); site `docs/codegen.html` | That guide and the route contracts it links | Curated; the site page is a summary |
| `.mfd` import and export coverage | [`.mfd` interoperability](../mfd-interop.md); site `docs/mfd.html` | [`conformance/mfd-2026r2.json`](../../conformance/mfd-2026r2.json), validated by `mfd-conformance` | Curated; the site page is a summary |
| Product overview and highlights | `README.md`; site `index.html` and `docs/index.html` | None | Curated marketing text, not generated |

## Claim Categories

Every format entry separates three kinds of claim:

- `summary` is implemented capability: behavior of the native adapters and the
  CLI that uses them. Each format cites at least one existing source or test
  path in `evidence`.
- `qualified` lists separately named routes, such as generated library
  adapters or editor workflows. Each route cites a repository `record` that
  owns its contract or qualification evidence and covers only that route.
- `unsupported` lists named shapes, directions, or behaviors outside the
  implemented scope. The list is not exhaustive.

Keep entries to finite, tested coverage. A route or corpus result does not make
a claim about a complete format standard or another product.

## Editing Rules

- Text fields are single paragraphs of plain words and `code` spans. Markdown
  markup, HTML, and `|` are rejected so the same value renders as a Markdown
  table cell and escaped HTML. Whitespace collapses to single spaces.
- Paths are normalized, repository-relative, and must exist.
- Every root `Cargo.toml` workspace member must appear exactly once in
  `workspace.toml`. Other listed directories must not contain a `Cargo.toml`.
- To publish a new claim family, add its source here, a renderer and `Surface`
  entry in `crates/reference-docs`, and markers in each target document.
