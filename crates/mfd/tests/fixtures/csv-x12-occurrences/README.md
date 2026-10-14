# CSV row occurrences in an X12 target

The complete authored X12 configuration retains repeating Interchange, Group,
selected Message, Rows and NTE metadata. Noniterating constructed ancestors
produce one item; only the saved CSV block connection drives Rows at `[]`.
Each row constructs two NTE occurrences in saved declaration order. Independent
nonempty literals establish occurrence presence even when a data cell is Null.

CSV String declarations do not promise presence. By default empty and missing
cells are Null. The preserve-empty design retains a physically present empty
String while a missing trailing cell remains Null.

The envelope count is specific to the single-row physical control. Typed zero
and many-row mapping controls do not reuse it as a wire-count oracle. Ordinary
generated typed Rust and CSharp mapping, native CSV parsing, native X12 writing,
and the strict CSharp CSV/X12 descriptor are separate contracts. The default
classic descriptor continues to refuse this repeated grouped hierarchy. An
explicit GroupedTransactions policy requires its own complete descriptor and
count proof; the configuration is never weakened to obtain that eligibility.
