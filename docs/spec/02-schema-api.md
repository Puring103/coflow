# Schema API

## Scope

This document describes the public Rust schema reflection model produced by `coflow-cft`.

## Stable Concepts

- `CftContainer` owns compiled modules and exposes all consts, enums, and types.
- `CftSchemaModule` groups definitions by module.
- `CftSchemaType.fields` are declared fields.
- `CftSchemaType.all_fields` are inherited fields in effective order.
- `CftSchemaField.ty_ref` is the resolved public type reference.
- `span` and `module` identify source locations for diagnostics.
- `check` holds the compiled check expression block.
- `default` holds compiled default values.
- `CftSchemaEnumVariant.annotations` carries variant-level `@display` and `@deprecated`.

## Consumers

Codegen crates should use this schema API rather than re-reading AST.
