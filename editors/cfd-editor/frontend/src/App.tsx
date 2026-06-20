import { useEffect, useState, useCallback, useRef } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import "./App.css";
import { useRouter, type Route } from "./router";
import { useProject } from "./hooks/useProject";
import { api } from "./api";
import type { FieldPathSegment, FieldValue, FileTreeNode, RecordBrief } from "./bindings";
import { FileTree } from "./components/FileTree";
import { TableView } from "./components/TableView";
import { RecordView } from "./components/RecordView";
import { GraphView, invalidateGraphCache } from "./components/GraphView";
import { DiagnosticsPanel } from "./components/DiagnosticsPanel";
import { CommandPalette } from "./components/CommandPalette";
import { GlobalSearch } from "./components/GlobalSearch";
import { GlobalTableView } from "./components/GlobalTableView";

function collectFilePaths(nodes: FileTreeNode[]): string[] {
  const paths: string[] = [];
  for (const node of nodes) {
    if (!node.is_dir && node.path.endsWith(".cfd") && node.in_sources) {
      paths.push(node.path);
    }
    if (node.is_dir) paths.push(...collectFilePaths(node.children));
  }
  return paths;
}

interface MoveRecordModal {
  srcFile: string;
  recordKey: string;
  dstFile: string;
  error: string | null;
}

interface CopyRecordModal {
  srcFile: string;
  recordKey: string;
  dstFile: string;
  newKey: string;
  error: string | null;
}

const RECENT_KEY = "cfd-recent-projects";
const RECENT_MAX = 8;

function loadRecent(): string[] {
  try { return JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]"); } catch { return []; }
}

function pushRecent(path: string) {
  const list = [path, ...loadRecent().filter(p => p !== path)].slice(0, RECENT_MAX);
  try { localStorage.setItem(RECENT_KEY, JSON.stringify(list)); } catch { /* ignore */ }
}

export default function App() {
  const router = useRouter();
  const project = useProject();
  const [recentProjects, setRecentProjects] = useState<string[]>(() => loadRecent());
  const [showNewFileModal, setShowNewFileModal] = useState(false);
  const [newFilePath, setNewFilePath] = useState("");
  const [newFileError, setNewFileError] = useState<string | null>(null);
  const [moveRecordModal, setMoveRecordModal] = useState<MoveRecordModal | null>(null);
  const [copyRecordModal, setCopyRecordModal] = useState<CopyRecordModal | null>(null);
  const [opError, setOpError] = useState<string | null>(null);
  const [graphRefreshKey, setGraphRefreshKey] = useState(0);
  const [showCommandPalette, setShowCommandPalette] = useState(false);
  const [paletteRecords, setPaletteRecords] = useState<RecordBrief[]>([]);
  const [showGlobalSearch, setShowGlobalSearch] = useState(false);
  const [showStats, setShowStats] = useState(false);
  const [statsData, setStatsData] = useState<RecordBrief[] | null>(null);
  const opErrorTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  interface UndoEntry {
    sessionId: number;
    filePath: string;
    recordKey: string;
    fieldPath: FieldPathSegment[];
    oldValue: FieldValue;
    newValue: FieldValue;
  }
  const undoStackRef = useRef<UndoEntry[]>([]);
  const [canUndo, setCanUndo] = useState(false);
  const [undoCount, setUndoCount] = useState(0);

  const showOpError = useCallback((msg: string) => {
    if (opErrorTimerRef.current) clearTimeout(opErrorTimerRef.current);
    setOpError(msg);
    opErrorTimerRef.current = setTimeout(() => { setOpError(null); opErrorTimerRef.current = null; }, 6000);
  }, []);

  const currentFile = (router.current && router.current.view !== "global-table") ? router.current.file : null;

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

  const handleUndo = useCallback(async () => {
    const entry = undoStackRef.current.pop();
    if (!entry) return;
    setCanUndo(undoStackRef.current.length > 0);
    setUndoCount(undoStackRef.current.length);
    try {
      await api.writeField(entry.sessionId, entry.filePath, entry.recordKey, entry.fieldPath, entry.oldValue);
      invalidateGraphCache(entry.sessionId, entry.filePath);
      setGraphRefreshKey(k => k + 1);
      project.markDirty(entry.sessionId, entry.filePath);
    } catch (e) {
      showOpError(`Undo failed: ${e}`);
    }
  }, [project, showOpError]);

  // Ctrl+S: flush dirty debounce immediately
  // Alt+Left/Right: navigate history
  // Ctrl+P: open command palette (jump to record)
  // Ctrl+Z: undo last field write
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && e.key === "z") {
        e.preventDefault();
        handleUndo();
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.key === "s") {
        e.preventDefault();
        if (project.dirty && project.snapshot && currentFile) {
          project.saveNow(project.snapshot.session_id, currentFile);
        }
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.key === "p") {
        e.preventDefault();
        if (project.snapshot) {
          api.getAllRecordsBrief(project.snapshot.session_id)
            .then(records => { setPaletteRecords(records); setShowCommandPalette(true); })
            .catch(e => showOpError(`Failed to open command palette: ${e}`));
        }
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key === "G") {
        e.preventDefault();
        if (project.snapshot) setShowGlobalSearch(true);
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key === "T") {
        e.preventDefault();
        if (project.snapshot) {
          api.getAllTypeNames(project.snapshot.session_id)
            .then(names => {
              if (names.length > 0) router.push({ view: "global-table", typeName: names[0] });
            })
            .catch(e2 => showOpError(`加载类型列表失败: ${e2}`));
        }
        return;
      }
      if (e.altKey && e.key === "ArrowLeft" && router.canBack) {
        e.preventDefault();
        router.back();
        return;
      }
      if (e.altKey && e.key === "ArrowRight" && router.canForward) {
        e.preventDefault();
        router.forward();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [project.dirty, project.snapshot?.session_id, currentFile, router.canBack, router.canForward, handleUndo]);

  const handleOpen = async () => {
    const path = await open({
      filters: [{ name: "Coflow Project", extensions: ["yaml", "yml"] }],
      multiple: false,
      directory: false,
    });
    if (!path) return;
    const pathStr = typeof path === "string" ? path : path[0];
    await project.loadProject(pathStr);
    pushRecent(pathStr);
    setRecentProjects(loadRecent());
  };

  const handleOpenPath = async (pathStr: string) => {
    await project.loadProject(pathStr);
    pushRecent(pathStr);
    setRecentProjects(loadRecent());
  };

  // Reset router when a DIFFERENT project is loaded (yaml path changes means different project)
  const prevYamlPathRef = useRef<string | null>(null);
  useEffect(() => {
    const currentYaml = project.loadedYamlPath;
    if (currentYaml && prevYamlPathRef.current && currentYaml !== prevYamlPathRef.current) {
      router.reset();
      undoStackRef.current = [];
      setCanUndo(false);
      setUndoCount(0);
    }
    prevYamlPathRef.current = currentYaml;
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [project.loadedYamlPath]);

  // Auto-select first file after loading project, or after the current file is deleted.
  // Also reset router if the currently viewed file is no longer in the snapshot (e.g. after external delete + Reload).
  useEffect(() => {
    const snap = project.snapshot;
    if (!snap) return;

    function findFirstFile(nodes: FileTreeNode[]): string | null {
      for (const node of nodes) {
        if (!node.is_dir) return node.path;
        const found = findFirstFile(node.children);
        if (found) return found;
      }
      return null;
    }

    function fileExists(nodes: FileTreeNode[], path: string): boolean {
      for (const node of nodes) {
        if (!node.is_dir && node.path === path) return true;
        if (node.is_dir && fileExists(node.children, path)) return true;
      }
      return false;
    }

    if (!router.current) {
      const firstFile = findFirstFile(snap.file_tree);
      if (firstFile) router.push({ view: "table", file: firstFile });
    } else if (router.current.view !== "global-table" && !fileExists(snap.file_tree, router.current.file)) {
      // Current file no longer exists in the snapshot — reset and auto-select
      router.reset();
    }
  // Re-run when session changes (new project), current route changes, or snapshot updates
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [project.snapshot?.session_id, router.current]);

  const handleWriteField = useCallback(async (
    sessionId: number,
    filePath: string,
    recordKey: string,
    fieldPath: FieldPathSegment[],
    newValue: FieldValue,
    oldValue?: FieldValue
  ) => {
    try {
      // If fieldPath is empty, it's a create record request from TableView
      if (fieldPath.length === 0 && newValue.kind === "Object") {
        await api.createRecord(sessionId, filePath, recordKey, newValue.actual_type);
      } else {
        await api.writeField(sessionId, filePath, recordKey, fieldPath, newValue);
        // Record undo entry for field writes (not record creation)
        if (oldValue !== undefined) {
          const stack = undoStackRef.current;
          stack.push({ sessionId, filePath, recordKey, fieldPath, oldValue, newValue });
          if (stack.length > 50) stack.splice(0, stack.length - 50);
          setCanUndo(true);
          setUndoCount(stack.length);
        }
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
    if (!project.snapshot || !router.current || router.current.view === "global-table") return;
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

  const handleDuplicateRecord = useCallback(async (
    sessionId: number,
    filePath: string,
    srcKey: string,
    newKey: string
  ) => {
    try {
      await api.duplicateRecord(sessionId, filePath, srcKey, newKey);
      invalidateGraphCache(sessionId, filePath);
      setGraphRefreshKey(k => k + 1);
      project.markDirty(sessionId, filePath);
      router.push({ view: "record", file: filePath, recordKey: newKey });
    } catch (e) {
      showOpError(`Duplicate failed: ${e}`);
      throw e;
    }
  }, [project, router, showOpError]);

  const handleWriteRecordSource = useCallback(async (filePath: string, recordKey: string, source: string) => {
    if (!project.snapshot) return;
    try {
      await api.writeRecordSource(project.snapshot.session_id, filePath, recordKey, source);
      invalidateGraphCache(project.snapshot.session_id, filePath);
      setGraphRefreshKey(k => k + 1);
      project.markDirty(project.snapshot.session_id, filePath);
    } catch (e) {
      showOpError(`Write source failed: ${e}`);
      throw e;
    }
  }, [project, showOpError]);

  const handleMoveRecordCommit = useCallback(async () => {
    if (!moveRecordModal || !project.snapshot) return;
    const { srcFile, dstFile, recordKey } = moveRecordModal;
    if (!dstFile || dstFile === srcFile) {
      setMoveRecordModal(m => m && ({ ...m, error: "Choose a different destination file" }));
      return;
    }
    try {
      await api.moveRecord(project.snapshot.session_id, srcFile, dstFile, recordKey);
      invalidateGraphCache(project.snapshot.session_id, srcFile);
      invalidateGraphCache(project.snapshot.session_id, dstFile);
      setGraphRefreshKey(k => k + 1);
      project.markDirty(project.snapshot.session_id, srcFile);
      project.markDirty(project.snapshot.session_id, dstFile);
      setMoveRecordModal(null);
      // Navigate to the record in its new file
      router.replace({ view: "record", file: dstFile, recordKey });
    } catch (e) {
      setMoveRecordModal(m => m && ({ ...m, error: String(e) }));
    }
  }, [moveRecordModal, project, router]);

  const handleCopyRecordCommit = useCallback(async () => {
    if (!copyRecordModal || !project.snapshot) return;
    const { srcFile, dstFile, recordKey, newKey } = copyRecordModal;
    if (!dstFile) {
      setCopyRecordModal(m => m && ({ ...m, error: "选择目标文件" }));
      return;
    }
    if (!newKey.trim()) {
      setCopyRecordModal(m => m && ({ ...m, error: "新 Key 不能为空" }));
      return;
    }
    try {
      await api.copyRecordToFile(project.snapshot.session_id, srcFile, dstFile, recordKey, newKey.trim());
      invalidateGraphCache(project.snapshot.session_id, dstFile);
      setGraphRefreshKey(k => k + 1);
      project.markDirty(project.snapshot.session_id, dstFile);
      setCopyRecordModal(null);
      router.push({ view: "record", file: dstFile, recordKey: newKey.trim() });
    } catch (e) {
      setCopyRecordModal(m => m && ({ ...m, error: String(e) }));
    }
  }, [copyRecordModal, project, router]);

  const handleDeleteFile = useCallback(async (filePath: string) => {
    if (!project.snapshot) return;
    const wasViewing = router.current && router.current.view !== "global-table" && router.current.file === filePath;
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

  const handleReloadFile = useCallback(async (filePath: string) => {
    if (!project.snapshot) return;
    try {
      await api.reloadFileFromDisk(project.snapshot.session_id, filePath);
      project.markDirty(project.snapshot.session_id, filePath);
    } catch (err) {
      showOpError(`Reload file failed: ${err}`);
    }
  }, [project, showOpError]);

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
        {project.loadedYamlPath && (
          <span style={{
            fontSize: 11,
            color: "var(--text-muted)",
            fontFamily: "monospace",
            maxWidth: 200,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }} title={project.loadedYamlPath}>
            {project.loadedYamlPath.split(/[\\/]/).slice(-2).join("/")}
          </span>
        )}
        <button onClick={handleOpen}>Open Project…</button>
        {project.snapshot && (
          <button
            onClick={() => project.refreshSnapshot()}
            title="Reload project from disk (picks up external file changes)"
            style={{ fontSize: 11 }}
          >↺ Reload</button>
        )}
        {project.snapshot && (
          <button
            onClick={async () => {
              if (!project.snapshot) return;
              try {
                const count = await api.sortAllFiles(project.snapshot.session_id);
                if (count > 0) {
                  const filePaths = project.snapshot.file_tree
                    ? (() => { const f: string[] = []; const visit = (n: import("./bindings").FileTreeNode[]) => { for (const x of n) { if (!x.is_dir && x.in_sources) f.push(x.path); if (x.is_dir) visit(x.children); } }; visit(project.snapshot!.file_tree); return f; })()
                    : [];
                  for (const fp of filePaths) project.markDirty(project.snapshot!.session_id, fp);
                  setGraphRefreshKey(k => k + 1);
                }
              } catch (e) { showOpError(`Sort all files failed: ${e}`); }
            }}
            title="Sort all files: sort records alphabetically by key in every .cfd file"
            style={{ fontSize: 11 }}
          >⇅ Sort All</button>
        )}
        {router.canBack && (
          <button onClick={router.back} title="Back (Alt+Left)">←</button>
        )}
        {router.canForward && (
          <button onClick={router.forward} title="Forward (Alt+Right)">→</button>
        )}
        {canUndo && (() => {
          const top = undoStackRef.current[undoStackRef.current.length - 1];
          const tipDetail = top ? ` — ${top.recordKey}.${top.fieldPath.map(s => s.kind === "Field" ? s.name : `[${s.i}]`).join(".")}` : "";
          return (
            <button onClick={handleUndo} title={`Undo last field edit (Ctrl+Z)${tipDetail}\n${undoCount} step${undoCount !== 1 ? "s" : ""} available`} style={{ fontSize: 11 }}>
              ↩ Undo {undoCount > 1 ? <span style={{ opacity: 0.6, fontSize: 10 }}>×{undoCount}</span> : null}
            </button>
          );
        })()}
        {project.dirty && <span className="dirty-indicator" title="Unsaved changes — reloading data…">●</span>}
        {project.loading && <span style={{ color: "var(--text-muted)", fontSize: 12 }}>Loading…</span>}
        {project.error && (
          <span className="error-msg" title={project.error}>⚠ {project.error}</span>
        )}
        {project.snapshot && (
          <button
            onClick={() => {
              api.getAllRecordsBrief(project.snapshot!.session_id)
                .then(records => { setPaletteRecords(records); setShowCommandPalette(true); })
                .catch(e => showOpError(`Failed to open command palette: ${e}`));
            }}
            title="Jump to record (Ctrl+P)"
            style={{ fontSize: 11 }}
          >⌕ Jump to…</button>
        )}
        {project.snapshot && (
          <button
            onClick={() => setShowGlobalSearch(true)}
            title="Global search — search all records by value (Ctrl+Shift+G)"
            style={{ fontSize: 11 }}
          >⌕ 全局搜索</button>
        )}
        {project.snapshot && (
          <button
            onClick={() => {
              api.getAllTypeNames(project.snapshot!.session_id)
                .then(names => {
                  if (names.length > 0) router.push({ view: "global-table", typeName: names[0] });
                })
                .catch(e => showOpError(`Failed to open global table: ${e}`));
            }}
            title="全局表视图 — 跨文件查看同类型记录 (Ctrl+Shift+T)"
            style={{ fontSize: 11 }}
          >☰ 全局表</button>
        )}
        <button
          title={[
            "Keyboard Shortcuts",
            "─────────────────",
            "Ctrl+Z         Undo last field edit (up to 50 steps)",
            "Ctrl+P         Jump to record (command palette)",
            "Ctrl+Shift+G   Global search by key or field value",
            "Ctrl+Shift+T   Open global table (cross-file by type)",
            "Ctrl+S         Save / flush diagnostics",
            "Alt+← / →     Back / Forward",
            "Ctrl+N         New record (in table/record view)",
            "Ctrl+D         Duplicate current record (table / record view)",
            "Ctrl+F         Filter fields (record view) / filter records (table view)",
            "Ctrl+Shift+F   Focus sidebar search (record view)",
            "Ctrl+Shift+R   Jump to next required empty field (record view)",
            "Ctrl+Shift+E   Expand all nodes (graph view)",
            "Ctrl+Shift+W   Collapse all nodes (graph view)",
            "Escape         Cancel edit / close modal",
          ].join("\n")}
          style={{ marginLeft: "auto", fontSize: 11, opacity: 0.6 }}
        >?</button>
        {project.snapshot && (
          <button
            onClick={() => {
              if (showStats) { setShowStats(false); return; }
              api.getAllRecordsBrief(project.snapshot!.session_id)
                .then(records => { setStatsData(records); setShowStats(true); })
                .catch(e => showOpError(`加载统计失败: ${e}`));
            }}
            title="Project statistics"
            style={{ fontSize: 11, opacity: 0.6 }}
          >
            ≡ Stats
          </button>
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
              selectedPath={(router.current && router.current.view !== "global-table") ? router.current.file : null}
              sessionId={project.snapshot.session_id}
              onSelect={(file) => router.push({ view: "table", file })}
              onNewFile={handleNewFile}
              onDeleteFile={handleDeleteFile}
              onRenameFile={handleRenameFile}
              onReloadFile={handleReloadFile}
              onError={showOpError}
            />
          </aside>
        )}

        {/* Right: view area */}
        <main className="content">
          {router.current ? (
            <>
              {/* View tabs */}
              <div className="view-tabs">
                {router.current.view === "global-table" ? (
                  <button
                    className="tab active"
                  >
                    全局表
                  </button>
                ) : (
                  (["table", "record", "graph"] as const).map(v => (
                    <button
                      key={v}
                      className={router.current?.view === v ? "tab active" : "tab"}
                      onClick={() => {
                        const cur = router.current!;
                        if (cur.view === "global-table") return;
                        if (v === "table" && cur.view === "record" && "recordKey" in cur) {
                          const recordKey = (cur as { recordKey: string }).recordKey;
                          const recordType = project.fileRecords?.records.find(r => r.key === recordKey)?.actual_type;
                          router.replace({ view: "table", file: cur.file, ...(recordType ? { typeFilter: recordType } : {}) });
                        } else if (v === "graph") {
                          router.replace({ view: "graph", file: cur.file });
                        } else if (v === "table") {
                          const typeFilter = cur.view === "table" ? cur.typeFilter : undefined;
                          router.replace({ view: "table", file: cur.file, ...(typeFilter ? { typeFilter } : {}) });
                        } else {
                          if ("recordKey" in cur) {
                            router.replace({ view: "record", file: cur.file, recordKey: (cur as { recordKey: string }).recordKey });
                          }
                        }
                      }}
                      disabled={v === "record" && !("recordKey" in (router.current ?? {}))}
                    >
                      {v === "table" ? "Table" : v === "record" ? "Record" : "Graph"}
                    </button>
                  ))
                )}
                {"file" in (router.current ?? {}) && (router.current as { file?: string }).file && (
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
                    {(router.current as { file?: string }).file}
                  </span>
                )}
              </div>

              {/* View content */}
              <div style={{ flex: 1, display: "flex", overflow: "hidden" }}>
                {(() => {
                  const curFile = router.current.view !== "global-table" ? router.current.file : null;
                  const matchedRecords = project.fileRecords?.file_path === curFile ? project.fileRecords : null;
                  return router.current.view === "table" && matchedRecords ? (
                    <TableView
                      fileRecords={matchedRecords}
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
                      onDuplicateRecord={handleDuplicateRecord}
                      onMoveRecord={(srcFile, recordKey) => {
                        const availableFiles = collectFilePaths(project.snapshot?.file_tree ?? []);
                        const firstOther = availableFiles.find(f => f !== srcFile) ?? srcFile;
                        setMoveRecordModal({ srcFile, recordKey, dstFile: firstOther, error: null });
                      }}
                      onCopyRecord={(srcFile, recordKey) => {
                        const availableFiles = collectFilePaths(project.snapshot?.file_tree ?? []);
                        const firstOther = availableFiles.find(f => f !== srcFile) ?? srcFile;
                        setCopyRecordModal({ srcFile, recordKey, dstFile: firstOther, newKey: `${recordKey}_copy`, error: null });
                      }}
                      onSortFile={project.snapshot ? async () => {
                        if (!project.snapshot) return;
                        const currentFile = router.current?.view !== "global-table" ? router.current?.file : undefined;
                        if (!currentFile) return;
                        try {
                          const count = await api.sortFileRecords(project.snapshot.session_id, currentFile);
                          if (count > 0) {
                            project.markDirty(project.snapshot.session_id, currentFile);
                            setGraphRefreshKey(k => k + 1);
                          }
                        } catch (e) {
                          showOpError(String(e));
                        }
                      } : undefined}
                      onNavigate={router.push}
                      diagnostics={project.snapshot?.diagnostics}
                      onError={showOpError}
                    />
                  ) : router.current.view === "table" ? (
                    <div style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center", color: "var(--text-muted)" }}>
                      Loading…
                    </div>
                  ) : null;
                })()}

                {router.current.view === "record" && "recordKey" in router.current && (
                  <RecordView
                    sessionId={project.snapshot?.session_id ?? 0}
                    filePath={router.current.file}
                    recordKey={(router.current as { view: "record"; file: string; recordKey: string }).recordKey}
                    initialFieldSearch={(router.current as { view: "record"; file: string; recordKey: string; fieldSearch?: string }).fieldSearch}
                    fileRecords={project.fileRecords?.file_path === router.current.file ? project.fileRecords : null}
                    onWriteField={handleWriteField}
                    onRenameRecord={handleRenameRecord}
                    onDeleteRecord={handleDeleteRecord}
                    onDuplicateRecord={handleDuplicateRecord}
                    onMoveRecord={(srcFile, recordKey) => {
                      const availableFiles = collectFilePaths(project.snapshot?.file_tree ?? []);
                      const firstOther = availableFiles.find(f => f !== srcFile) ?? srcFile;
                      setMoveRecordModal({ srcFile, recordKey, dstFile: firstOther, error: null });
                    }}
                    onCopyRecord={(srcFile, recordKey) => {
                      const availableFiles = collectFilePaths(project.snapshot?.file_tree ?? []);
                      const firstOther = availableFiles.find(f => f !== srcFile) ?? srcFile;
                      setCopyRecordModal({ srcFile, recordKey, dstFile: firstOther, newKey: `${recordKey}_copy`, error: null });
                    }}
                    onWriteRecordSource={handleWriteRecordSource}
                    onError={showOpError}
                    onNavigate={router.push}
                    diagnostics={project.snapshot?.diagnostics}
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
                    onError={showOpError}
                  />
                )}

                {router.current.view === "global-table" && project.snapshot && (
                  <GlobalTableView
                    sessionId={project.snapshot.session_id}
                    typeName={router.current.typeName}
                    refreshKey={graphRefreshKey}
                    onTypeChange={typeName => router.replace({ view: "global-table", typeName })}
                    onNavigate={router.push}
                    onWriteField={handleWriteField}
                    onDeleteRecord={handleDeleteRecord}
                    onMoveRecord={(srcFile, recordKey) => {
                      const availableFiles = collectFilePaths(project.snapshot?.file_tree ?? []);
                      const firstOther = availableFiles.find(f => f !== srcFile) ?? srcFile;
                      setMoveRecordModal({ srcFile, recordKey, dstFile: firstOther, error: null });
                    }}
                    onCopyRecord={(srcFile, recordKey) => {
                      const availableFiles = collectFilePaths(project.snapshot?.file_tree ?? []);
                      const firstOther = availableFiles.find(f => f !== srcFile) ?? srcFile;
                      setCopyRecordModal({ srcFile, recordKey, dstFile: firstOther, newKey: `${recordKey}_copy`, error: null });
                    }}
                    diagnostics={project.snapshot.diagnostics}
                    onError={showOpError}
                  />
                )}
              </div>
            </>
          ) : (
            <div className="welcome">
              <p>Open a <code>coflow.yaml</code> to get started.</p>
              <button className="primary" onClick={handleOpen} style={{ marginTop: 8 }}>
                Open Project…
              </button>
              {recentProjects.length > 0 && (
                <div style={{ marginTop: 24, textAlign: "left", maxWidth: 480 }}>
                  <div style={{ fontSize: 11, color: "var(--text-muted)", marginBottom: 6, textTransform: "uppercase", letterSpacing: 1 }}>
                    Recent Projects
                  </div>
                  {recentProjects.map(p => (
                    <div
                      key={p}
                      onClick={() => handleOpenPath(p)}
                      style={{
                        padding: "5px 10px",
                        borderRadius: 4,
                        cursor: "pointer",
                        fontFamily: "monospace",
                        fontSize: 12,
                        color: "var(--accent)",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                        maxWidth: 480,
                      }}
                      onMouseEnter={e => (e.currentTarget.style.background = "var(--bg3)")}
                      onMouseLeave={e => (e.currentTarget.style.background = "transparent")}
                      title={p}
                    >
                      {p}
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}
        </main>
      </div>

      {/* Diagnostics panel */}
      {project.snapshot && (
        <DiagnosticsPanel diagnostics={project.snapshot.diagnostics} onNavigate={router.push} currentFile={(router.current && router.current.view !== "global-table") ? router.current.file : undefined} onError={showOpError} />
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

      {/* Command palette (Ctrl+P) */}
      {showCommandPalette && (
        <CommandPalette
          records={paletteRecords}
          onNavigate={(filePath, recordKey) => {
            router.push({ view: "record", file: filePath, recordKey });
          }}
          onClose={() => setShowCommandPalette(false)}
        />
      )}

      {/* Global search (Ctrl+Shift+G) */}
      {showGlobalSearch && project.snapshot && (
        <GlobalSearch
          sessionId={project.snapshot.session_id}
          onNavigate={router.push}
          onClose={() => setShowGlobalSearch(false)}
        />
      )}

      {/* Stats popover */}
      {showStats && statsData && (
        <div
          style={{ position: "fixed", inset: 0, zIndex: 3000 }}
          onClick={() => setShowStats(false)}
        >
          <div
            onClick={e => e.stopPropagation()}
            style={{
              position: "absolute",
              top: 38,
              right: 8,
              background: "var(--bg2)",
              border: "1px solid var(--border)",
              borderRadius: 8,
              padding: 16,
              width: 320,
              maxHeight: 420,
              overflowY: "auto",
              boxShadow: "0 4px 24px rgba(0,0,0,0.4)",
              fontSize: 12,
            }}
          >
            <div style={{ fontWeight: 600, fontSize: 13, marginBottom: 10, display: "flex", justifyContent: "space-between", alignItems: "center" }}>
              Project Statistics
              <button onClick={() => setShowStats(false)} style={{ background: "none", border: "none", color: "var(--text-muted)", cursor: "pointer", fontSize: 14 }}>✕</button>
            </div>
            <div style={{ marginBottom: 8, color: "var(--text-muted)" }}>
              Total: {statsData.length} records · {statsData.filter(r => r.is_fallback).length > 0 && <span style={{ color: "var(--warning)" }}>{statsData.filter(r => r.is_fallback).length} fallback</span>}
            </div>
            <div style={{ fontWeight: 600, fontSize: 11, color: "var(--text-muted)", textTransform: "uppercase", letterSpacing: 1, marginBottom: 4 }}>By Type</div>
            {(() => {
              const byType = new Map<string, number>();
              for (const r of statsData) byType.set(r.actual_type, (byType.get(r.actual_type) ?? 0) + 1);
              return [...byType.entries()].sort((a, b) => b[1] - a[1]).map(([type, count]) => (
                <div key={type} style={{ display: "flex", justifyContent: "space-between", padding: "2px 0", borderBottom: "1px solid var(--bg3)" }}>
                  <span style={{ fontFamily: "monospace", color: "var(--accent)" }}>{type}</span>
                  <span style={{ color: "var(--text-muted)" }}>{count}</span>
                </div>
              ));
            })()}
            <div style={{ fontWeight: 600, fontSize: 11, color: "var(--text-muted)", textTransform: "uppercase", letterSpacing: 1, margin: "10px 0 4px" }}>By File</div>
            {(() => {
              const byFile = new Map<string, number>();
              for (const r of statsData) {
                const name = r.file_path.split(/[\\/]/).pop() ?? r.file_path;
                byFile.set(name, (byFile.get(name) ?? 0) + 1);
              }
              return [...byFile.entries()].sort((a, b) => b[1] - a[1]).map(([file, count]) => (
                <div key={file} style={{ display: "flex", justifyContent: "space-between", padding: "2px 0", borderBottom: "1px solid var(--bg3)" }}>
                  <span style={{ fontFamily: "monospace", color: "var(--text-muted)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{file}</span>
                  <span style={{ color: "var(--text-muted)", flexShrink: 0, marginLeft: 8 }}>{count}</span>
                </div>
              ));
            })()}
          </div>
        </div>
      )}

      {/* Move record modal */}
      {moveRecordModal && (
        <div
          style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,0.6)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 3000 }}
          onClick={() => setMoveRecordModal(null)}
        >
          <div
            style={{ background: "var(--bg2)", border: "1px solid var(--border)", borderRadius: 8, padding: 24, width: 380, display: "flex", flexDirection: "column", gap: 16 }}
            onClick={e => e.stopPropagation()}
          >
            <div style={{ fontWeight: 600, fontSize: 14 }}>移动记录到文件</div>
            <div style={{ fontSize: 12, color: "var(--text-muted)" }}>
              记录 <code style={{ color: "var(--text)", fontFamily: "monospace" }}>{moveRecordModal.recordKey}</code> 将被移动到:
            </div>
            <select
              value={moveRecordModal.dstFile}
              onChange={e => setMoveRecordModal(m => m && ({ ...m, dstFile: e.target.value, error: null }))}
              style={{ background: "var(--bg3)", border: "1px solid var(--border)", borderRadius: 4, color: "var(--text)", padding: "4px 8px", fontSize: 13, fontFamily: "monospace", outline: "none" }}
              autoFocus
            >
              {collectFilePaths(project.snapshot?.file_tree ?? []).map(p => (
                <option key={p} value={p} disabled={p === moveRecordModal.srcFile}>{p}</option>
              ))}
            </select>
            {moveRecordModal.error && (
              <div style={{ color: "#ff5555", fontSize: 12 }}>{moveRecordModal.error}</div>
            )}
            <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
              <button onClick={() => setMoveRecordModal(null)}>Cancel</button>
              <button
                className="primary"
                onClick={handleMoveRecordCommit}
                disabled={!moveRecordModal.dstFile || moveRecordModal.dstFile === moveRecordModal.srcFile}
              >
                移动
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Copy record to file modal */}
      {copyRecordModal && (
        <div
          style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,0.6)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 3000 }}
          onClick={() => setCopyRecordModal(null)}
        >
          <div
            style={{ background: "var(--bg2)", border: "1px solid var(--border)", borderRadius: 8, padding: 24, width: 380, display: "flex", flexDirection: "column", gap: 16 }}
            onClick={e => e.stopPropagation()}
          >
            <div style={{ fontWeight: 600, fontSize: 14 }}>复制记录到文件</div>
            <div style={{ fontSize: 12, color: "var(--text-muted)" }}>
              记录 <code style={{ color: "var(--text)", fontFamily: "monospace" }}>{copyRecordModal.recordKey}</code> 将被复制到（原记录保留）:
            </div>
            <select
              value={copyRecordModal.dstFile}
              onChange={e => setCopyRecordModal(m => m && ({ ...m, dstFile: e.target.value, error: null }))}
              style={{ background: "var(--bg3)", border: "1px solid var(--border)", borderRadius: 4, color: "var(--text)", padding: "4px 8px", fontSize: 13, fontFamily: "monospace", outline: "none" }}
              autoFocus
            >
              {collectFilePaths(project.snapshot?.file_tree ?? []).map(p => (
                <option key={p} value={p}>{p}</option>
              ))}
            </select>
            <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 13 }}>
              新 Key（可与原 Key 相同，但目标文件中不能已存在）
              <input
                value={copyRecordModal.newKey}
                onChange={e => setCopyRecordModal(m => m && ({ ...m, newKey: e.target.value, error: null }))}
                onKeyDown={e => {
                  if (e.key === "Enter") { e.preventDefault(); handleCopyRecordCommit(); }
                  if (e.key === "Escape") setCopyRecordModal(null);
                  e.stopPropagation();
                }}
                style={{
                  background: "var(--bg3)",
                  border: copyRecordModal.error ? "1px solid #ff5555" : "1px solid var(--border)",
                  borderRadius: 4,
                  color: "var(--text)",
                  padding: "4px 8px",
                  fontSize: 13,
                  fontFamily: "monospace",
                  outline: "none",
                }}
                placeholder="new_key"
              />
            </label>
            {copyRecordModal.error && (
              <div style={{ color: "#ff5555", fontSize: 12 }}>{copyRecordModal.error}</div>
            )}
            <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
              <button onClick={() => setCopyRecordModal(null)}>取消</button>
              <button
                className="primary"
                onClick={handleCopyRecordCommit}
                disabled={!copyRecordModal.dstFile || !copyRecordModal.newKey.trim()}
              >
                复制
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
