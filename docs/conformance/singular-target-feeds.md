# Singular target feeds — 2026-10-26

## Contract

Two wires to one physical pin and two cloned entries are different shapes.
Executable import refuses competing wires; repair import preserves diagnostics.
Cloned entries retain separate bindings, even for a shared expression.
A singular target refuses duplicates; a repeating target emits the present
occurrences in entry-tree order. Nested branches still require a declared
ancestor owner. No first-, last- or non-null-winner policy is implied.

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

The clone designs saved and reopened byte-identically. Comparisons checked
complete ordered XML and guarded inputs and output sentinels. Invalid
competing-feed designs stopped at validation; they supply no execution or
successful save/reopen evidence.

## Ferrule comparison and regression ownership

After the [renamed-constant fix](https://github.com/DeandreT/ferrule/issues/1080),
the single-feed and repeating-clone designs agreed on complete XML. The
same-pin and singular-clone designs still refused executable import:

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

## Disposition by target shape

| Target, in one invocation | Executable decision | Cardinality and failure boundary |
| --- | --- | --- |
| One wire to a singular entry | Existing single-binding path | The constant control emits exactly one Amount. An explicit conditional feeding one pin is still one binding. |
| Two distinct sources on one physical connector | Refuse competing wires | Reference validation fails in either tested vertex order. No preview value or competing-expression error is defined by those runs. |
| Two declared entries for the same singular field in one scope | Preserve both bindings; refuse executable admission | For two present values the reference emits two occurrences and violates the XSD. Equal values, a shared expression, or a particular input with at most one present value do not make this a singular mapping. |
| Same singular topology with missing or nil source elements | Same refusal for every input | For the tested direct XML feeds, missing contributes zero and explicit nil contributes one occurrence. Two nils violate the singular XSD. |
| Same singular topology with two fallible expressions | Same refusal before execution or lowering | Ferrule reports duplicate binding. In the reference control with two invalid string-to-double conversions, reversing the cloned entries changed the first failing token from `ochre` to `indigo`. |
| Two declared repeating entries | Existing repeated binding path | Emit each present occurrence in entry-tree order, including explicit XML nil. [Enabled target casting](xml-target-casting.md#enabled-casting-and-failure-selection) selects the first declared failure for the tested conversion/division pair. |
| Separate declared ancestor branches | Existing [#239](https://github.com/DeandreT/ferrule/issues/239) ownership rules | Resolve the declared owner first, then check cardinality within that scope. Equal field names across owners are not evidence of competition. |
| Untyped null, JSON null, other expression failures or ambiguous owners | No additional admission | XML nil controls give no JSON or general error-order rule. An ambiguous owner cannot be repaired by choosing a value. |

**No same-scope singular multiple-feed shape qualifies.** The
[distinct-constant fixture](../../crates/mfd/tests/fixtures/input_guard/competing-singular.mfd)
refuses before acquisition or generated publication. Compiled full-value
comparisons cover repeating targets only. The [two-conversion probes](../design/fixtures/native-feed-controls/README.md)
reproduced the first failing token before and after reopen and left partial
XML in all four failures. They cannot turn two present values into a
schema-valid singular result.
