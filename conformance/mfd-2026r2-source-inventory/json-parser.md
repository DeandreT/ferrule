# Parse an array of JSON records from a string parameter

The checked-in [parser design and cases](../../crates/mfd/tests/fixtures/json-array-parser/)
bind the required `InputText` string to a schema-bounded record array. The
ordinary primary input is `{}`. Strict executable import of the fixture
produced zero warnings on Release CLI revision
`740af0c38ede7d814a17146bab2bffd7301320c9`. The CLI, identified for this run
by SHA-256 `9147b511b7f0a35c347768875ccad356c1e28e895d2e07dda7301d4b60ab9c07`,
ran 24 inputs:

- **Nine complete typed JSON results:** ordered mapped members,
  empty and absent arrays, separate missing and explicit-null values,
  repeated and empty records, and the observed duplicate-key behavior.
- **Fifteen refusals before any output:** malformed JSON, invalid array
  and scalar shapes, schema limits and an omitted `InputText` binding.
  The omitted binding reported the missing parameter, not a parse error.

From this directory, use the standard-library oracle with a built CLI:

```text
python -B local-oracles/check.py PATH_TO_FERRULE --case json-parser
```

The oracle passes each parameter payload as one argument and compares the
whole parsed output with the checked-in fixture, preserving array order and
scalar types. This is **partial interpreter evidence** on a narrow saved
design and CLI surface, not a build of this inventory revision. It does
not establish generated-host, native-export, vendor, or roundtrip support.
