# Singular target feeds — 2026-10-26

## Contract

Within one target scope, two wires to the same physical input connector are not
two declared target occurrences. Ferrule does not choose a first, last or
non-null winner for either case when the target schema is singular. Executable
import refuses competing wires; best-effort import retains a repair project with
diagnostics. Separate
cloned entries retain their separate bindings, and executable
validation rejects duplicate bindings to a singular target field.

A repeating target is different: its separately declared entries can emit each
value in entry-tree order. Existing declared-ancestor ownership remains required
for nested branches. Equal expression IDs, reversed graph vertices or repeated
XML names do not establish a different owner or authorize singular selection.

## Native observation

The reference was MapForce Enterprise 2026r2 x64, executable 28.2.0.0, running
offline in an isolated Wine prefix and display. Five independently authored
designs used an unqualified `Envelope`, integer `Amount` and string `Label`.
Original files were guarded, and complete outputs and validation messages were
retained. This is not native Windows ABI or general XML qualification.

- **Single feed:** one `Amount=17`, then `Label=safe`. The full tree had no
  namespace declarations, root attributes or additional nodes and passed its
  target XSD. The first native save and its reopened re-save were byte-identical;
  the independently authored original remained unchanged.
- **Competing feeds:** inputs carrying 17 and 29 connected to the same Amount
  input failed validation with `Input has multiple connections: Amount`.
  Reversing the serialized graph vertices gave the same refusal. These invalid
  designs were not executed.
- **Cloned singular entries:** separate Amount input keys emitted both values,
  in order 17 then 29, followed by Label. Native validation and execution
  succeeded, but the complete output failed the singular target XSD at the
  second Amount. Native success alone does not establish schema validity.
- **Cloned repeating entries:** the same values and order passed the explicitly
  repeating target XSD. Its raw output matched the cloned-singular output.

The cloned designs retained byte-identical native save/reopen/resave results.
Actual source and target references were checked, and guarded designs, schemas,
inputs and preexisting output sentinels stayed unchanged during preview.
Full XML comparisons preserve names, attributes, text and child order; an XML
declaration and element-only indentation are not semantic differences.

The initial attempt to save the invalid competing-feed design stopped at
“Mapping is not valid, Save anyway?” A separate inspection selected **No** and
retained the validation message. The later run validated first and did not try
to save or execute invalid designs. That failed attempt is not counted as a
successful save/reopen.

## Ferrule comparison and regression ownership

The comparison first exposed a separate renamed-constant defect: the native
kind-2 constant displayed as `SecondValue` was imported as an unknown function.
After correcting that recognition in [#1080](https://github.com/DeandreT/ferrule/issues/1080),
the unchanged single-feed and repeating-clone designs imported and executed
with complete XML agreement. Direct competing feeds and singular cloned entries
still refused executable import. No feed-selection rule changed.

The existing self-authored regressions cover the implementation boundaries:

- `crates/mfd/tests/input_guard.rs`: competing physical input feeds and preserved
  strict-import refusal.
- `crates/mfd/tests/duplicate_binding_occurrences.rs`: separate occurrences keep
  multiplicity; singular duplicates refuse even for a shared expression.
- `crates/cli/tests/import_mfd_input_guard.rs`: exact singular-binding errors
  before native I/O or Rust/C# generation can publish artifacts.
- `crates/cli/tests/code_generation/nested_clone_owners.rs`: compiled Rust/C#
  coverage of the existing declared-owner and repeating-binding contracts.

## Optional source elements

A separate eight-design control uses optional integer `First` and `Second`
source elements, connected in that order to two distinct cloned `Amount`
entries. The four source inputs contain both values, only First, only Second,
or neither; each retains `Label=safe`. Four targets declare Amount repeating,
and four declare it singular. This probes missing XML elements, not `xsi:nil`,
JSON null, fallible expressions or competing wires to one connector.

The isolated 2026r2 reference saved, reopened and resaved all eight designs
without changing their saved bytes. Complete native output retained only the
present Amount values, in declared order, followed by Label. Missing elements
produced no empty Amount placeholder. All repeating outputs passed their target
XSD. The singular both-present output failed its XSD; the other three singular
outputs happened to satisfy it. Input-specific validity does not justify a
schema-wide singular winner rule.

Ferrule's repeating target retains the same complete outputs through native
execution and generated Rust/C# XML text and byte APIs. Its strict importer
continues to reject both singular bindings for every input, including when one
or both source values are absent. Public regression fixtures are in
`crates/mfd/tests/fixtures/optional_cloned_feeds`; the native and compiled
typed/JSON controls are in `crates/mfd/tests/optional_cloned_feeds.rs` and
`crates/cli/tests/code_generation/optional_cloned_feeds.rs`.

## Explicit XML nil

Ten separate controls make both source integers and the target Amount nillable.
They distinguish two ordinary values, nil in either position, two nil values,
and nil followed by an absent source field, for repeating and singular targets.
Each direct wire still owns a distinct cloned entry; no `set-xsi-nil` function
or same-connector fan-in is used.

The isolated reference retained each explicit `xsi:nil="true"` as an Amount
occurrence, in order, while the absent field produced no occurrence. All ten
designs saved/reopened/resaved byte-identically. The five repeating outputs
passed their XSDs. Four singular outputs contained two Amount occurrences and
failed their XSDs, including the two-nil case; the nil-and-absent output passed.
Nil does not remove an occurrence from the target cardinality.

Native Ferrule and generated Rust/C# XML text/byte outputs retain the same
complete values. Strict import still refuses all five singular designs. The
public `xml_nil_cloned_feeds` tests retain exact typed XML-nil values and native
export/re-import behavior. Their fixture JSON is only notation for constructing
typed values, not evidence about JSON null or JSON mapping APIs. These controls
do not establish other nil lexical forms, nested nil, coercion or error ordering.

## Remaining design gate

[#241](https://github.com/DeandreT/ferrule/issues/241) remains open. These finite
non-null constants, optional-element and explicit-nil controls do not establish general null
handling, fallible-expression precedence or a new singular multiple-feed representation. No such shape is
admitted by this observation. Distinct declared branch ownership belongs to
[#239](https://github.com/DeandreT/ferrule/issues/239), not an inferred winner.
