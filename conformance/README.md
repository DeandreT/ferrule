# MFD conformance inventory

`mfd-2026r2.json` pins the current conformance target to an **Enterprise
2026r2 `.mfd`-compatible application**. It is a seeded, non-exhaustive
capability inventory, not a claim of full compatibility or a product-parity
percentage. The release pin is
intentional even when the vendor's release-history page changes.

Run these commands from the workspace root:

```sh
cargo run -p mfd-conformance
cargo test -p mfd-conformance
cargo run -p mfd-conformance -- summary --format json
cargo run -p mfd-conformance -- gate --profile mfd-2026r2-enterprise
```

The binary accepts another ledger path and `--root PATH` for its repository
evidence root. Relative ledger and evidence paths resolve from that root. The
workspace's ordinary tests validate the checked-in inventory and its local
references; no vendor application, network access, or private sample corpus is
needed. The default validation command exits nonzero for malformed inventory or
evidence.

`summary` reports exact status counts for each selected profile and dimension;
`--format json` emits the same deterministic counts for automation. `gate`
reports those counts plus a failure for every selected in-scope cell that is
unassessed, unverified, unsupported, partial, blocked, or missing required
evidence. It exits 0 only on a pass and 1 on a conformance failure. A
`not_applicable` cell is excluded only when it cites evidence; native MFD
export and vendor-backend exclusions need vendor documentation or execution
evidence. A selection containing no applicable cells fails rather than passing
vacuously. No skip or ignore flag exists.

Use repeated `--profile ID`, `--capability ID`, and `--dimension NAME` options
to create a focused development gate. Each option is inclusive; omitting an
axis selects all values on that axis. Unknown, duplicate, or empty
profile/capability intersections fail. Dimension names are `import`,
`interpreter`, `native_export`, `ferrule_roundtrip`, `gui`, `debug`, `rust`,
`csharp`, and `vendor_backends.xslt1`, `.xslt2`, `.xquery1`, `.cpp`, `.java`,
`.csharp`. For example:

```sh
cargo run -p mfd-conformance -- gate \
  --profile mfd-2026r2-enterprise \
  --capability interop.mfd-designs \
  --dimension native_export --format json
```

A gate covering every capability and dimension in a selected profile also
requires `inventory_complete=true`, whether those values were selected
implicitly or listed explicitly. The checked-in seeded inventory is incomplete,
so its full-profile gate intentionally fails. A focused gate can qualify a
development slice without implying that the product or profile is complete.

Each capability independently assesses MFD import, interpreter execution,
native MFD export acceptance, Ferrule self-roundtrip, GUI authoring,
debugging, Ferrule Rust/C# generation, and vendor XSLT 1.0, XSLT 2.0, XQuery 1.0,
C++, Java and C# backends. Vendor-backend applicability must be checked per
capability: the presence of a language in this schema does not imply that every
format or operation is available in that language.

| Status | Meaning |
|---|---|
| `unassessed` | This surface has not yet been inventoried. |
| `unverified` | A relevant implementation or claim exists, but its required behavioral proof is absent. |
| `unsupported` | The specified capability is not implemented on this surface. |
| `partial` | The detail identifies an implemented subset with linked evidence. |
| `supported` | The entire capability as scoped in this row has linked evidence. |
| `blocked` | A required external dependency prevents assessment or execution; explain it in the detail. |
| `not_applicable` | The surface is outside this capability's scope; explain why. |

`supported` and `partial` require evidence. On `native_export` and every
`vendor_backends` surface they specifically require `vendor_execution` evidence;
an emitter, a vendor manual or a Ferrule re-import test cannot establish vendor
execution. The validator checks evidence structure and file existence, not the
truth of a supplied execution record. Reviewers must verify that such records
actually exercise the claimed capability and pinned vendor version.

The `native_mfd` profile is distinct from `ferrule_native_extensions`.
Ferrule components, captured-service provenance and retained layout metadata can
successfully self-roundtrip without being understood by the vendor application.
Add native profile coverage only after native lowering and vendor acceptance
are proved.
Neither profile should silently inherit status from the other.

Version 1 uses required, strictly typed fields; unknown fields and unknown
enum values reject. Every ID must be unique within its namespace and use
lowercase ASCII letters, digits, dots, underscores or hyphens. All profile,
capability-dependency and evidence references must resolve. Duplicate references,
dependency cycles, empty inventories, blank explanations, unsafe or missing
local evidence paths and missing status dimensions reject. Both profiles must
have at least one capability. HTTPS documentation locations are checked
structurally but are not fetched by the validator.

To extend the inventory, split broad seeded rows into individually testable
functions, configuration commands, formats, connector operations and workflows.
Preserve existing IDs when their scope remains the same. Add repository-relative
evidence or official vendor-documentation references, assess every surface
explicitly, and run the validator and focused tests. Vendor execution records
should identify the release, backend, inputs, outcomes and comparison contract;
keep proprietary samples and outputs outside the public repository. Public
evidence can be self-authored fixtures and non-proprietary run summaries.

Keep `inventory_complete` false until the chosen vendor edition/release has been
fully itemized and reviewed. The current entries are a starting inventory drawn
from the repository's recorded support boundaries and official product/manual
references. Historical corpus counts in linked documents are not fresh run
results, and the local informational surveys are not automatically promoted to
release gates by this ledger.
