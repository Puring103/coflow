import { useEffect, useState, useCallback, useRef } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import "./App.css";
import { useRouter, type Route } from "./router";
import { useProject } from "./hooks/useProject";
import { api } from "./api";
import type { FieldPathSegment, FieldValue, FileTreeNode } from "./bindings";
import { FileTree } from "./components/FileTree";
import { TableView } from "./components/TableView";
import { RecordView } from "./components/RecordView";
import { GraphView } from "./components/GraphView";
import { DiagnosticsPanel } from "./components/DiagnosticsPanel";

export default function App() {
  const router = useRouter();
  const project = useProject();
  const [showNewFileModal, setShowNewFileModal] = useState(false);
  const [newFilePath, setNewFilePath] = useState("");
  const [opError, setOpError] = useState<string | null>(null);

  const showOpError = useCallback((msg: string) => {
    setOpError(msg);
    setTimeout(() => setOpError(null), 6000);
  }, []);

  const currentFile = router.current?.file ?? null;

  // Load file records when file changes; auto-flush dirty for previous file
  useEffect(() => {
    if (!project.snapshot || !currentFile) return;
    project.loadFile(project.snapshot.session_id, currentFile);
  // We only want to re-run when currentFile or session_id changes
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentFile, project.snapshot?.session_id]);

  // When file changes, flush any pending dirty state for a *different* file
  const prevFileRef = useRef<string | null>(null);
  useEffect(() => {
    if (prevFileRef.current && prevFileRef.current !== currentFile) {
      if (project.dirty && project.snapshot) {
        project.saveNow(project.snapshot.session_id, prevFileRef.current);
      }
    }
    prevFileRef.current = currentFile;
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentFile]);

  // Ctrl+S: flush dirty debounce immediately
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "s") {
        e.preventDefault();
        if (project.dirty && project.snapshot && currentFile) {
          project.saveNow(project.snapshot.session_id, currentFile);
        }
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [project.dirty, project.snapshot?.session_id, currentFile]);

  const handleOpen = async () => {
    const path = await open({
      filters: [{ name: "Coflow Project", extensions: ["yaml", "yml"] }],
      multiple: false,
      directory: false,
    });
    if (!path) return;
    const pathStr = typeof path === "string" ? path : path[0];
    await project.loadProject(pathStr);
  };

  // Auto-select first file after loading project, or after the current file is deleted
  useEffect(() => {
    const snap = project.snapshot;
    if (!snap || router.current) return;
    function findFirstFile(nodes: FileTreeNode[]): string | null {
      for (const node of nodes) {
        if (!node.is_dir) return node.path;
        const found = findFirstFile(node.children);
        if (found) return found;
      }
      return null;
    }
    const firstFile = findFirstFile(snap.file_tree);
    if (firstFile) {
      router.push({ view: "table", file: firstFile });
    }
  // Re-run both when session changes (new project) and when current route clears (file deleted)
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [project.snapshot?.session_id, router.current]);

  const handleWriteField = useCallback(async (
    sessionId: number,
    filePath: string,
    recordKey: string,
    fieldPath: FieldPathSegment[],
    newValue: FieldValue
  ) => {
    try {
      // If fieldPath is empty, it's a create record request from TableView
      if (fieldPath.length === 0 && newValue.kind === "Object") {
        await api.createRecord(sessionId, filePath, recordKey, newValue.actual_type);
      } else {
        await api.writeField(sessionId, filePath, recordKey, fieldPath, newValue);
      }
      project.markDirty(sessionId, filePath);
    } catch (e) {
      showOpError(`Write failed: ${e}`);
      throw e;
    }
  }, [project, showOpError]);

  const handleRenameRecord = useCallback(async (oldKey: string, newKey: string) => {
    if (!project.snapshot || !router.current) return;
    const filePath = router.current.file;
    try {
      await api.renameRecord(project.snapshot.session_id, filePath, oldKey, newKey);
      project.markDirty(project.snapshot.session_id, filePath);
      // Navigate to the new key
      router.replace({ view: "record", file: filePath, recordKey: newKey });
    } catch (e) {
      showOpError(`Rename failed: ${e}`);
      throw e;
    }
  }, [project, router, showOpError]);

  const handleRenameRecordFromTable = useCallback(async (
    sessionId: number, filePath: string, oldKey: string, newKey: string
  ) => {
    try {
      await api.renameRecord(sessionId, filePath, oldKey, newKey);
      project.markDirty(sessionId, filePath);
    } catch (e) {
      showOpError(`Rename failed: ${e}`);
      throw e;
    }
  }, [project, showOpError]);

  const handleDeleteRecord = useCallback(async (
    sessionId: number,
    filePath: string,
    recordKey: string
  ) => {
    try {
      await api.deleteRecord(sessionId, filePath, recordKey);
      project.markDirty(sessionId, filePath);
    } catch (e) {
      showOpError(`Delete failed: ${e}`);
      throw e;
    }
  }, [project, showOpError]);

  const handleDeleteFile = useCallback(async (filePath: string) => {
    if (!project.snapshot) return;
    const wasViewing = router.current?.file === filePath;
    try {
      await api.deleteFile(project.snapshot.session_id, filePath);
      await project.refreshSnapshot();
      // Clear navigation so the auto-select effect can pick the first remaining file
      if (wasViewing) {
        router.reset();
      }
    } catch (err) {
      showOpError(`Delete file failed: ${err}`);
    }
  }, [project, router, showOpError]);

  const handleNewFile = () => {
    setNewFilePath("");
    setShowNewFileModal(true);
  };

  const handleCreateFile = async () => {
    if (!project.snapshot || !newFilePath.trim()) return;
    try {
      const node = await api.createFile(project.snapshot.session_id, newFilePath.trim());
      setShowNewFileModal(false);
      await project.refreshSnapshot();
      router.push({ view: "table", file: node.path });
    } catch (err) {
      showOpError(`Create file failed: ${err}`);
    }
  };

  return (
    <div className="app-shell">
      {/* Top bar */}
      <header className="topbar">
        <span className="app-title">CFD Editor</span>
        <button onClick={handleOpen}>Open Project…</button>
        {router.canBack && (
          <button onClick={router.back} title="Back">←</button>
        )}
        {router.canForward && (
          <button onClick={router.forward} title="Forward">→</button>
        )}
        {project.dirty && <span className="dirty-indicator" title="Unsaved changes">●</span>}
        {project.loading && <span style={{ color: "var(--text-muted)", fontSize: 12 }}>Loading…</span>}
        {project.error && (
          <span className="error-msg" title={project.error}>⚠ {project.error}</span>
        )}
        {project.snapshot && (
          <span style={{ color: "var(--text-muted)", fontSize: 11, marginLeft: "auto" }}>
            Session #{project.snapshot.session_id}
          </span>
        )}
      </header>

      {/* Operation error toast */}
      {opError && (
        <div style={{
          position: "fixed",
          bottom: 60,
          left: "50%",
          transform: "translateX(-50%)",
          background: "#ff5555",
          color: "#fff",
          padding: "8px 16px",
          borderRadius: 6,
          fontSize: 13,
          zIndex: 9999,
          maxWidth: 500,
          wordBreak: "break-all",
          boxShadow: "0 2px 12px rgba(0,0,0,0.4)",
        }}>
          {opError}
          <button
            onClick={() => setOpError(null)}
            style={{ marginLeft: 12, background: "none", border: "none", color: "#fff", cursor: "pointer", fontSize: 14 }}
          >✕</button>
        </div>
      )}

      <div className="main-layout">
        {/* Left: file tree */}
        {project.snapshot && (
          <aside className="sidebar">
            <FileTree
              nodes={project.snapshot.file_tree}
              selectedPath={router.current?.file ?? null}
              onSelect={(file) => router.push({ view: "table", file })}
              onNewFile={handleNewFile}
              onDeleteFile={handleDeleteFile}
              sessionId={project.snapshot.session_id}
            />
          </aside>
        )}

        {/* Right: view area */}
        <main className="content">
          {router.current ? (
            <>
              {/* View tabs */}
              <div className="view-tabs">
                {(["table", "record", "graph"] as const).map(v => (
                  <button
                    key={v}
                    className={router.current?.view === v ? "tab active" : "tab"}
                    onClick={() => router.replace({ ...router.current!, view: v as "table" | "record" | "graph" } as Parameters<typeof router.replace>[0])}
                    disabled={v === "record" && !("recordKey" in (router.current ?? {}))}
                  >
                    {v === "table" ? "Table" : v === "record" ? "Record" : "Graph"}
                  </button>
                ))}
                {router.current.file && (
                  <span style={{
                    marginLeft: "auto",
                    color: "var(--text-muted)",
                    fontSize: 11,
                    fontFamily: "monospace",
                    alignSelf: "center",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                    maxWidth: 300,
                  }}>
                    {router.current.file}
                  </span>
                )}
              </div>

              {/* View content */}
              <div style={{ flex: 1, display: "flex", overflow: "hidden" }}>
                {router.current.view === "table" && project.fileRecords && (
                  <TableView
                    fileRecords={project.fileRecords}
                    sessionId={project.snapshot?.session_id ?? 0}
                    filePath={router.current.file}
                    initialTypeFilter={router.current.typeFilter}
                    onTypeChange={typeName => {
                      if (router.current?.view === "table") {
                        router.replace({ ...router.current, typeFilter: typeName });
                      }
                    }}
                    onWriteField={handleWriteField}
                    onDeleteRecord={handleDeleteRecord}
                    onRenameRecord={handleRenameRecordFromTable}
                    onNavigate={router.push}
                  />
                )}
                {router.current.view === "table" && !project.fileRecords && (
                  <div style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center", color: "var(--text-muted)" }}>
                    {project.loading ? "Loading…" : "Select a file to view records"}
                  </div>
                )}

                {router.current.view === "record" && "recordKey" in router.current && (
                  <RecordView
                    sessionId={project.snapshot?.session_id ?? 0}
                    filePath={router.current.file}
                    recordKey={(router.current as { view: "record"; file: string; recordKey: string }).recordKey}
                    fileRecords={project.fileRecords}
                    onWriteField={handleWriteField}
                    onRenameRecord={handleRenameRecord}
                    onNavigate={router.push}
                  />
                )}
                {router.current.view === "record" && !("recordKey" in router.current) && (
                  <div style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center", color: "var(--text-muted)" }}>
                    No record selected. Open a record from the table view.
                  </div>
                )}

                {router.current.view === "graph" && (
                  <GraphView
                    sessionId={project.snapshot?.session_id ?? 0}
                    filePath={router.current.file}
                    onNavigate={router.push}
                  />
                )}
              </div>
            </>
          ) : (
            <div className="welcome">
              <p>Open a <code>coflow.yaml</code> to get started.</p>
            </div>
          )}
        </main>
      </div>

      {/* Diagnostics panel */}
      {project.snapshot && (
        <DiagnosticsPanel diagnostics={project.snapshot.diagnostics} onNavigate={router.push} />
      )}

      {/* New file modal */}
      {showNewFileModal && (
        <div
          style={{
            position: "fixed",
            inset: 0,
            background: "rgba(0,0,0,0.6)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            zIndex: 2000,
          }}
          onClick={() => setShowNewFileModal(false)}
        >
          <div
            style={{
              background: "var(--bg2)",
              border: "1px solid var(--border)",
              borderRadius: 8,
              padding: 24,
              width: 360,
              display: "flex",
              flexDirection: "column",
              gap: 12,
            }}
            onClick={e => e.stopPropagation()}
          >
            <h3 style={{ margin: 0, fontSize: 15 }}>New File</h3>
            <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 13 }}>
              Relative path
              <input
                value={newFilePath}
                onChange={e => setNewFilePath(e.target.value)}
                onKeyDown={e => {
                  if (e.key === "Enter") handleCreateFile();
                  if (e.key === "Escape") setShowNewFileModal(false);
                }}
                style={{
                  background: "var(--bg3)",
                  border: "1px solid var(--border)",
                  borderRadius: 4,
                  color: "var(--text)",
                  padding: "4px 8px",
                  fontSize: 13,
                  fontFamily: "monospace",
                  outline: "none",
                }}
                placeholder="data/my_file.yaml"
                autoFocus
              />
            </label>
            <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
              <button onClick={() => setShowNewFileModal(false)}>Cancel</button>
              <button
                className="primary"
                onClick={handleCreateFile}
                disabled={!newFilePath.trim()}
              >
                Create
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
