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
  onDeleteRecord?: (sessionId: number, filePath: string, recordKey: string) => Promise<void>;
  onDuplicateRecord?: (sessionId: number, filePath: string, srcKey: string, newKey: string) => Promise<void>;
  onNavigate: (route: Route) => void;
  onError?: (msg: string) => void;
}

interface DuplicateModal { srcKey: string; draft: string; error: string | null }
interface DeleteModal { recordKey: string }

export function RecordView({
  sessionId,
  filePath,
  recordKey,
  fileRecords,
  onWriteField,
  onRenameRecord,
  onDeleteRecord,
  onDuplicateRecord,
  onNavigate,
  onError,
}: RecordViewProps) {
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  const [fetchedRecord, setFetchedRecord] = useState<RecordRow | null>(null);
  const [fetchError, setFetchError] = useState<string | null>(null);
  const [typeFilter, setTypeFilter] = useState<string | null>(null);
  const [sidebarSearch, setSidebarSearch] = useState("");
  const [editingKey, setEditingKey] = useState(false);
  const [keyText, setKeyText] = useState(recordKey);
  const [duplicateModal, setDuplicateModal] = useState<DuplicateModal | null>(null);
  const [deleteModal, setDeleteModal] = useState<DeleteModal | null>(null);
  const [fieldSearch, setFieldSearch] = useState("");
  const keyInputRef = useRef<HTMLInputElement>(null);
  const sidebarSearchRef = useRef<HTMLInputElement>(null);
  const fieldSearchRef = useRef<HTMLInputElement>(null);
  const selectedItemRef = useRef<HTMLDivElement>(null);
  // Set to a key to trigger rename-edit mode after that key becomes the active record
  const pendingRenameKeyRef = useRef<string | null>(null);

  // Sync keyText when recordKey prop changes; trigger pending rename-edit if requested
  useEffect(() => {
    setKeyText(recordKey);
    if (pendingRenameKeyRef.current === recordKey) {
      pendingRenameKeyRef.current = null;
      setEditingKey(true);
    } else {
      setEditingKey(false);
    }
  }, [recordKey]);

  // Focus the key input when editingKey becomes true
  useEffect(() => {
    if (editingKey) {
      requestAnimationFrame(() => {
        keyInputRef.current?.focus();
        keyInputRef.current?.select();
      });
    }
  }, [editingKey]);

  const recordFromFile = fileRecords?.records.find(r => r.key === recordKey) ?? null;
  const record = recordFromFile ?? fetchedRecord;
  const allRecords = fileRecords?.records ?? [];

  // All unique type names in current file, sorted
  const typeNames = useMemo(() => {
    const seen = new Set<string>();
    for (const r of allRecords) seen.add(r.actual_type);
    return Array.from(seen).sort();
  }, [allRecords]);

  // Reset type filter and search when file changes; reset field search when record changes
  useEffect(() => { setTypeFilter(null); setSidebarSearch(""); }, [filePath]);
  useEffect(() => { setFieldSearch(""); }, [recordKey]);

  const filteredRecords = allRecords
    .filter(r => {
      if (typeFilter && r.actual_type !== typeFilter && r.key !== recordKey) return false;
      if (sidebarSearch) {
        const q = sidebarSearch.toLowerCase();
        return r.key.toLowerCase().includes(q) || r.actual_type.toLowerCase().includes(q);
      }
      return true;
    })
    .slice()
    .sort((a, b) => a.key.localeCompare(b.key));

  // Scroll selected item into view when recordKey changes
  useEffect(() => {
    selectedItemRef.current?.scrollIntoView({ block: "nearest" });
  }, [recordKey]);

  // Keyboard navigation + sidebar search shortcut
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      // Ctrl+F focuses field search (main content); Ctrl+Shift+F focuses sidebar search
      if ((e.ctrlKey || e.metaKey) && e.key === "f") {
        e.preventDefault();
        if (e.shiftKey) {
          sidebarSearchRef.current?.focus();
          sidebarSearchRef.current?.select();
        } else {
          fieldSearchRef.current?.focus();
          fieldSearchRef.current?.select();
        }
        return;
      }
      // Ctrl+N navigates to table view (to create a new record of the same type)
      if ((e.ctrlKey || e.metaKey) && e.key === "n") {
        e.preventDefault();
        const currentType = record?.actual_type;
        onNavigate({ view: "table", file: filePath, ...(currentType ? { typeFilter: currentType } : {}) });
        return;
      }
      // Only if focus is not inside an input/textarea
      const tag = (e.target as HTMLElement).tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;

      if (e.key === "ArrowUp" || e.key === "ArrowDown") {
        const idx = filteredRecords.findIndex(r => r.key === recordKey);
        if (idx === -1) return;
        const next = e.key === "ArrowUp" ? idx - 1 : idx + 1;
        if (next >= 0 && next < filteredRecords.length) {
          e.preventDefault();
          onNavigate({ view: "record", file: filePath, recordKey: filteredRecords[next].key });
        }
      } else if ((e.key === "Delete" || e.key === "Backspace") && onDeleteRecord) {
        e.preventDefault();
        setDeleteModal({ recordKey });
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [filteredRecords, recordKey, filePath, record?.actual_type, onNavigate, onDeleteRecord, sessionId]);

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

  const handleDuplicateCommit = useCallback(async () => {
    if (!duplicateModal || !onDuplicateRecord) return;
    const newKey = duplicateModal.draft.trim();
    if (!newKey) { setDuplicateModal(m => m && ({ ...m, error: "Key cannot be empty" })); return; }
    if (newKey === duplicateModal.srcKey) { setDuplicateModal(null); return; }
    try {
      await onDuplicateRecord(sessionId, filePath, duplicateModal.srcKey, newKey);
      setDuplicateModal(null);
    } catch (e) {
      setDuplicateModal(m => m && ({ ...m, error: String(e) }));
    }
  }, [duplicateModal, onDuplicateRecord, sessionId, filePath]);

  const handleDeleteCommit = useCallback(async () => {
    if (!deleteModal || !onDeleteRecord) return;
    try {
      await onDeleteRecord(sessionId, filePath, deleteModal.recordKey);
      setDeleteModal(null);
    } catch (e) {
      onError?.(`Delete failed: ${e}`);
      setDeleteModal(null);
    }
  }, [deleteModal, onDeleteRecord, sessionId, filePath, onError]);

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
        {/* Sidebar search */}
        <div style={{ padding: "4px 6px", borderBottom: "1px solid var(--border)", flexShrink: 0 }}>
          <input
            ref={sidebarSearchRef}
            value={sidebarSearch}
            onChange={e => setSidebarSearch(e.target.value)}
            onKeyDown={e => {
              if (e.key === "Escape") { setSidebarSearch(""); e.stopPropagation(); }
              e.stopPropagation();
            }}
            placeholder="Filter records…"
            style={{
              width: "100%",
              background: "var(--bg3)",
              border: "1px solid var(--border)",
              borderRadius: 3,
              color: "var(--text)",
              padding: "2px 6px",
              fontSize: 11,
              outline: "none",
              boxSizing: "border-box",
            }}
          />
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
                <span style={{ marginLeft: 3, opacity: 0.7 }}>
                  ({allRecords.filter(r => r.actual_type === t).length})
                </span>
              </button>
            ))}
          </div>
        )}
        <div style={{ flex: 1, overflowY: "auto" }}>
          {filteredRecords.map(r => (
            <div
              key={r.key}
              ref={r.key === recordKey ? selectedItemRef : undefined}
              onClick={() => onNavigate({ view: "record", file: filePath, recordKey: r.key })}
              onContextMenu={e => {
                e.preventDefault();
                const items: { label: string; danger?: boolean; onClick: () => void }[] = [
                  { label: "复制 Key", onClick: () => navigator.clipboard.writeText(r.key).catch(() => {}) },
                ];
                if (onRenameRecord) items.push({
                  label: "重命名记录 Key",
                  onClick: () => {
                    if (r.key === recordKey) {
                      setEditingKey(true);
                    } else {
                      pendingRenameKeyRef.current = r.key;
                      onNavigate({ view: "record", file: filePath, recordKey: r.key });
                    }
                  },
                });
                if (onDuplicateRecord) items.push({
                  label: "复制记录",
                  onClick: () => setDuplicateModal({ srcKey: r.key, draft: `${r.key}_copy`, error: null }),
                });
                if (onDeleteRecord) items.push({
                  label: "删除记录",
                  danger: true,
                  onClick: () => setDeleteModal({ recordKey: r.key }),
                });
                setContextMenu({ x: e.clientX, y: e.clientY, items });
              }}
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
            <div style={{ padding: "8px 12px", color: "var(--text-muted)", fontSize: 12 }}>
              {sidebarSearch ? `No matches for "${sidebarSearch}"` : "No records match filter"}
            </div>
          )}
          {sidebarSearch && filteredRecords.length > 0 && (
            <div style={{ padding: "2px 10px 4px", color: "var(--text-muted)", fontSize: 10 }}>
              {filteredRecords.length} / {allRecords.length}
            </div>
          )}
        </div>
        {/* New record button */}
        <div style={{ borderTop: "1px solid var(--border)", padding: 6, flexShrink: 0 }}>
          <button
            onClick={() => {
              const currentType = record?.actual_type;
              onNavigate({ view: "table", file: filePath, ...(currentType ? { typeFilter: currentType } : {}) });
            }}
            title="Go to table view to create a record (Ctrl+N)"
            style={{ width: "100%", fontSize: 11, justifyContent: "flex-start" }}
          >
            ＋ New record…
          </button>
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
                  onContextMenu={e => {
                    e.preventDefault();
                    const items: { label: string; danger?: boolean; onClick: () => void }[] = [
                      { label: "复制 Key", onClick: () => navigator.clipboard.writeText(recordKey).catch(() => {}) },
                    ];
                    if (onRenameRecord) items.push({
                      label: "重命名记录 Key",
                      onClick: () => setEditingKey(true),
                    });
                    if (onDuplicateRecord) items.push({
                      label: "复制记录",
                      onClick: () => setDuplicateModal({ srcKey: recordKey, draft: `${recordKey}_copy`, error: null }),
                    });
                    if (onDeleteRecord) items.push({
                      label: "删除记录",
                      danger: true,
                      onClick: () => setDeleteModal({ recordKey }),
                    });
                    setContextMenu({ x: e.clientX, y: e.clientY, items });
                  }}
                  title={onRenameRecord ? "Click to rename · Right-click for options" : "Right-click for options"}
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
              <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 4 }}>
                <span style={{ color: "var(--text-muted)", fontSize: 12 }}>
                  {record.actual_type}
                </span>
                {onDeleteRecord && (
                  <button
                    onClick={() => setDeleteModal({ recordKey })}
                    title="Delete this record"
                    style={{
                      fontSize: 11,
                      padding: "1px 8px",
                      background: "transparent",
                      border: "1px solid #ff555566",
                      borderRadius: 3,
                      color: "#ff5555",
                      cursor: "pointer",
                    }}
                  >
                    Delete
                  </button>
                )}
              </div>
              {/* Spread sources list */}
              {record.spread_sources.length > 0 && (
                <div style={{ display: "flex", flexWrap: "wrap", gap: 4, marginTop: 6 }}>
                  <span style={{ color: "var(--text-muted)", fontSize: 11 }}>spreads from:</span>
                  {record.spread_sources.map(src => (
                    <span
                      key={src.key}
                      onClick={() => onNavigate({ view: "record", file: src.file || filePath, recordKey: src.key })}
                      title={`跳转到 spread 源记录 ${src.key}${src.file && src.file !== filePath ? ` (${src.file})` : ""}`}
                      style={{
                        color: "var(--accent)",
                        fontSize: 11,
                        fontFamily: "monospace",
                        cursor: "pointer",
                        textDecoration: "underline",
                        textDecorationStyle: "dotted",
                      }}
                    >
                      {src.key}
                    </span>
                  ))}
                </div>
              )}
            </div>

            {/* Field search */}
            {record.fields.length > 6 && (
              <div style={{ marginBottom: 8 }}>
                <input
                  ref={fieldSearchRef}
                  value={fieldSearch}
                  onChange={e => setFieldSearch(e.target.value)}
                  onKeyDown={e => {
                    if (e.key === "Escape") { setFieldSearch(""); e.stopPropagation(); }
                    e.stopPropagation();
                  }}
                  placeholder="Filter fields… (Ctrl+F)"
                  style={{
                    width: "100%",
                    background: "var(--bg3)",
                    border: "1px solid var(--border)",
                    borderRadius: 4,
                    color: "var(--text)",
                    padding: "3px 8px",
                    fontSize: 12,
                    outline: "none",
                    boxSizing: "border-box",
                  }}
                />
              </div>
            )}

            {/* Fields */}
            <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
              {record.fields
                .filter(field => !fieldSearch || field.name.toLowerCase().includes(fieldSearch.toLowerCase()))
                .map(field => {
                const isSpread = record.spread_fields.includes(field.name);
                const spreadNavTarget = isSpread && record.spread_sources.length === 1
                  ? record.spread_sources[0]
                  : null;
                const spreadNavFile = spreadNavTarget?.file || filePath;
                const handleSpreadNavClick = isSpread
                  ? spreadNavTarget
                    ? () => onNavigate({ view: "record", file: spreadNavFile, recordKey: spreadNavTarget.key })
                    : record.spread_sources.length > 1
                      ? (e: React.MouseEvent) => {
                          e.stopPropagation();
                          setContextMenu({
                            x: e.clientX,
                            y: e.clientY,
                            items: record.spread_sources.map(src => ({
                              label: `跳转到 ${src.key}${src.file && src.file !== filePath ? ` (${src.file})` : ""}`,
                              onClick: () => onNavigate({ view: "record", file: src.file || filePath, recordKey: src.key }),
                            })),
                          });
                        }
                      : undefined
                  : undefined;
                return (
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
                  <span
                    title={isSpread ? "来自 spread — 请前往源记录编辑" : undefined}
                    style={{
                      minWidth: 140,
                      color: isSpread ? "var(--text-muted)" : "var(--text-muted)",
                      fontSize: 12,
                      fontFamily: "monospace",
                      paddingTop: 3,
                      flexShrink: 0,
                      opacity: isSpread ? 0.6 : 1,
                    }}>
                    {field.name}
                    {isSpread && (
                      <span
                        onClick={handleSpreadNavClick}
                        title={spreadNavTarget
                          ? `跳转到源记录 ${spreadNavTarget.key}`
                          : record.spread_sources.length > 1
                            ? "来自多个 spread — 点击选择源记录"
                            : "来自 spread — 前往源记录编辑"}
                        style={{
                          marginLeft: 4,
                          fontSize: 10,
                          color: "var(--accent)",
                          opacity: 0.7,
                          cursor: handleSpreadNavClick ? "pointer" : "default",
                        }}
                      >↗</span>
                    )}
                  </span>
                  <div style={{ flex: 1 }}>
                    <DataCard
                      mode="expanded"
                      value={field.value}
                      depth={0}
                      sessionId={sessionId}
                      label={undefined}
                      onEdit={isSpread ? undefined : (nv) => handleFieldEdit(field, nv)}
                      onRefClick={(targetFile, targetKey) =>
                        onNavigate({ view: "record", file: targetFile ?? filePath, recordKey: targetKey })
                      }
                    />
                  </div>
                </div>
                );
              })}
              {record.fields.length === 0 && (
                <div style={{ color: "var(--text-muted)", fontSize: 12, padding: 8 }}>No fields</div>
              )}
              {record.fields.length > 0 && fieldSearch &&
                record.fields.filter(f => f.name.toLowerCase().includes(fieldSearch.toLowerCase())).length === 0 && (
                <div style={{ color: "var(--text-muted)", fontSize: 12, padding: 8 }}>
                  No fields match "{fieldSearch}"
                </div>
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

      {/* Duplicate record modal */}
      {duplicateModal && (
        <div
          style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,0.6)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 2000 }}
          onClick={() => setDuplicateModal(null)}
        >
          <div
            style={{ background: "var(--bg2)", border: "1px solid var(--border)", borderRadius: 8, padding: 24, width: 360, display: "flex", flexDirection: "column", gap: 12 }}
            onClick={e => e.stopPropagation()}
          >
            <h3 style={{ margin: 0, fontSize: 15 }}>复制记录</h3>
            <div style={{ fontSize: 12, color: "var(--text-muted)" }}>
              源记录: <code style={{ fontFamily: "monospace" }}>{duplicateModal.srcKey}</code>
            </div>
            <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 13 }}>
              新 Key
              <input
                value={duplicateModal.draft}
                onChange={e => setDuplicateModal(m => m && ({ ...m, draft: e.target.value, error: null }))}
                onKeyDown={e => {
                  if (e.key === "Enter") { e.preventDefault(); handleDuplicateCommit(); }
                  if (e.key === "Escape") setDuplicateModal(null);
                  e.stopPropagation();
                }}
                style={{
                  background: "var(--bg3)",
                  border: duplicateModal.error ? "1px solid #ff5555" : "1px solid var(--border)",
                  borderRadius: 4, color: "var(--text)", padding: "4px 8px", fontSize: 13, fontFamily: "monospace", outline: "none",
                }}
                autoFocus
              />
              {duplicateModal.error && <span style={{ color: "#ff5555", fontSize: 11 }}>{duplicateModal.error}</span>}
            </label>
            <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
              <button onClick={() => setDuplicateModal(null)}>取消</button>
              <button className="primary" onClick={handleDuplicateCommit} disabled={!duplicateModal.draft.trim()}>复制</button>
            </div>
          </div>
        </div>
      )}

      {/* Delete confirmation modal */}
      {deleteModal && (
        <div
          style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,0.6)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 2000 }}
          onClick={() => setDeleteModal(null)}
        >
          <div
            style={{ background: "var(--bg2)", border: "1px solid var(--border)", borderRadius: 8, padding: 24, width: 360, display: "flex", flexDirection: "column", gap: 12 }}
            onClick={e => e.stopPropagation()}
          >
            <h3 style={{ margin: 0, fontSize: 15 }}>删除记录</h3>
            <div style={{ fontSize: 13, color: "var(--text)" }}>
              确认删除记录 <code style={{ fontFamily: "monospace" }}>{deleteModal.recordKey}</code>？此操作不可撤销。
            </div>
            <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
              <button onClick={() => setDeleteModal(null)}>取消</button>
              <button
                onClick={handleDeleteCommit}
                style={{ background: "#ff5555", color: "#fff", border: "none", borderRadius: 4, padding: "4px 16px", cursor: "pointer", fontSize: 13 }}
              >删除</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
