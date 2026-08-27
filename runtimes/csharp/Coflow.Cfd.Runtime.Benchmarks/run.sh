#!/usr/bin/env bash
set -euo pipefail

benchmark_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd "$benchmark_dir/../../.." && pwd)"
dotnet_command="${COFLOW_DOTNET:-dotnet}"
if [[ "$dotnet_command" == */* ]]; then
  dotnet_path="$dotnet_command"
else
  dotnet_path="$(command -v "$dotnet_command")"
fi
export PATH="$(dirname "$dotnet_path"):$PATH"

cargo run --manifest-path "$repo_dir/Cargo.toml" -- codegen "$repo_dir/examples/csharp-runtime"
"$dotnet_path" run -c Release --project "$benchmark_dir/Coflow.Cfd.Runtime.Benchmarks.csproj" -- "$@"
