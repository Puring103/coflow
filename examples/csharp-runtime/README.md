# C# Runtime Integration Example

This project exercises generated CFT types and CFD execution through the C# runtime. It covers
strongly typed nested data, enum record keys, record references, host-to-CFD calls, CFD-to-host
calls, and higher-order functions crossing the VM boundary repeatedly. The CFD inputs are separate
`CoflowModule` instances; replacing the character child module also verifies that the root relinks
data and recompiles parent functions atomically.

Run it from the repository root:

```bash
COFLOW_DOTNET=/path/to/dotnet examples/csharp-runtime/test.sh
```

The script regenerates the C# types from CFT, compiles the CFD sources at runtime, runs all
assertions, and prints `csharp-runtime-integration-ok` on success.
