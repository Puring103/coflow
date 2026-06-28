# Agent Workflow

Before pushing any branch, run all four checks from the repository root:

```powershell
cargo check --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Do not push while any of these commands fail.

When the user specifies a version to package or release, reinstall the local Cargo CLI after the checks pass:

```powershell
cargo install --path . --force
```

If files under `skills/` changed in that version, refresh installed skills as well. For this local
skill package, re-run `add` without `--all`; `--all` expands to every agent and can include
project-only agents during global installs.

```powershell
npx skills add . -g --skill "*" --copy -y
```

For skills installed from a remote package with version tracking, use the updater instead:

```powershell
npx skills update -g -y
```
