# Finite offline composition contracts

These tests cover a finite set of Ferrule contracts, not exhaustive MFD parity.
All new mappings, schemas, inputs and expected values are independently authored
synthetic material, licensed with Ferrule under GPL-3.0-only. No vendor mapping,
captured payload, reference-run output or private sample is used.

- `grouped_runtime_clock_composition`: one inputless `lang/now` output fans out
  through root, ending-boundary group and sibling bindings. The generic XML
  declaration syntax follows the public `fixtures/grouping/ending.mfd` and
  `runtime_now.rs` examples, without copying a vendor implementation. Literal
  clocks and complete literal trees implement the stable invocation-clock
  contract in `docs/code-generation.md`; empty grouped demand stays lazy.
- `sparse_repeat_absence_composition`: the unchanged public self-authored
  `fixtures/sparse-nested-child/sparse.mfd`, JSON schema and target XSD are
  read-only inputs. A separate literal catalog distinguishes omitted arrays,
  explicit empty arrays and the absence of a child scope. It checks complete
  typed instances, JSON and compact XML, including Unicode, zero quantities and
  ownership after intervening absent/empty parents. It does not redefine legacy
  unframed outward fallback or change the original five-case catalog.
- `parsed_lookup_syntax_composition`: the unchanged public self-authored
  `fixtures/json-array-lookup` mapping format, schemas and literal catalog are
  read-only inputs. Exact single-site syntax edits reverse equality, remove the
  identity of the unused false slot, or combine both. Each variant runs every
  literal result and typed failure; the canonical mapping's actual output is
  never used as the oracle. First-match absence/null, exact key identity, complete
  validation and empty-primary laziness follow `docs/mfd-interop.md`.
- `engine/tests/grouped_runtime_composition`: an independently authored native
  Project checks the invocation table shared by grouped members and ordered
  targets, as specified in `docs/design/invocation-counter-expressions.md`.
  This is **native Ferrule behavior, not MFD auto-number admission**.

Ignored tests build and execute ordinary emitted Rust **Release** and C#
**Release** libraries, with repeated invocations inside each host process.
They use the maintained CLI host-target policy and .NET temporary-directory
helper without changing either. They copy the exact SDK pin, prove the selected
SDK with the same executable/environment/cwd, clear NuGet sources and disable
targeting/runtime pack downloads. Complete command, status, stdout and stderr
originals are written before assertions; failed sandboxes are retained.
Set `FERRULE_CODEGEN_KEEP_ARTIFACTS=1` to retain successful runs as well.
`FERRULE_CODEGEN_HOST_TARGET_DIR`, when supplied, must be an exclusively leased
host cache distinct from the outer target. No ignored test's presence establishes
that it has executed. A closed ordinary capability gate is a refusal, never
permission to substitute a passthrough graph or label direct-runtime execution
as emitted-library evidence.

XML-target expected typed trees are constructed directly from the finite literal
catalog, without inserting schema-reader defaults. This preserves the absence of
the sparse left branch's child scope. Catalog numbers are signed integers;
unsupported scalar types fail this finite test codec rather than coercing an
oracle. JSON-target lookups keep the original catalog's schema-shaped typed
oracle: a bound missing scalar is the `Null` sentinel, while explicit JSON null
is `JsonNull`; serialized missing properties remain absent. All complete literal
JSON documents remain unchanged.
