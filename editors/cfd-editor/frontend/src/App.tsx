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
import { GraphView, invalidateGraphCache } from "./components/GraphView";
import { DiagnosticsPanel } from "./components/DiagnosticsPanel";

export default function App() {
  const router = useRouter();
  const project = useProject();
  const [showNewFileModal, setShowNewFileModal] = useState(false);
  const [newFilePath, setNewFilePath] = useState("");
  const [newFileError, setNewFileError] = useState<string | null>(null);
  const [opError, setOpError] = useState<string | null>(null);
  const [graphRefreshKey, setGraphRefreshKey] = useState(0);
  const opErrorTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const showOpError = useCallback((msg: string) => {
    if (opErrorTimerRef.current) clearTimeout(opErrorTimerRef.current);
    setOpError(msg);
    opErrorTimerRef.current = setTimeout(() => { setOpError(null); opErrorTimerRef.current = null; }, 6000);
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

  // Reset router when a DIFFERENT project is loaded (yaml path changes means different project)
  const prevYamlPathRef = useRef<string | null>(null);
  useEffect(() => {
    const currentYaml = project.loadedYamlPath;
    if (currentYaml && prevYamlPathRef.current && currentYaml !== prevYamlPathRef.current) {
      router.reset();
    }
    prevYamlPathRef.current = currentYaml;
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [project.loadedYamlPath]);

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
      invalidateGraphCache(sessionId, filePath);
      setGraphRefreshKey(k => k + 1);
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
      invalidateGraphCache(project.snapshot.session_id, filePath);
      setGraphRefreshKey(k => k + 1);
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
      invalidateGraphCache(sessionId, filePath);
      setGraphRefreshKey(k => k + 1);
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
      invalidateGraphCache(sessionId, filePath);
      setGraphRefreshKey(k => k + 1);
      project.markDirty(sessionId, filePath);
      // If the deleted record is the currently viewed one, navigate back to table (preserve type)
      if (router.current?.view === "record" &&
          "recordKey" in router.current &&
          (router.current as { recordKey: string }).recordKey === recordKey) {
        const deletedType = project.fileRecords?.records.find(r => r.key === recordKey)?.actual_type;
        router.replace({ view: "table", file: filePath, ...(deletedType ? { typeFilter: deletedType } : {}) });
      }
    } catch (e) {
      showOpError(`Delete failed: ${e}`);
      throw e;
    }
  }, [project, router, showOpError]);

  const handleDeleteFile = useCallback(async (filePath: string) => {
    if (!project.snapshot) return;
    const wasViewing = router.current?.file === filePath;
    try {
      await api.deleteFile(project.snapshot.session_id, filePath);
      await project.refreshSnapshot();
      // Reset after refresh so the auto-select effect fires with the updated snapshot
      if (wasViewing) {
        router.reset();
      }
    } catch (err) {
      showOpError(`Delete file failed: ${err}`);
    }
  }, [project, router, showOpError]);

  const handleRenameFile = useCallback(async (oldPath: string, newPath: string) => {
    if (!project.snapshot) return;
    try {
      await api.renameFile(project.snapshot.session_id, oldPath, newPath);
      // Rewrite entire router history before refreshing snapshot
      router.rewriteFile(oldPath, newPath);
      await project.refreshSnapshot();
    } catch (err) {
      showOpError(`Rename file failed: ${err}`);
      throw err;
    }
  }, [project, router, showOpError]);

  const handleNewFile = () => {
    setNewFilePath("");
    setNewFileError(null);
    setShowNewFileModal(true);
  };

  const handleCreateFile = async () => {
    if (!project.snapshot || !newFilePath.trim()) return;
    const trimmed = newFilePath.trim();
    if (!trimmed.endsWith(".cfd")) {
      setNewFileError("File path must end with .cfd");
      return;
    }
    setNewFileError(null);
    try {
      const node = await api.createFile(project.snapshot.session_id, trimmed);
      setShowNewFileModal(false);
      await project.refreshSnapshot();
      router.push({ view: "table", file: node.path });
    } catch (err) {
      setNewFileError(String(err));
    }
  };

  return (
    <div className="app-shell">
      {/* Top bar */}
      <header className="topbar">
        <span className="app-title">CFD Editor</span>
        <button onClick={handleOpen}>Open Project…</button>
        {project.snapshot && (
          <button
            onClick={() => project.refreshSnapshot()}
            title="Reload project from disk (picks up external file changes)"
            style={{ fontSize: 11 }}
          >↺ Reload</button>
        )}
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
              onRenameFile={handleRenameFile}
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
                    onClick={() => {
                      const cur = router.current!;
                      if (v === "table" && cur.view === "record" && "recordKey" in cur) {
                        // Preserve type context when switching from record → table
                        const recordKey = (cur as { recordKey: string }).recordKey;
                        const recordType = project.fileRecords?.records.find(r => r.key === recordKey)?.actual_type;
                        router.replace({ view: "table", file: cur.file, ...(recordType ? { typeFilter: recordType } : {}) });
                      } else {
                        router.replace({ ...cur, view: v as "table" | "record" | "graph" } as Parameters<typeof router.replace>[0]);
                      }
                    }}
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
                    onDeleteRecord={handleDeleteRecord}
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
                    refreshKey={graphRefreshKey}
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
            {newFileError && (
              <div style={{ color: "#ff5555", fontSize: 12, background: "#ff555522", border: "1px solid #ff555544", borderRadius: 4, padding: "4px 8px" }}>
                {newFileError}
              </div>
            )}
            <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 13 }}>
              Relative path (must end with .cfd)
              <input
                value={newFilePath}
                onChange={e => { setNewFilePath(e.target.value); setNewFileError(null); }}
                onKeyDown={e => {
                  if (e.key === "Enter") handleCreateFile();
                  if (e.key === "Escape") setShowNewFileModal(false);
                }}
                style={{
                  background: "var(--bg3)",
                  border: newFileError ? "1px solid #ff5555" : "1px solid var(--border)",
                  borderRadius: 4,
                  color: "var(--text)",
                  padding: "4px 8px",
                  fontSize: 13,
                  fontFamily: "monospace",
                  outline: "none",
                }}
                placeholder="data/my_file.cfd"
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
