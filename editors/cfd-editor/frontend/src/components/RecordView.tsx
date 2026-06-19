import { useState, useCallback, useEffect, useMemo, useRef } from "react";
import type { FileRecords, FieldPathSegment, FieldValue, FieldCell, RecordRow } from "../bindings";
import type { Route } from "../router";
import { DataCard } from "./DataCard";
import { ContextMenu, type ContextMenuState } from "./ContextMenu";
import { api } from "../api";

interface RecordViewProps {
  sessionId: number;
  filePath: string;
  recordKey: string;
  fileRecords: FileRecords | null;
  onWriteField: (
    sessionId: number,
    filePath: string,
    recordKey: string,
    fieldPath: FieldPathSegment[],
    newValue: FieldValue
  ) => Promise<void>;
  onRenameRecord?: (oldKey: string, newKey: string) => Promise<void>;
  onNavigate: (route: Route) => void;
}

export function RecordView({
  sessionId,
  filePath,
  recordKey,
  fileRecords,
  onWriteField,
  onRenameRecord,
  onNavigate,
}: RecordViewProps) {
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  const [fetchedRecord, setFetchedRecord] = useState<RecordRow | null>(null);
  const [fetchError, setFetchError] = useState<string | null>(null);
  const [typeFilter, setTypeFilter] = useState<string | null>(null);
  const [editingKey, setEditingKey] = useState(false);
  const [keyText, setKeyText] = useState(recordKey);
  const keyInputRef = useRef<HTMLInputElement>(null);

  // Sync keyText when recordKey prop changes (e.g. navigation)
  useEffect(() => { setKeyText(recordKey); setEditingKey(false); }, [recordKey]);

  const recordFromFile = fileRecords?.records.find(r => r.key === recordKey) ?? null;
  const record = recordFromFile ?? fetchedRecord;
  const allRecords = fileRecords?.records ?? [];

  // All unique type names in current file, sorted
  const typeNames = useMemo(() => {
    const seen = new Set<string>();
    for (const r of allRecords) seen.add(r.actual_type);
    return Array.from(seen).sort();
  }, [allRecords]);

  // Reset type filter when file changes
  useEffect(() => { setTypeFilter(null); }, [filePath]);

  const filteredRecords = typeFilter
    ? allRecords.filter(r => r.actual_type === typeFilter || r.key === recordKey)
    : allRecords;

  // Keyboard navigation: up/down arrows move through filteredRecords
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
      // Only if focus is not inside an input/textarea
      const tag = (e.target as HTMLElement).tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
      const idx = filteredRecords.findIndex(r => r.key === recordKey);
      if (idx === -1) return;
      const next = e.key === "ArrowUp" ? idx - 1 : idx + 1;
      if (next >= 0 && next < filteredRecords.length) {
        e.preventDefault();
        onNavigate({ view: "record", file: filePath, recordKey: filteredRecords[next].key });
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [filteredRecords, recordKey, filePath, onNavigate]);

  // If fileRecords hasn't loaded yet for this key, fetch directly
  useEffect(() => {
    if (recordFromFile) {
      setFetchedRecord(null);
      setFetchError(null);
      return;
    }
    setFetchError(null);
    api.getRecord(sessionId, filePath, recordKey)
      .then(r => { setFetchedRecord(r); setFetchError(null); })
      .catch(e => { setFetchedRecord(null); setFetchError(String(e)); });
  }, [sessionId, filePath, recordKey, recordFromFile]);

  const handleKeyRename = useCallback(async () => {
    const trimmed = keyText.trim();
    setEditingKey(false);
    if (!trimmed || trimmed === recordKey) { setKeyText(recordKey); return; }
    if (onRenameRecord) {
      try {
        await onRenameRecord(recordKey, trimmed);
        // Navigation to the new key happens in App via onRenameRecord handler
      } catch (err) {
        setKeyText(recordKey); // revert on error
      }
    }
  }, [keyText, recordKey, onRenameRecord]);

  const handleFieldEdit = useCallback(async (field: FieldCell, newValue: FieldValue) => {
    await onWriteField(sessionId, filePath, recordKey, [{ kind: "Field", name: field.name }], newValue);
  }, [sessionId, filePath, recordKey, onWriteField]);

  const handleFieldContextMenu = useCallback((e: React.MouseEvent, field: FieldCell) => {
    if (field.value.kind !== "Ref") return;
    e.preventDefault();
    const refValue = field.value;
    setContextMenu({
      x: e.clientX,
      y: e.clientY,
      items: [
        {
          label: "跳转到引用记录",
          onClick: () => {
            const targetFile = refValue.target_file ?? filePath;
            onNavigate({ view: "record", file: targetFile, recordKey: refValue.target_key });
          },
        },
      ],
    });
  }, [filePath, onNavigate]);

  return (
    <div style={{ display: "flex", flex: 1, overflow: "hidden" }}>
      {/* Left sidebar: record list */}
      <div style={{
        width: 200,
        borderRight: "1px solid var(--border)",
        display: "flex",
        flexDirection: "column",
        flexShrink: 0,
        background: "var(--bg2)",
        overflow: "hidden",
      }}>
        <div style={{
          padding: "6px 8px",
          fontSize: 11,
          fontWeight: 600,
          color: "var(--text-muted)",
          textTransform: "uppercase",
          letterSpacing: 1,
          borderBottom: "1px solid var(--border)",
          flexShrink: 0,
        }}>
          Records
        </div>
        {/* Type filter tabs */}
        {typeNames.length > 1 && (
          <div style={{
            display: "flex",
            flexWrap: "wrap",
            gap: 2,
            padding: "4px 6px",
            borderBottom: "1px solid var(--border)",
            flexShrink: 0,
          }}>
            <button
              onClick={() => setTypeFilter(null)}
              style={{
                fontSize: 10,
                padding: "1px 6px",
                background: typeFilter === null ? "var(--bg3)" : "transparent",
                border: typeFilter === null ? "1px solid var(--border)" : "1px solid transparent",
                borderRadius: 3,
                color: typeFilter === null ? "var(--text)" : "var(--text-muted)",
                cursor: "pointer",
              }}
            >
              All
            </button>
            {typeNames.map(t => (
              <button
                key={t}
                onClick={() => setTypeFilter(typeFilter === t ? null : t)}
                style={{
                  fontSize: 10,
                  padding: "1px 6px",
                  background: typeFilter === t ? "var(--bg3)" : "transparent",
                  border: typeFilter === t ? "1px solid var(--border)" : "1px solid transparent",
                  borderRadius: 3,
                  color: typeFilter === t ? "var(--text)" : "var(--text-muted)",
                  cursor: "pointer",
                }}
              >
                {t}
              </button>
            ))}
          </div>
        )}
        <div style={{ flex: 1, overflowY: "auto" }}>
          {filteredRecords.map(r => (
            <div
              key={r.key}
              onClick={() => onNavigate({ view: "record", file: filePath, recordKey: r.key })}
              style={{
                padding: "4px 10px",
                cursor: "pointer",
                background: r.key === recordKey ? "var(--bg3)" : "transparent",
                borderLeft: r.key === recordKey ? "2px solid var(--accent)" : "2px solid transparent",
                fontSize: 12,
              }}
              onMouseEnter={e => { if (r.key !== recordKey) e.currentTarget.style.background = "var(--bg3)"; }}
              onMouseLeave={e => { if (r.key !== recordKey) e.currentTarget.style.background = "transparent"; }}
              title={`${r.key} (${r.actual_type})`}
            >
              <div style={{
                fontFamily: "monospace",
                color: r.key === recordKey ? "var(--text)" : "var(--text-muted)",
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}>
                {r.key}
              </div>
              {typeNames.length > 1 && typeFilter === null && (
                <div style={{ fontSize: 10, color: "var(--accent)", marginTop: 1 }}>{r.actual_type}</div>
              )}
            </div>
          ))}
          {allRecords.length === 0 && (
            <div style={{ padding: "8px 12px", color: "var(--text-muted)", fontSize: 12 }}>No records</div>
          )}
          {allRecords.length > 0 && filteredRecords.length === 0 && (
            <div style={{ padding: "8px 12px", color: "var(--text-muted)", fontSize: 12 }}>No records match filter</div>
          )}
        </div>
      </div>

      {/* Main content */}
      <div style={{ flex: 1, overflow: "auto", padding: 16 }}>
        {record ? (
          <>
            {/* Header */}
            <div style={{ marginBottom: 16, paddingBottom: 12, borderBottom: "1px solid var(--border)" }}>
              {editingKey && onRenameRecord ? (
                <input
                  ref={keyInputRef}
                  value={keyText}
                  onChange={e => setKeyText(e.target.value)}
                  onBlur={handleKeyRename}
                  onKeyDown={e => {
                    if (e.key === "Enter") { e.preventDefault(); handleKeyRename(); }
                    if (e.key === "Escape") { setEditingKey(false); setKeyText(recordKey); }
                    e.stopPropagation();
                  }}
                  style={{
                    fontFamily: "monospace",
                    fontSize: 18,
                    fontWeight: 700,
                    background: "var(--bg3)",
                    border: "1px solid var(--accent)",
                    borderRadius: 4,
                    color: "var(--text)",
                    padding: "2px 6px",
                    outline: "none",
                    width: "100%",
                  }}
                  autoFocus
                />
              ) : (
                <div
                  onClick={onRenameRecord ? () => setEditingKey(true) : undefined}
                  title={onRenameRecord ? "Click to rename record key" : undefined}
                  style={{
                    fontFamily: "monospace",
                    fontSize: 18,
                    fontWeight: 700,
                    color: "var(--text)",
                    cursor: onRenameRecord ? "pointer" : "default",
                    borderBottom: onRenameRecord ? "1px dashed var(--border)" : "none",
                    display: "inline-block",
                  }}
                >
                  {record.key}
                  {onRenameRecord && (
                    <span style={{ marginLeft: 6, color: "var(--text-muted)", fontSize: 12, opacity: 0.5 }}>✎</span>
                  )}
                </div>
              )}
              <div style={{ color: "var(--text-muted)", fontSize: 12, marginTop: 4 }}>
                {record.actual_type}
              </div>
            </div>

            {/* Fields */}
            <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
              {record.fields.map(field => (
                <div
                  key={field.name}
                  onContextMenu={e => handleFieldContextMenu(e, field)}
                  style={{
                    display: "flex",
                    alignItems: "flex-start",
                    gap: 8,
                    padding: "4px 8px",
                    borderRadius: 4,
                  }}
                  onMouseEnter={e => (e.currentTarget.style.background = "var(--bg3)")}
                  onMouseLeave={e => (e.currentTarget.style.background = "transparent")}
                >
                  <span style={{
                    minWidth: 140,
                    color: "var(--text-muted)",
                    fontSize: 12,
                    fontFamily: "monospace",
                    paddingTop: 3,
                    flexShrink: 0,
                  }}>
                    {field.name}
                  </span>
                  <div style={{ flex: 1 }}>
                    <DataCard
                      mode="expanded"
                      value={field.value}
                      depth={0}
                      label={undefined}
                      onEdit={(nv) => handleFieldEdit(field, nv)}
                    />
                  </div>
                </div>
              ))}
              {record.fields.length === 0 && (
                <div style={{ color: "var(--text-muted)", fontSize: 12, padding: 8 }}>No fields</div>
              )}
            </div>
          </>
        ) : fetchError ? (
          <div style={{ color: "var(--error)", padding: 16, fontSize: 13 }}>
            Failed to load <code style={{ fontFamily: "monospace" }}>{recordKey}</code>: {fetchError}
          </div>
        ) : !fileRecords ? (
          <div style={{ color: "var(--text-muted)", padding: 16, fontSize: 13 }}>Loading…</div>
        ) : (
          <div style={{ color: "var(--text-muted)", padding: 16, fontSize: 13 }}>
            Record <code style={{ fontFamily: "monospace" }}>{recordKey}</code> not found in this file.
          </div>
        )}
      </div>

      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          items={contextMenu.items}
          onClose={() => setContextMenu(null)}
        />
      )}
    </div>
  );
}
