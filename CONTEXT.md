# Coflow Domain Context

## Project generation

A project generation is one immutable, internally consistent view of a Coflow project. It binds
the project configuration, compiled CFT schema, CFD data model, diagnostics, and source/record/file
indexes to one revision. Hosts consume purpose-specific projections of a generation; they do not
traverse its internal indexes or model identifiers directly.

## CFD input resolution

Input resolution turns project paths into the fixed CFD file set. It recursively discovers `.cfd`
files under configured directories, excludes generated dimension files, assigns source spans, and
emits project diagnostics. There is no provider selection, source option decoding, or format
competition.

## Mutation plan

A mutation plan is the validated execution plan for one batch of record changes. It resolves CFD
write targets, source spans, writer requests, and reference rewrite actions once. Preflight,
transaction enlistment, and staged writes consume the same plan.

## Code artifact release

A code artifact release is the ordered validation, target-language generation, staging,
verification, and publication of source files. `build` and `codegen` share this lifecycle; no data
export artifact is produced.

## Editor generation

An editor generation is the frontend view of one backend project generation. Session identity and
revision ordering determine whether a snapshot or mutation outcome may update caches and history.
Undo and redo history moves only after the corresponding mutation is committed to that generation.

## Graph layout

A graph layout is the filtered, generation-local projection of record reference edges into visible
nodes, forward/back edges, and positions. Field selection, reachability, cycle classification, card
geometry, and ELK graph construction belong to one pure module; browser workers are layout adapters.

## Generator output

A generator output is an in-memory set of target-language source files produced from one immutable
schema/model snapshot and target options. Filesystem destinations, staging directories, manifests,
and publication belong to the code artifact release, not to a generator.
