# CFD Editor Local Plugin Examples

In CFD Editor, open the Extensions view and choose Install from file, then select
`chemical-equation/plugin.json`.

External plugins are single-file ESM modules. They export
`default function activate(host)` and declare each renderer target through
`host.renderers.register(...)`. The chemical-equation example takes over
the `ChemicalExpression` type in table cells and expandable value headers. Its
fields remain rendered and editable by the editor.
