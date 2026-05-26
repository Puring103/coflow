# coflow-cfc

coflow-cfc is a Rust reference implementation for the CFC configuration language.

CFC is a typed, self-validating configuration format for game data and other
object graphs. It supports schema definitions, enums, imports, typed objects,
arrays, dictionaries, named data nodes, shared references, and cycles through
named nodes.

The current library crate exposes the parser, loader container, graph builder,
schema reflection, and value model. The workspace also includes the `cfc` CLI
in `src/coflow-cfc-cli`.

```sh
cargo run -p coflow-cfc-cli -- check path/to/root.cfc
cargo run -p coflow-cfc-cli -- get path/to/root.cfc slime.stats.hp
cargo run -p coflow-cfc-cli -- type path/to/root.cfc Monster
```

See `docs/spec/02-cfc.md` for the language, API model, and CLI behavior.
