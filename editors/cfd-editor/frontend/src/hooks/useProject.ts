import { useState, useCallback, useRef } from "react";
import { api } from "../api";
import type { ProjectSnapshot, FileRecords, DiagnosticItem } from "../bindings";

export function useProject() {
  const [snapshot, setSnapshot] = useState<ProjectSnapshot | null>(null);
  const [fileRecords, setFileRecords] = useState<FileRecords | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);
  // Use ref for timer to avoid stale-closure race in markDirty
  const dirtyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const dirtyFileRef = useRef<string | null>(null);
  const yamlPathRef = useRef<string | null>(null);

  // Separate ref to track the current session id for cleanup without reading state in async callbacks
  const sessionIdRef = useRef<number | null>(null);

  const loadProject = useCallback(async (yamlPath: string) => {
    setLoading(true);
    setError(null);
    yamlPathRef.current = yamlPath;
    // Close old session before creating a new one
    if (sessionIdRef.current != null) {
      api.closeSession(sessionIdRef.current).catch(() => {});
      sessionIdRef.current = null;
    }
    try {
      const snap = await api.loadProject(yamlPath);
      sessionIdRef.current = snap.session_id;
      setSnapshot(snap);
      setFileRecords(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  const refreshSnapshot = useCallback(async () => {
    if (!yamlPathRef.current) return;
    if (sessionIdRef.current != null) {
      api.closeSession(sessionIdRef.current).catch(() => {});
      sessionIdRef.current = null;
    }
    try {
      const snap = await api.loadProject(yamlPathRef.current);
      sessionIdRef.current = snap.session_id;
      setSnapshot(snap);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const loadFile = useCallback(async (sessionId: number, filePath: string) => {
    try {
      const records = await api.getFileRecords(sessionId, filePath);
      setFileRecords(records);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  // Re-fetch diagnostics from the current in-memory model and update snapshot
  const refreshDiagnostics = useCallback(async (sessionId: number) => {
    try {
      const diags: DiagnosticItem[] = await api.getDiagnostics(sessionId);
      setSnapshot(prev => prev && prev.session_id === sessionId
        ? { ...prev, diagnostics: diags }
        : prev
      );
    } catch {
      // Non-fatal: diagnostics panel will just be stale
    }
  }, []);

  const markDirty = useCallback((sessionId: number, filePath: string) => {
    setDirty(true);
    dirtyFileRef.current = filePath;
    if (dirtyTimerRef.current) clearTimeout(dirtyTimerRef.current);
    dirtyTimerRef.current = setTimeout(async () => {
      dirtyTimerRef.current = null;
      setDirty(false);
      // Reload file records and refresh diagnostics in parallel
      const reloadFile = dirtyFileRef.current === filePath
        ? loadFile(sessionId, filePath)
        : Promise.resolve();
      await Promise.all([reloadFile, refreshDiagnostics(sessionId)]);
    }, 1000);
  }, [loadFile, refreshDiagnostics]);

  const saveNow = useCallback(async (sessionId: number, filePath: string) => {
    if (dirtyTimerRef.current) {
      clearTimeout(dirtyTimerRef.current);
      dirtyTimerRef.current = null;
    }
    setDirty(false);
    try {
      await Promise.all([loadFile(sessionId, filePath), refreshDiagnostics(sessionId)]);
    } catch (e) {
      setError(String(e));
    }
  }, [loadFile, refreshDiagnostics]);

  return { snapshot, fileRecords, loading, error, dirty, loadProject, refreshSnapshot, loadFile, markDirty, saveNow };
}
