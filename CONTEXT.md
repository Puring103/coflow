# Coflow Domain Context

## Project generation

A project generation is one immutable, internally consistent view of a Coflow project. It binds
the project configuration, compiled CFT schema, CFD data model, diagnostics, and source/record/file
indexes to one revision. Hosts consume purpose-specific projections of a generation; they do not
traverse its internal indexes or model identifiers directly.

## Source resolution

Source resolution turns a project-facing source configuration into provider-owned typed options
and one or more concrete resolved sources. It owns provider selection, option decoding, provider
identity contracts, directory expansion, target location overrides, and project-config diagnostics.

## Mutation plan

A mutation plan is the validated execution plan for one batch of record changes. It resolves write
targets, sources, writers, provider request facts, and reference rewrite actions once. Preflight,
transaction enlistment, and staged writes consume the same plan.

## Artifact release

An artifact release is the ordered validation, generation, staging, verification, and publication
of data or code artifacts. Build, export, and codegen are command adapters over the same release
lifecycle. Publication activates an immutable artifact generation through the manifest.

## Editor generation

An editor generation is the frontend view of one backend project generation. Session identity and
revision ordering determine whether a snapshot or mutation outcome may update caches and history.
Undo and redo history moves only after the corresponding mutation is committed to that generation.

## Graph layout

A graph layout is the filtered, generation-local projection of record reference edges into visible
nodes, forward/back edges, and positions. Field selection, reachability, cycle classification, card
geometry, and ELK graph construction belong to one pure module; browser workers are layout adapters.

## Provider output

A provider output is an in-memory artifact set generated from schema/model input and provider-owned
decoded options. Filesystem destinations, staging directories, manifests, and publication belong to
the artifact release, not to the provider output contract.
