# C# Code Generation Example

This package passes `input.json` to
`GeneratedMapping.ExecuteJson`, compares the returned document with
`expected-output.json`, and checks the complete filtered and sorted result. The
generated library vendors its runtime and has no NuGet dependencies.

From the repository root, generate the library into a fresh destination:

```sh
cargo +nightly run -p cli -- generate \
  --project examples/codegen/project.json \
  --language csharp \
  --out examples/codegen/generated/csharp
```

For repeatable regeneration, run `./examples/codegen/run.sh` from the repository
root; it safely clears only the ignored generated-artifact directory before
rebuilding both language examples.

Then build and run the host application:

```sh
dotnet run \
  --project examples/codegen/csharp/Ferrule.CodegenExample.csproj \
  --configuration Release
```

The host calls `Ferrule.Generated.GeneratedMapping.ExecuteJson`, prints the three
generated invoice summaries, and exits unsuccessfully if the output differs
from the mapping contract.
