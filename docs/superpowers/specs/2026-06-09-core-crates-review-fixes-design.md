# Core Crates Review Fixes Design

## Context

This design records the agreed fixes from the strict review of Coflow core crates and specs. The goal is to remove spec contradictions, align implementation boundaries, and add tests for the high-risk paths without expanding runtime responsibilities beyond the intended trust model.

## C# Codegen Trust Boundary

Generated C# is a trusted artifact loader. It is intended to load JSON produced by the official Coflow exporter from data that already passed the Rust data model and checker stages.

The C# loader should:

- deserialize official JSON export files into generated runtime objects;
- build lookup dictionaries and reference object properties needed by the generated API;
- compile cleanly for generated code from valid Coflow schemas and official examples.

The C# loader should not:

- promise a stable diagnostic model for arbitrary hand-written or corrupted JSON;
- duplicate Rust data model validation such as schema-level constraints, duplicate polymorphic IDs, or missing reference diagnostics;
- run CFT `check {}` expressions.

Consequences:

- `@struct` generated C# types must not emit property initializers. CFT defaults are applied by the loader when reading missing JSON fields, not by C# struct construction.
- Existing validation-shaped helper names and docs may be simplified where they only exist to report data errors for untrusted JSON.
- `@deprecated` continues to map to C# `[Obsolete]`. Generated code may produce obsolete warnings if the schema contains deprecated generated types; this is accepted and will not be hidden with pragmas.

## JSON Export Completeness

The official JSON exporter must emit a complete table set for a schema.

- Every concrete top-level table type that the generated C# loader expects must have a corresponding JSON file.
- Empty tables must be exported as empty table JSON, rather than omitted.
- Missing table files remain outside the official exporter contract and do not need special C# loader semantics.

The table enumeration rule should match the codegen table enumeration rule so exporter and generated loaders share the same artifact contract.

## CFT Language Semantics

`is null` is a nullable check:

- `T? is null` is valid for any nullable type.
- Non-nullable operands with `is null` are type errors.
- `is TypeName` remains a dynamic object type predicate. Its left operand must be an object or nullable object.

`is TypeName` uses assignable inheritance semantics:

- The predicate is true when the actual type is the target type or any subtype of the target.
- Exact-type predicates are out of scope for this fix set.

Built-in aggregate functions support nullable element arrays with defined runtime behavior:

- `unique([T?])` treats `null` as a regular comparable value, so multiple nulls are not unique.
- `min([T?])` and `max([T?])` ignore null elements; empty arrays or arrays with no non-null elements are evaluation errors.
- `sum([int?])` and `sum([float?])` ignore null elements; empty arrays or arrays with no non-null elements return zero.
- `contains([T?], null)` is valid and tests for a null element.
- `len` counts all elements, including nulls.

## Reserved Identifiers

CFT needs an explicit reserved identifier set. Reserved names cannot be used as top-level names, const names, field names, or enum variant names.

Current reserved names:

- syntax and literals: `const`, `enum`, `type`, `abstract`, `sealed`, `check`, `when`, `all`, `any`, `none`, `in`, `is`, `true`, `false`, `null`;
- primitive type names: `int`, `float`, `bool`, `string`;
- built-in function names: `len`, `contains`, `unique`, `min`, `max`, `sum`, `keys`, `values`, `matches`.

Future reserved names:

- `if`, `else`, `match`, `case`, `for`, `while`, `let`, `module`, `import`, `export`, `from`, `as`, `use`.

The single identifier `_` is reserved for future wildcard/discard syntax. Identifiers beginning with `_`, such as `_internal`, remain valid when they are not exactly `_`.

## Enum Variant Annotations

Enum variants should support annotations where they add real value:

- `@display("text")` is allowed on enum variants.
- `@deprecated` is allowed on enum variants.
- Other annotations remain invalid on enum variants.

Generated C# should map these annotations to enum member XML summaries and `[Obsolete]` attributes respectively.

## Spec Structure

The existing specs should be split by responsibility:

- CFT language spec: syntax, type rules, evaluation semantics, annotations, diagnostics.
- Schema API spec: public Rust schema reflection model, including modules, spans, declared fields, inherited fields, checks, defaults, and enum variant annotations.
- Excel loader spec: low-level `coflow-excel-loader` crate behavior, taking an already compiled schema and already parsed Excel source definitions.
- Project pipeline spec: project YAML, schema discovery, source discovery, CLI orchestration, export, and codegen flow.

This split keeps crate-level APIs from being described as project-level orchestration and gives codegen consumers a stable schema API reference.

## Excel Loader Cell Text Policy

Excel cell conversion must avoid dangerous silent conversions:

- Excel error cells must report an Excel load error.
- Native Excel date/time cells must report an Excel load error. ISO date/time text cells may still be treated as strings.
- Boolean cells may continue converting to `true` or `false`.
- Numeric and string cells keep their existing intended conversions.

`load_excel_model` must be documented as not executing CFT `check {}` expressions. `load_excel` must be documented as the convenience API that runs checks and returns check diagnostics.

## Testing Strategy

The implementation should add tests that cover the reviewed failure modes:

- generated C# for the RPG example or an equivalent fixture compiles;
- generated C# can load official exporter output;
- JSON export includes empty table files;
- `@struct` defaults do not generate C# struct property initializers;
- `is null` accepts nullable operands and rejects non-nullable operands;
- nullable element arrays have the specified `unique`, `min`, `max`, `sum`, and `contains` behavior;
- reserved identifiers are rejected consistently;
- enum variant `@display` and `@deprecated` compile and affect C# output;
- unsupported Excel error/date cells report loader errors.

## Non-Goals

- Do not make generated C# validate arbitrary untrusted JSON.
- Do not add exact-type predicates.
- Do not add nullable `@index` fields.
- Do not hide C# obsolete warnings with generated pragmas.
- Do not add collection filtering or mapping functions in this fix set.
