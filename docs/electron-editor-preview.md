# Electron editor preview

The Electron shell reuses the production React frontend and runs editor operations in the
`cfd-editor-sidecar` Rust process. It is an alternative development host; Tauri remains the release
host.

From the repository root:

```bash
npm --prefix editors/cfd-editor install
npm --prefix editors/cfd-editor run electron
```

Useful checks:

```bash
npm --prefix editors/cfd-editor run electron:test
npm --prefix editors/cfd-editor run electron:smoke
```

`electron:test` exercises the stdio protocol, project loading, record reads, and file-watch events.
`electron:smoke` starts a headless BrowserWindow and verifies that the sandboxed preload bridge,
React application, and Rust sidecar all respond.

The preview supports project loading, editing, dimensions, graph queries, project check/build,
native project selection, and external-file reload events. Frontend plugin installation and desktop
auto-update remain Tauri-only until Electron becomes a release target.
