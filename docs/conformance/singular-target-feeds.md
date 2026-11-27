# Repeated target bindings in an MFD

An input pin identifies a connection. A target entry identifies a binding:
two cloned entries remain two bindings even if the same expression feeds
both. Resolve a nested entry to its declared ancestor before checking how
many bindings that field can have in one scope.

## Disposition by target shape

| Saved shape | Ferrule behavior |
| --- | --- |
| One wire to a singular entry | Import the binding. A conditional expression wired to that pin is still one binding. |
| Competing wires to one pin | Executable import refuses. Repair import keeps the diagnostic. |
| Separate cloned entries for one singular field | Preserve both bindings and refuse executable admission. Equal values or an input with only one present value do not make a singular mapping. |
| Separate entries for a repeating field | Emit present values in entry-tree order. An absent XML element contributes no occurrence; an explicit XML nil contributes one. |

A successful preview elsewhere can emit two elements that violate a singular
target's XSD. Choosing the first, last or non-null value would change the
mapping. For the supported conversion cases, see [XML target casting](../design/target-float-cast.md).

[Optional and absent inputs](../../crates/mfd/tests/fixtures/optional_cloned_feeds/README.md),
[competing wires](../../crates/mfd/tests/fixtures/input_guard/competing-singular.mfd)
and the [conversion controls](../design/fixtures/native-feed-controls/README.md)
exercise these distinctions.
