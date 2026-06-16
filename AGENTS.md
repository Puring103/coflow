# Agent Workflow

Before pushing any branch, run all four checks from the repository root:

```powershell
cargo check --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Do not push while any of these commands fail.
