#!/usr/bin/env bash
set -euo pipefail

example_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd "$example_dir/../.." && pwd)"
dotnet_command="${COFLOW_DOTNET:-dotnet}"

cargo run --manifest-path "$repo_dir/Cargo.toml" -- codegen "$example_dir"
"$dotnet_command" run --project "$example_dir/app/Coflow.Runtime.Example.csproj"
