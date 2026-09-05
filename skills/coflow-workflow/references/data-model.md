# CFD data model

The runtime lowers CFD syntax nodes into schema-guided records and source-neutral values. Record identity is `(source_id, record_index)` and references use `(declared_type, key)`. The model is immutable after a successful refresh and exposes record, value, source, and diagnostic indexes to checks and code generators.
