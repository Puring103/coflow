# C# Runtime Integration Fixture

This CI fixture verifies generated C# types, CFD loading, host functions, function calls,
record queries, and module replacement. It is not a user-facing example.

Run it from the repository root:

```bash
COFLOW_DOTNET=/path/to/dotnet tests/csharp-runtime-integration/test.sh
```

The script regenerates the C# types, runs the example checks, and prints
`csharp-runtime-integration-ok` on success.
