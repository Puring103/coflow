# coflow-cfc

coflow-cfc is a Rust reference implementation for the CFC configuration language.

CFC is a typed, self-validating configuration format for game data and other
object graphs. It supports schema definitions, enums, imports, typed objects,
arrays, dictionaries, named data nodes, shared references, and cycles through
named nodes.

The current crate exposes the parser, loader container, graph builder, and value
model as a library. See `docs/spec/02-cfc.md` for the language and API model.
