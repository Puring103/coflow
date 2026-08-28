# C# Runtime Integration Example

This example shows how to generate C# types, load CFD files, configure host functions, call CFD
functions, query records, and replace a module in a `CoflowModuleSet`.

Run it from the repository root:

```bash
COFLOW_DOTNET=/path/to/dotnet examples/csharp-runtime/test.sh
```

The script regenerates the C# types, runs the example checks, and prints
`csharp-runtime-integration-ok` on success.
