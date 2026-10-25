# Supplemental source inventory

These files review documentation bodies beyond the original topic inventory in
[`mfd-2026r2.source-contracts.json`](../mfd-2026r2.source-contracts.json).
They do not change the canonical capability ledger or establish execution support.

| File | Source coverage |
| --- | --- |
| `nonleaf.json` | 191 parent-topic bodies, including additional operations and existing-context refinements |
| `functions.json` | 319 named function leaves, parameter tables, language declarations and unresolved contract facets |
| `constructors.json` | 43 `xs:` constructor leaves and their shared lexical/type statements |
| `com-api.json` | Separate public COM API catalog; release binding remains unestablished |

The manual reviews use the same pinned PDF and layout-text hashes as the original
source companion. Line numbers refer to that exact extraction; physical PDF page
numbers remain distinct from printed headers. The public PDF can be retrieved from
<https://www.altova.com/documents/MapForceEnterprise.pdf>; verify its recorded hash
before reusing those anchors. `pdftotext -layout` produces the referenced extraction.

Missing declarations, defaults, types and conflicting source claims remain
explicit. An editorial signature summary is not a literal API declaration.
Language badges describe vendor documentation, not Ferrule support or independently
verified backend applicability. These overlapping sets are not an additive parity
denominator.

The COM catalog has its own per-page hashes and explicitly provisional identities.
Its documented installation and visible-window requirements do not prove a working
vendor environment. Its scoped catalog-completeness flag does not establish
Enterprise 2026r2 inventory completeness. The canonical `inventory_complete` flag
remains false while release provenance, supporting classes and remaining source
contracts are unresolved.

Run `cargo test -p mfd-conformance --test source_inventory` to validate finite
coverage, unique source identities and the separation from execution assessments.
