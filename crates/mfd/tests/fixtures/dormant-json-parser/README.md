# Dormant JSON parser

This self-authored design copies the required `Message` string from the source
document to the report. A separate required global String parameter feeds a
string parser that declares no output ports. No mapping value demands that
parser. The complete expected reports are literal copies of `Message`; malformed,
missing and unexpected source fields remain input failures.

The native and compiled Rust/C# checks supply an omitted parameter, malformed
JSON text and a valid payload. None changes the report. Separate controls demand
a parser output and require the actual parsing and missing-parameter failures.
Schema availability and unambiguous input declarations are checked independently
of execution demand. The design-time preview is retained, not used as a default.
