# Generated Mapping Hosts

This example uses one ferrule project to generate equivalent Rust and C#
mapping libraries. The mapping filters zero-value orders, sorts the remaining
orders, assigns output positions, and formats invoice display text.

From the repository root, generate both libraries and run both hosts:

```sh
./examples/codegen/run.sh
```

The script writes generated artifacts under `examples/codegen/generated/`,
which is intentionally ignored. Each host constructs the same typed source
instance, calls its generated library, validates the result, and prints the
three invoice rows.

- [`project.json`](project.json) is the portable mapping project.
- [`input.json`](input.json) and [`expected-output.json`](expected-output.json)
  show the equivalent JSON boundary values.
- [`rust/`](rust/) demonstrates the generated Rust API.
- [`csharp/`](csharp/) demonstrates the generated C# API.

The generated libraries return ferrule instance trees. Format adapters and file
I/O remain the host application's responsibility.
