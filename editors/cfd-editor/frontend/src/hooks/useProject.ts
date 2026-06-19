import { useState, useCallback, useRef } from "react";
import { api } from "../api";
import type { ProjectSnapshot, FileRecords } from "../bindings";

export function useProject() {
  const [snapshot, setSnapshot] = useState<ProjectSnapshot | null>(null);
  const [fileRecords, setFileRecords] = useState<FileRecords | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);
  const [dirtyTimer, setDirtyTimer] = useState<ReturnType<typeof setTimeout> | null>(null);
  const yamlPathRef = useRef<string | null>(null);

  const loadProject = useCallback(async (yamlPath: string) => {
    setLoading(true);
    setError(null);
    yamlPathRef.current = yamlPath;
    try {
      const snap = await api.loadProject(yamlPath);
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
    try {
      const snap = await api.loadProject(yamlPathRef.current);
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

  const markDirty = useCallback((sessionId: number, filePath: string) => {
    setDirty(true);
    if (dirtyTimer) clearTimeout(dirtyTimer);
    const t = setTimeout(async () => {
      setDirty(false);
      await loadFile(sessionId, filePath);
    }, 1000);
    setDirtyTimer(t);
  }, [dirtyTimer, loadFile]);

  // Immediately flush the dirty debounce and reload
  const saveNow = useCallback(async (sessionId: number, filePath: string) => {
    if (dirtyTimer) {
      clearTimeout(dirtyTimer);
      setDirtyTimer(null);
    }
    setDirty(false);
    await loadFile(sessionId, filePath);
  }, [dirtyTimer, loadFile]);

  return { snapshot, fileRecords, loading, error, dirty, loadProject, refreshSnapshot, loadFile, markDirty, saveNow };
}
