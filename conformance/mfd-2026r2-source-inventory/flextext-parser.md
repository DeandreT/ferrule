# Parse delimited records from repeated XML string values

The checked-in [string parser design](../../crates/mfd/tests/fixtures/flextext-string-parser.mfd)
resolves an external FlexText layout, projects a string and an integer, and
creates an XML `Row` for each admitted input item. Strict executable import
reported zero warnings on Release CLI revision
`740af0c38ede7d814a17146bab2bffd7301320c9` (executable SHA-256
`9147b511b7f0a35c347768875ccad356c1e28e895d2e07dda7301d4b60ab9c07`).

- Four complete output trees matched: original and reversed source order,
  a quoted embedded delimiter with Unicode and a signed integer, and one
  first-record projection from an input value containing a line break.
- Invalid integer text and an absent field separator produced the layout
  diagnostic before writing any file. The same refusal left no output when
  a valid item came before an invalid one.

From this directory, run the standard-library oracle with a built CLI:

```text
python -B local-oracles/check.py PATH_TO_FERRULE --case flextext-parser
```

This is partial import and interpreter evidence for a single delimited
string-parser profile, not a build of this inventory revision. It does not
qualify arbitrary layouts, generated consumers, native export or roundtrip.
