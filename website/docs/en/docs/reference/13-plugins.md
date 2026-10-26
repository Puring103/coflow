# Editor plugins

Plugins add specialized views, field presentations, standalone pages, sidebars and commands to the CFD editor. Store plugins with a project so the team shares the same presentation tools.

## Installation and project configuration

Choose the project-plugin action in the editor's plugin manager and select `plugin.json`. The plugin list supports enabling, disabling and removing project plugins, as well as uninstalling global plugins. Plugins execute in the editor; use trusted sources.

Project plugin configuration lives in `editor-setting/plugins.json`:

```json
{
  "version": 1,
  "plugins": [
    { "id": "item-tools", "manifest": "plugins/item-tools/plugin.json", "enabled": true }
  ],
  "defaults": { "views": {}, "presentations": {} }
}
```

`manifest` resolves relative to the project root. Commit configuration, manifests and entry files together when sharing. `defaults.views` selects views by type; `defaults.presentations` selects presentations by type and `cell`, `inspector` or `summary` slot. Values use `plugin ID/contribution ID`, for example `"views": {"Item": "item-tools/item-list"}`. Contributions may also set `default`.

## Manifest and entry point

`plugins/item-tools/plugin.json`:

```json
{
  "id": "item-tools",
  "name": "Item tools",
  "description": "Specialized views for item configuration",
  "version": "1.0.0",
  "entry": "plugin.js"
}
```

IDs must be nonempty ASCII letters, digits, underscores or hyphens. `name` and `entry` must be nonempty. The entry is a relative `.js` path inside the plugin directory; `..` must not escape it. Bundle dependencies into one ES module file; the entry does not use relative module imports.

The entry must default-export `activate(host)`, optionally async, returning nothing or an object with `dispose()`:

```js
export default function activate(host) {
  const unregister = host.register.view({
    id: 'item-list',
    title: 'Item list',
    types: ['Item'],
    readOnly: true,
    mount(context, outlet) {
      const element = document.createElement('pre')
      const update = next => {
        element.textContent = next.records.map(record => record.coordinate.id).join('\n')
      }
      update(context)
      outlet.replace(element)
      return { update }
    }
  })
  return { dispose() { unregister() } }
}
```

The project must define Item. Select the Item list view when viewing that type's data.

## Extension points

All registration methods return unregister functions. Contribution IDs must be unique within the plugin. Views and presentations must list explicit type names in `types`.

| Method | Configuration and behavior |
| --- | --- |
| `host.register.view` | `id`, `title`, `types`, `mount`; optional `includeFieldValues`, `readOnly`, `default` |
| `host.register.presentation` | `id`, `types`, `slot`, optional `default`; `cell` and `inspector` use `mount`, `summary` uses `render(context): string` |
| `host.register.page` | `id`, `title`, `mount`; open with `host.register.openPage(id)` |
| `host.register.sidebar` | `id`, `title`, `mount`, optional `extensions` or `search` icon; open with `host.register.openSidebar(id)` |
| `host.register.command` | `id`, `title`, `run(context)`; may be async |
| `host.register.keybinding` | `command`, `key`, optional `when(context)`; binds a plugin command |

View context contains `identity`, `filePath`, `typeName`, `records`, `readOnly` and `openPage`. Each record contains `filePath` and `coordinate`; enable `includeFieldValues` when values are needed. Presentation context contains `identity`, `filePath`, `coordinate`, `actualType`, `fieldPath`, `declaredType` and optional `value`. Root records have `fieldPath: null`.

Page context provides `pageId`, `identity`, `openPage` and `closePage`. Sidebar context provides `sidebarId`, `identity`, `openPage`, `openRecord` and `closeSidebar`. Commands and keybinding conditions can read current `identity`, `filePath`, `typeName`, `selection` and `surface`; commands can also call `openPage`. Without a project, `identity` may be null.

## Mounting and cleanup

`mount(context, outlet)` displays content through `outlet.element` or `outlet.replace(Node|string)`. Return nothing, a cleanup function, or an object with `update(context)` and `dispose()`. Update displays when context changes; release timers, events and resources on unmount. Async work should check `outlet.signal.aborted` before updating the UI. Use `textContent` for user text.

`host.events.on(event, listener)` returns an unsubscribe function. Events include `project` (possibly null identity), `data` (new data identity), `selection` and `surface` (current document, view or sidebar). The host removes registrations on deactivation; release plugin-created resources in the activation result's `dispose`.

## Querying and editing data

`host.data.current()` returns the current `{ sessionId, revision }` or null. Async queries below return `{ sessionId, revision, data }`; retain identity to check that results still belong to the current project and revision.

| Method | Purpose |
| --- | --- |
| `getSchema()` | Read type definitions |
| `getRecordsByType(typeName, { includeFieldValues })` | Query records by type |
| `getRecord(filePath, coordinate)` | Query one record; missing data is null |
| `getField(filePath, coordinate, fieldPath)` | Query a field; missing data is null |
| `searchRecords(query, { mode, limit })` | `mode` is `id` or `full_text`; data includes `hits` and `truncated` |

Record coordinates use `{ actual_type: 'Item', id: 'sword' }`. Field paths use `{ kind: 'field', value: 'name' }`, `{ kind: 'index', value: 0 }` or `{ kind: 'dict_key', value: '"key"' }`; dictionary path values are CFD key text. Plugin paths use editor objects; [CLI changesets](./08-cli.md#batch-changes-and-request-structures) use their own structure.

For example, change an item name in response to a user action:

```js
await host.data.mutate({
  kind: 'write_field',
  filePath: context.filePath,
  coordinate: { actual_type: 'Item', id: 'sword' },
  fieldPath: [{ kind: 'field', value: 'name' }],
  value: { kind: 'string', value: 'Long sword' }
})
```

Respect `context.readOnly` and confirm targets belong to the current project before writing. `mutate` returns the published identity and rejects its Promise on failure. Using this interface preserves editor undo, redo and update flows. Query snapshots are for reading.

Supported mutations:

| `kind` | Parameters besides `kind` |
| --- | --- |
| `write_field` | `filePath, coordinate, fieldPath, value` |
| `write_fields` | `filePath, coordinates, fieldPath, value` |
| `write_field_batch` | `filePath, writes`; each entry has `coordinate, field_path, new_value` |
| `edit_collection` | `filePath, coordinate, fieldPath, edit` |
| `rename_record` | `filePath, coordinate, newId` |
| `insert_record` | `filePath, recordId, actualType, fields` |
| `delete_record` | `filePath, coordinate` |
| `swap_records` | `filePath, first, second`; the latter two are record coordinates |
| `move_record` | `filePath, coordinate, targetIndex` |
| `transfer_record` | `sourceFile, destinationFile, coordinate, targetIndex` |

`edit_collection.edit` supports `array_append` (optional `value`), `array_remove` (`index`), `array_move` (`from, to`), `dict_insert` (`key`, optional `value`) and `dict_remove` (`key`). Dictionary keys use `{ kind: 'string', value: 'key' }`, integer keys or enum keys, retaining the structure returned by queries.

Field values are tagged objects with kinds `option_none`, `option_some`, `bool`, `int`, `float`, `string`, `formatted_string`, `enum`, `object`, `ref`, `array` or `dict`. `int.value` is a JavaScript bigint. Preserve queried composite structures, edit the needed parts and submit through mutation APIs.

See the [chemical formula plugin entry](/examples/chemistry/plugin.js) for custom cell and inspector presentations.
