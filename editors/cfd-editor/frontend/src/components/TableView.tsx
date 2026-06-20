import { useState, useRef, useCallback, useEffect, useMemo } from "react";
import {
  useReactTable,
  getCoreRowModel,
  getSortedRowModel,
  createColumnHelper,
  flexRender,
  type SortingState,
  type ColumnResizeMode,
  type VisibilityState,
  type ColumnSizingState,
} from "@tanstack/react-table";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { FileRecords, RecordRow, FieldValue, FieldPathSegment, FieldSchema, DiagnosticItem } from "../bindings";
import type { Route } from "../router";
import { DataCard } from "./DataCard";
import { ContextMenu, type ContextMenuState } from "./ContextMenu";
import { api } from "../api";

interface TableViewProps {
  fileRecords: FileRecords;
  sessionId: number;
  filePath: string;
  initialTypeFilter?: string;
  onTypeChange?: (typeName: string) => void;
  onWriteField: (
    sessionId: number,
    filePath: string,
    recordKey: string,
    fieldPath: FieldPathSegment[],
    newValue: FieldValue,
    oldValue?: FieldValue
  ) => Promise<void>;
  onDeleteRecord: (sessionId: number, filePath: string, recordKey: string) => Promise<void>;
  onRenameRecord?: (sessionId: number, filePath: string, oldKey: string, newKey: string) => Promise<void>;
  onDuplicateRecord?: (sessionId: number, filePath: string, srcKey: string, newKey: string) => Promise<void>;
  onMoveRecord?: (srcFile: string, recordKey: string) => void;
  onCopyRecord?: (srcFile: string, recordKey: string) => void;
  onSortFile?: () => void;
  onNavigate: (route: Route) => void;
  diagnostics?: DiagnosticItem[];
  onError?: (msg: string) => void;
}

interface NewRecordForm {
  key: string;
  typeName: string;
  error: string | null;
}

interface EditingCell {
  rowKey: string;
  fieldName: string;
  value: FieldValue;
}

interface RenameModal {
  rowKey: string;
  draft: string;
  error: string | null;
}

interface DuplicateModal {
  srcKey: string;
  draft: string;
  error: string | null;
}

interface DeleteModal {
  rowKey: string;
}

interface PasteModal {
  source: string;
  error: string | null;
  importing: boolean;
  importedKeys?: string[];
}

type RowData = RecordRow & { _filePath: string };

function fieldValueToString(v: FieldValue): string {
  switch (v.kind) {
    case "Null": return "null";
    case "Bool": return String(v.v);
    case "Int": return String(v.v);
    case "Float": return String(v.v);
    case "Str": return v.v;
    case "Enum": return v.variant;
    case "Ref": return v.target_key;
    default: return "";
  }
}

function parseFieldValue(raw: string, original: FieldValue): FieldValue {
  const t = raw.trim();
  if (original.kind === "Enum") {
    return { kind: "Enum", enum_name: original.enum_name, variant: t, int_value: original.int_value };
  }
  if (original.kind === "Ref") {
    return { kind: "Ref", target_type: original.target_type, target_key: t, target_file: original.target_file };
  }
  if (t === "null") return { kind: "Null" };
  if (t === "true") return { kind: "Bool", v: true };
  if (t === "false") return { kind: "Bool", v: false };
  if (/^-?\d+$/.test(t)) { const n = Number(t); if (!isNaN(n)) return { kind: "Int", v: n }; }
  if (/^-?\d*\.\d+([eE][+-]?\d+)?$/.test(t)) { const f = parseFloat(t); if (!isNaN(f)) return { kind: "Float", v: f }; }
  return { kind: "Str", v: raw };
}

function fieldValueToJson(v: FieldValue): unknown {
  switch (v.kind) {
    case "Null": return null;
    case "Bool": return v.v;
    case "Int": case "Float": return v.v;
    case "Str": return v.v;
    case "Enum": return v.variant;
    case "Ref": return v.target_key;
    case "Object": { const o: Record<string, unknown> = { _type: v.actual_type }; for (const f of v.fields) o[f.name] = fieldValueToJson(f.value); return o; }
    case "Array": return v.items.map(fieldValueToJson);
    case "Dict": { const o: Record<string, unknown> = {}; for (const e of v.entries) { const k = e.key.kind === "Str" ? e.key.v : e.key.kind === "Int" ? String(e.key.v) : e.key.variant; o[k] = fieldValueToJson(e.value); } return o; }
  }
}

const columnHelper = createColumnHelper<RowData>();

interface CellEditorProps {
  value: FieldValue;
  sessionId: number;
  onCommit: (raw: string) => void;
  onCancel: () => void;
  onTabCommit?: (raw: string) => void;
}

const CELL_STYLE: React.CSSProperties = {
  width: "100%",
  height: "100%",
  padding: "4px 8px",
  background: "var(--bg3)",
  border: "1px solid var(--accent)",
  borderRadius: 0,
  color: "var(--text)",
  fontSize: 12,
  fontFamily: "monospace",
  outline: "none",
  boxSizing: "border-box",
};

function CellEditor({ value, sessionId, onCommit, onCancel, onTabCommit }: CellEditorProps) {
  const [text, setText] = useState(() => fieldValueToString(value));
  const inputRef = useRef<HTMLInputElement>(null);
  const [enumVariants, setEnumVariants] = useState<string[] | null>(null);
  const [refTargets, setRefTargets] = useState<string[]>([]);
  const listId = useRef(`cl-${Math.random().toString(36).slice(2)}`).current;

  useEffect(() => {
    if (value.kind === "Enum") {
      api.getEnumVariants(sessionId, value.enum_name).then(vs => {
        setEnumVariants(vs.length > 0 ? vs : []);
      }).catch(e => { console.error("getEnumVariants failed:", e); setEnumVariants([]); });
    } else if (value.kind === "Ref" && value.target_type) {
      api.getRefTargets(sessionId, value.target_type).then(keys => {
        if (keys.length > 0) setRefTargets(keys);
      }).catch(e => console.error("getRefTargets failed:", e));
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, value.kind === "Enum" ? (value as { enum_name: string }).enum_name : "",
      value.kind === "Ref" ? (value as { target_type: string }).target_type : ""]);

  useEffect(() => {
    if (value.kind !== "Enum") {
      inputRef.current?.focus();
      inputRef.current?.select();
    }
  }, [value.kind]);

  if (value.kind === "Enum") {
    if (enumVariants === null) {
      return (
        <div
          tabIndex={0}
          onKeyDown={e => { if (e.key === "Escape") { e.preventDefault(); onCancel(); } e.stopPropagation(); }}
          style={{ ...CELL_STYLE, color: "var(--text-muted)", fontStyle: "italic" }}
        >
          Loading…
        </div>
      );
    }
    if (enumVariants.length > 0) {
      return (
        <select
          value={text}
          onChange={e => { setText(e.target.value); }}
          onBlur={e => onCommit(e.currentTarget.value)}
          onKeyDown={e => {
            if (e.key === "Enter") { e.preventDefault(); onCommit(e.currentTarget.value); }
            if (e.key === "Escape") { e.preventDefault(); onCancel(); }
            if (e.key === "Tab") { e.preventDefault(); (onTabCommit ?? onCommit)(e.currentTarget.value); }
            e.stopPropagation();
          }}
          autoFocus
          style={CELL_STYLE}
        >
          {enumVariants.map(v => <option key={v} value={v}>{v}</option>)}
        </select>
      );
    }
    // Enum variants failed to load — fall through to text input
  }

  if (value.kind === "Ref") {
    return (
      <>
        {refTargets.length > 0 && (
          <datalist id={listId}>
            {refTargets.map(k => <option key={k} value={k} />)}
          </datalist>
        )}
        <input
          ref={inputRef}
          value={text}
          list={refTargets.length > 0 ? listId : undefined}
          onChange={e => setText(e.target.value)}
          onBlur={() => {
            const t = text.trim();
            if (t && t !== value.target_key) onCommit(text); else onCancel();
          }}
          onKeyDown={e => {
            if (e.key === "Enter") { e.preventDefault(); if (text.trim()) onCommit(text); else onCancel(); }
            if (e.key === "Escape") { e.preventDefault(); onCancel(); }
            if (e.key === "Tab") { e.preventDefault(); if (text.trim()) (onTabCommit ?? onCommit)(text); else onCancel(); }
            e.stopPropagation();
          }}
          style={CELL_STYLE}
          placeholder="record_key"
        />
      </>
    );
  }

  return (
    <input
      ref={inputRef}
      value={text}
      onChange={e => setText(e.target.value)}
      onBlur={() => onCommit(text)}
      onKeyDown={e => {
        if (e.key === "Enter") { e.preventDefault(); onCommit(text); }
        if (e.key === "Escape") { e.preventDefault(); onCancel(); }
        if (e.key === "Tab") { e.preventDefault(); (onTabCommit ?? onCommit)(text); }
        e.stopPropagation();
      }}
      style={CELL_STYLE}
    />
  );
}

export function TableView({
  fileRecords,
  sessionId,
  filePath,
  initialTypeFilter,
  onTypeChange,
  onWriteField,
  onDeleteRecord,
  onRenameRecord,
  onDuplicateRecord,
  onMoveRecord,
  onCopyRecord,
  onSortFile,
  onNavigate,
  diagnostics,
  onError,
}: TableViewProps) {
  const [activeType, setActiveType] = useState<string>(
    initialTypeFilter && fileRecords.type_names.includes(initialTypeFilter)
      ? initialTypeFilter
      : fileRecords.type_names[0] ?? ""
  );
  const [sorting, setSorting] = useState<SortingState>([]);
  const [search, setSearch] = useState("");
  const [editingCell, setEditingCell] = useState<EditingCell | null>(null);
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  const [renameModal, setRenameModal] = useState<RenameModal | null>(null);
  const [duplicateModal, setDuplicateModal] = useState<DuplicateModal | null>(null);
  const [deleteModal, setDeleteModal] = useState<DeleteModal | null>(null);
  const [pasteModal, setPasteModal] = useState<PasteModal | null>(null);
  const [allTypeNames, setAllTypeNames] = useState<string[]>(fileRecords.type_names);

  // Keep activeType valid when the type list changes after reload
  useEffect(() => {
    if (fileRecords.type_names.length > 0 && !fileRecords.type_names.includes(activeType)) {
      setActiveType(fileRecords.type_names[0]);
    }
  }, [fileRecords.type_names, activeType]);

  // Sync activeType when the external typeFilter prop changes (e.g., navigated from RecordView)
  useEffect(() => {
    if (initialTypeFilter && fileRecords.type_names.includes(initialTypeFilter)) {
      setActiveType(initialTypeFilter);
    }
  // Only sync when initialTypeFilter changes, not on every render
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [initialTypeFilter]);

  // Load all schema type names for the new-record modal
  useEffect(() => {
    api.getAllTypeNames(sessionId).then(names => {
      const resolved = names.length > 0 ? names : fileRecords.type_names;
      setAllTypeNames(resolved);
      // Seed typeName: prefer activeType, then first resolved type
      setNewRecord(r => r.typeName ? r : { ...r, typeName: activeType ?? resolved[0] ?? "" });
    }).catch(() => {
      setAllTypeNames(fileRecords.type_names);
    });
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);

  const [fieldSchemas, setFieldSchemas] = useState<FieldSchema[]>([]);
  const [columnVisibility, setColumnVisibility] = useState<VisibilityState>(() => {
    if (!initialTypeFilter) return {};
    try {
      const stored = localStorage.getItem(`cfd-col-vis:${initialTypeFilter}`);
      return stored ? JSON.parse(stored) : {};
    } catch { return {}; }
  });
  const [columnSizing, setColumnSizing] = useState<ColumnSizingState>(() => {
    if (!initialTypeFilter) return {};
    try {
      const stored = localStorage.getItem(`cfd-col-size:${initialTypeFilter}`);
      return stored ? JSON.parse(stored) : {};
    } catch { return {}; }
  });
  const [selectedKeys, setSelectedKeys] = useState<Set<string>>(new Set());
  const [batchField, setBatchField] = useState("");
  const [batchValue, setBatchValue] = useState("");
  const [batchApplying, setBatchApplying] = useState(false);
  const [batchDeleting, setBatchDeleting] = useState(false);
  const [batchDeleteConfirm, setBatchDeleteConfirm] = useState(false);
  const [batchError, setBatchError] = useState<string | null>(null);
  const [batchSuccess, setBatchSuccess] = useState<string | null>(null);
  const batchSuccessTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastSelectedKeyRef = useRef<string | null>(null);
  const filteredRowsRef = useRef<RowData[]>([]);
  const [focusedRowIndex, setFocusedRowIndex] = useState<number | null>(null);
  const [showColumnPicker, setShowColumnPicker] = useState(false);
  const columnPickerRef = useRef<HTMLDivElement>(null);
  const [showRequiredOnly, setShowRequiredOnly] = useState(false);

  // Fetch field schemas for the active type to enable required-null highlighting
  useEffect(() => {
    if (!activeType) { setFieldSchemas([]); return; }
    let cancelled = false;
    api.getFieldSchemas(sessionId, activeType)
      .then(s => { if (!cancelled) setFieldSchemas(s); })
      .catch(() => { if (!cancelled) setFieldSchemas([]); });
    return () => { cancelled = true; };
  }, [sessionId, activeType]);

  // Reset sorting, search, column visibility and selection when type changes
  useEffect(() => {
    setSorting([]);
    setSearch("");
    try {
      const stored = localStorage.getItem(`cfd-col-vis:${activeType}`);
      setColumnVisibility(stored ? JSON.parse(stored) : {});
    } catch { setColumnVisibility({}); }
    try {
      const stored = localStorage.getItem(`cfd-col-size:${activeType}`);
      setColumnSizing(stored ? JSON.parse(stored) : {});
    } catch { setColumnSizing({}); }
    setShowColumnPicker(false);
    setShowRequiredOnly(false);
    setSelectedKeys(new Set());
    setFocusedRowIndex(null);
    setBatchField("");
    setBatchValue("");
    setBatchError(null);
    lastSelectedKeyRef.current = null;
  }, [activeType]);

  // Persist column visibility to localStorage when it changes
  useEffect(() => {
    if (!activeType) return;
    try { localStorage.setItem(`cfd-col-vis:${activeType}`, JSON.stringify(columnVisibility)); } catch { /* ignore */ }
  }, [activeType, columnVisibility]);

  // Persist column sizing to localStorage when it changes
  useEffect(() => {
    if (!activeType || Object.keys(columnSizing).length === 0) return;
    try { localStorage.setItem(`cfd-col-size:${activeType}`, JSON.stringify(columnSizing)); } catch { /* ignore */ }
  }, [activeType, columnSizing]);

  // Close column picker on outside click
  useEffect(() => {
    if (!showColumnPicker) return;
    const handler = (e: MouseEvent) => {
      if (columnPickerRef.current && !columnPickerRef.current.contains(e.target as Node)) {
        setShowColumnPicker(false);
      }
    };
    window.addEventListener("mousedown", handler);
    return () => window.removeEventListener("mousedown", handler);
  }, [showColumnPicker]);
  const [showNewRecord, setShowNewRecord] = useState(false);
  const [newRecord, setNewRecord] = useState<NewRecordForm>({ key: "", typeName: activeType ?? fileRecords.type_names[0] ?? "", error: null });
  const [creating, setCreating] = useState(false);
  const parentRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  const selectedKeysRef = useRef<Set<string>>(new Set<string>());
  selectedKeysRef.current = selectedKeys;
  const onDuplicateRecordRef = useRef(onDuplicateRecord);
  onDuplicateRecordRef.current = onDuplicateRecord;

  // Keyboard shortcuts: Ctrl+N opens new-record modal; Ctrl+D duplicates selected; Ctrl+F focuses search; Escape clears/closes
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "d") {
        const tag = (e.target as HTMLElement).tagName;
        if (tag !== "INPUT" && tag !== "TEXTAREA" && tag !== "SELECT") {
          e.preventDefault();
          const first = filteredRowsRef.current.find(r => selectedKeysRef.current.has(r.key)) ?? filteredRowsRef.current[0];
          if (first && onDuplicateRecordRef.current) {
            setDuplicateModal({ srcKey: first.key, draft: `${first.key}_copy`, error: null });
          }
          return;
        }
      }
      if ((e.ctrlKey || e.metaKey) && e.key === "n") {
        e.preventDefault();
        setNewRecord(r => ({ ...r, typeName: activeType ?? r.typeName }));
        setShowNewRecord(true);
      }
      if ((e.ctrlKey || e.metaKey) && e.key === "a") {
        const tag = (e.target as HTMLElement).tagName;
        if (tag !== "INPUT" && tag !== "TEXTAREA" && tag !== "SELECT") {
          e.preventDefault();
          setSelectedKeys(new Set(filteredRowsRef.current.map(r => r.key)));
        }
      }
      if ((e.ctrlKey || e.metaKey) && e.key === "f") {
        e.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
      }
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === "r") {
        const tag = (e.target as HTMLElement).tagName;
        if (tag !== "INPUT" && tag !== "TEXTAREA" && tag !== "SELECT") {
          e.preventDefault();
          setShowRequiredOnly(v => !v);
        }
      }
      if (e.key === "Escape") {
        if (document.activeElement === searchRef.current) {
          setSearch("");
          searchRef.current?.blur();
        } else {
          setShowNewRecord(false);
        }
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const requiredFieldNames = useMemo(
    () => new Set(fieldSchemas.filter(s => !s.has_default).map(s => s.name)),
    [fieldSchemas],
  );
  const filteredRows: RowData[] = fileRecords.records
    .filter(r => {
      if (r.actual_type !== activeType) return false;
      if (showRequiredOnly) {
        const hasRequiredNull = r.fields.some(f => requiredFieldNames.has(f.name) && f.value.kind === "Null");
        if (!hasRequiredNull) return false;
      }
      if (!search) return true;
      const q = search.toLowerCase();
      // Support field:value syntax to filter by specific field
      const colonIdx = search.indexOf(":");
      if (colonIdx > 0) {
        const fieldName = search.slice(0, colonIdx).trim().toLowerCase();
        const fieldVal = search.slice(colonIdx + 1).trim().toLowerCase();
        if (fieldName === "key") return r.key.toLowerCase().includes(fieldVal);
        const cell = r.fields.find(f => f.name.toLowerCase() === fieldName);
        if (cell) {
          const v = cell.value;
          switch (v.kind) {
            case "Str": return v.v.toLowerCase().includes(fieldVal);
            case "Enum": return v.variant.toLowerCase().includes(fieldVal);
            case "Ref": return v.target_key.toLowerCase().includes(fieldVal);
            case "Int": case "Float": return String(v.v).includes(fieldVal);
            case "Bool": return String(v.v).includes(fieldVal);
            default: return false;
          }
        }
      }
      if (r.key.toLowerCase().includes(q)) return true;
      return r.fields.some(f => {
        const v = f.value;
        switch (v.kind) {
          case "Str": return v.v.toLowerCase().includes(q);
          case "Enum": return v.variant.toLowerCase().includes(q);
          case "Ref": return v.target_key.toLowerCase().includes(q);
          case "Int": case "Float": return String(v.v).includes(q);
          case "Bool": return String(v.v).includes(q);
          default: return false;
        }
      });
    })
    .map(r => ({ ...r, _filePath: filePath }));
  filteredRowsRef.current = filteredRows;

  // Per-record diagnostic counts for row badges
  const rowDiagCounts = useMemo(() => {
    if (!diagnostics) return new Map<string, { errors: number; warnings: number }>();
    const map = new Map<string, { errors: number; warnings: number }>();
    for (const d of diagnostics) {
      if (!d.record_key || d.file_path !== filePath) continue;
      const entry = map.get(d.record_key) ?? { errors: 0, warnings: 0 };
      if (d.severity.toLowerCase() === "error") entry.errors++;
      else if (d.severity.toLowerCase() === "warning") entry.warnings++;
      map.set(d.record_key, entry);
    }
    return map;
  }, [diagnostics, filePath]);

  // Determine columns from union of all field names across all records of the active type.
  // Use insertion order from the first record as the primary order, then append any
  // extra names seen in other records (handles schema changes and heterogeneous records).
  const fieldNames: string[] = (() => {
    const seen = new Set<string>();
    const names: string[] = [];
    for (const r of fileRecords.records) {
      if (r.actual_type !== activeType) continue;
      for (const f of r.fields) {
        if (!seen.has(f.name)) { seen.add(f.name); names.push(f.name); }
      }
    }
    return names;
  })();

  const columns = [
    columnHelper.display({
      id: "__sel__",
      size: 32,
      enableResizing: false,
      enableSorting: false,
      header: () => (
        <input
          type="checkbox"
          title="全选/全不选"
          style={{ margin: 0, cursor: "pointer" }}
          checked={filteredRows.length > 0 && filteredRows.every(r => selectedKeys.has(r.key))}
          onChange={e => {
            if (e.target.checked) {
              setSelectedKeys(new Set(filteredRows.map(r => r.key)));
            } else {
              setSelectedKeys(new Set());
            }
          }}
        />
      ),
      cell: info => {
        const rowKey = info.row.original.key;
        return (
          <input
            type="checkbox"
            style={{ margin: 0, cursor: "pointer" }}
            checked={selectedKeys.has(rowKey)}
            onChange={() => {}}
            onClick={e => {
              e.stopPropagation();
              const keys = filteredRows.map(r => r.key);
              if (e.shiftKey && lastSelectedKeyRef.current) {
                const lastIdx = keys.indexOf(lastSelectedKeyRef.current);
                const thisIdx = keys.indexOf(rowKey);
                if (lastIdx !== -1) {
                  const lo = Math.min(lastIdx, thisIdx);
                  const hi = Math.max(lastIdx, thisIdx);
                  const rangeKeys = keys.slice(lo, hi + 1);
                  setSelectedKeys(prev => {
                    const next = new Set(prev);
                    rangeKeys.forEach(k => next.add(k));
                    return next;
                  });
                  return;
                }
              }
              setSelectedKeys(prev => {
                const next = new Set(prev);
                if (next.has(rowKey)) next.delete(rowKey); else next.add(rowKey);
                return next;
              });
              lastSelectedKeyRef.current = rowKey;
            }}
          />
        );
      },
    }),
    columnHelper.accessor("key", {
      header: "key",
      size: 160,
      enableSorting: true,
      cell: info => {
        const row = info.row.original;
        const diag = rowDiagCounts.get(row.key);
        return (
          <span style={{ display: "flex", alignItems: "center", gap: 4 }}>
            <span style={{ fontFamily: "monospace", fontWeight: 600, fontSize: 12, color: row.is_fallback ? "var(--warning)" : undefined, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
              title={row.is_fallback ? "Model build failed — record may have missing required fields" : row.key}>
              {info.getValue()}
              {row.is_fallback && <span style={{ fontSize: 9, marginLeft: 3, opacity: 0.7 }}>⚠</span>}
            </span>
            {diag && (
              <span style={{ display: "flex", gap: 2, flexShrink: 0 }}>
                {diag.errors > 0 && (
                  <span title={`${diag.errors} error${diag.errors > 1 ? "s" : ""}`} style={{ fontSize: 9, background: "var(--error)", color: "#fff", borderRadius: 8, padding: "0 3px", lineHeight: "14px" }}>{diag.errors}</span>
                )}
                {diag.warnings > 0 && (
                  <span title={`${diag.warnings} warning${diag.warnings > 1 ? "s" : ""}`} style={{ fontSize: 9, background: "var(--warning)", color: "#000", borderRadius: 8, padding: "0 3px", lineHeight: "14px" }}>{diag.warnings}</span>
                )}
              </span>
            )}
          </span>
        );
      },
    }),
    ...fieldNames.map(name =>
      columnHelper.accessor(
        row => row.fields.find(f => f.name === name)?.value ?? { kind: "Null" as const },
        {
          id: name,
          header: name,
          size: 160,
          enableSorting: true,
          sortingFn: (a, b) => {
            const va = a.getValue<FieldValue>(name);
            const vb = b.getValue<FieldValue>(name);
            // Numeric sort for numeric types, lexicographic for everything else
            if ((va.kind === "Int" || va.kind === "Float") && (vb.kind === "Int" || vb.kind === "Float")) {
              return (va.v as number) - (vb.v as number);
            }
            const str = (v: FieldValue): string => {
              switch (v.kind) {
                case "Null": return "";
                case "Bool": return String(v.v);
                case "Int": return String(v.v);
                case "Float": return String(v.v);
                case "Str": return v.v;
                case "Enum": return v.variant;
                case "Ref": return v.target_key;
                default: return v.kind;
              }
            };
            return str(va).localeCompare(str(vb));
          },
          cell: info => {
            const v = info.getValue();
            const schema = fieldSchemas.find(s => s.name === name);
            const isRequiredNull = v.kind === "Null" && !!schema && !schema.has_default;
            return (
              <span
                style={{ color: isRequiredNull ? "var(--warning)" : undefined }}
                title={isRequiredNull ? `Required field — must not be null (${schema!.type_str})` : schema?.type_str}
              >
                <DataCard mode="compact" value={v} />
              </span>
            );
          },
        }
      )
    ),
  ];

  const columnResizeMode: ColumnResizeMode = "onChange";

  const table = useReactTable({
    data: filteredRows,
    columns,
    state: { sorting, columnVisibility, columnSizing },
    onSortingChange: setSorting,
    onColumnVisibilityChange: setColumnVisibility,
    onColumnSizingChange: setColumnSizing,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    columnResizeMode,
    enableColumnResizing: true,
  });

  const rows = table.getRowModel().rows;

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 32,
    overscan: 10,
  });

  const virtualItems = virtualizer.getVirtualItems();
  const totalHeight = virtualizer.getTotalSize();

  const handleRowContextMenu = useCallback((
    e: React.MouseEvent,
    row: RowData
  ) => {
    e.preventDefault();
    setContextMenu({
      x: e.clientX,
      y: e.clientY,
      items: [
        {
          label: "跳转到记录视图",
          onClick: () => onNavigate({ view: "record", file: filePath, recordKey: row.key }),
        },
        {
          label: "在资源管理器中显示",
          onClick: () => api.revealInExplorer(sessionId, filePath).catch(e => onError?.(`无法打开资源管理器: ${e}`)),
        },
        {
          label: "复制 Key",
          onClick: () => navigator.clipboard.writeText(row.key).catch(e => onError?.(`复制失败: ${e}`)),
        },
        {
          label: "复制为 CFD 源码",
          onClick: () => api.getRecordSource(sessionId, filePath, row.key).then(src => navigator.clipboard.writeText(src)).catch(e => onError?.(`复制失败: ${e}`)),
        },
        {
          label: "复制为 JSON",
          onClick: () => {
            const obj: Record<string, unknown> = { _key: row.key, _type: row.actual_type };
            for (const f of row.fields) obj[f.name] = fieldValueToJson(f.value);
            navigator.clipboard.writeText(JSON.stringify(obj, null, 2)).catch(e => onError?.(`复制失败: ${e}`));
          },
        },
        ...(onRenameRecord ? [{
          label: "重命名记录 Key",
          onClick: () => {
            setRenameModal({ rowKey: row.key, draft: row.key, error: null });
          },
        }] : []),
        ...(onDuplicateRecord ? [{
          label: "复制记录",
          onClick: () => setDuplicateModal({ srcKey: row.key, draft: `${row.key}_copy`, error: null }),
        }] : []),
        ...(onMoveRecord ? [{
          label: "移动到文件…",
          onClick: () => onMoveRecord(filePath, row.key),
        }] : []),
        ...(onCopyRecord ? [{
          label: "复制到文件…",
          onClick: () => onCopyRecord(filePath, row.key),
        }] : []),
        {
          label: "删除记录",
          danger: true,
          onClick: () => setDeleteModal({ rowKey: row.key }),
        },
      ],
    });
  }, [filePath, sessionId, onNavigate, onDeleteRecord, onRenameRecord, onDuplicateRecord, onMoveRecord, onCopyRecord]);


  const SCALAR_KINDS = ["Null", "Bool", "Int", "Float", "Str", "Enum", "Ref"];

  const handleCellClick = useCallback((rowKey: string, fieldName: string, value: FieldValue) => {
    if (!SCALAR_KINDS.includes(value.kind)) return;
    // Bool cells toggle directly without opening a text editor
    if (value.kind === "Bool") {
      onWriteField(sessionId, filePath, rowKey, [{ kind: "Field", name: fieldName }], { kind: "Bool", v: !value.v }, value);
      return;
    }
    setEditingCell({ rowKey, fieldName, value });
  }, [filePath, sessionId, onWriteField]);

  const handleCellCommit = useCallback(async (rowKey: string, fieldName: string, raw: string, original: FieldValue) => {
    const newValue = parseFieldValue(raw, original);
    // Only write if changed
    const changed = fieldValueToString(newValue) !== fieldValueToString(original) || newValue.kind !== original.kind;
    if (!changed) { setEditingCell(null); return; }
    try {
      await onWriteField(sessionId, filePath, rowKey, [{ kind: "Field", name: fieldName }], newValue, original);
      setEditingCell(null);
    } catch {
      // onWriteField already shows error toast; keep cell open so user can retry or cancel
    }
  }, [sessionId, filePath, onWriteField]);

  const handleCellTabCommit = useCallback(async (rowKey: string, fieldName: string, raw: string, original: FieldValue) => {
    // Commit current cell, then move editing to the next editable column in the same row
    const newValue = parseFieldValue(raw, original);
    const changed = fieldValueToString(newValue) !== fieldValueToString(original) || newValue.kind !== original.kind;
    if (changed) {
      try {
        await onWriteField(sessionId, filePath, rowKey, [{ kind: "Field", name: fieldName }], newValue, original);
      } catch { /* keep going */ }
    }
    // Find next scalar editable field for this row
    const row = filteredRows.find(r => r.key === rowKey);
    if (!row) { setEditingCell(null); return; }
    const SCALAR_KINDS_SET = new Set(["Null", "Bool", "Int", "Float", "Str", "Enum", "Ref"]);
    const curIdx = fieldNames.indexOf(fieldName);
    for (let i = 1; i < fieldNames.length; i++) {
      const nextName = fieldNames[(curIdx + i) % fieldNames.length];
      if (row.spread_fields.includes(nextName)) continue;
      const nextVal = row.fields.find(f => f.name === nextName)?.value;
      if (nextVal && SCALAR_KINDS_SET.has(nextVal.kind) && nextVal.kind !== "Bool") {
        setEditingCell({ rowKey, fieldName: nextName, value: nextVal });
        return;
      }
    }
    setEditingCell(null);
  }, [sessionId, filePath, onWriteField, fieldNames, filteredRows]);

  const handleRenameCommit = useCallback(async () => {
    if (!renameModal || !onRenameRecord) return;
    const newKey = renameModal.draft.trim();
    if (!newKey) { setRenameModal(m => m && ({ ...m, error: "Key cannot be empty" })); return; }
    if (newKey === renameModal.rowKey) { setRenameModal(null); return; }
    try {
      await onRenameRecord(sessionId, filePath, renameModal.rowKey, newKey);
      setRenameModal(null);
    } catch (e) {
      setRenameModal(m => m && ({ ...m, error: String(e) }));
    }
  }, [renameModal, onRenameRecord, sessionId, filePath]);

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
    if (!deleteModal) return;
    try {
      await onDeleteRecord(sessionId, filePath, deleteModal.rowKey);
      setDeleteModal(null);
    } catch {
      // onDeleteRecord already shows error toast; modal auto-closes on success only
    }
  }, [deleteModal, onDeleteRecord, sessionId, filePath]);

  const handlePasteImport = useCallback(async () => {
    if (!pasteModal) return;
    if (!pasteModal.source.trim()) { setPasteModal(m => m && ({ ...m, error: "Paste CFD source first" })); return; }
    setPasteModal(m => m && ({ ...m, importing: true, error: null }));
    try {
      const importedKeys = await api.importRecordSource(sessionId, filePath, pasteModal.source);
      if (importedKeys.length === 0) {
        setPasteModal(m => m && ({ ...m, importing: false, error: "未导入任何记录（key 已存在或源码为空）" }));
        return;
      }
      if (importedKeys.length === 1) {
        setPasteModal(null);
        onNavigate({ view: "record", file: filePath, recordKey: importedKeys[0] });
      } else {
        setPasteModal(m => m && ({ ...m, importing: false, importedKeys }));
      }
    } catch (e) {
      setPasteModal(m => m && ({ ...m, importing: false, error: String(e) }));
    }
  }, [pasteModal, sessionId, filePath, onNavigate]);

  const showBatchSuccess = useCallback((msg: string) => {
    setBatchSuccess(msg);
    setBatchError(null);
    if (batchSuccessTimerRef.current) clearTimeout(batchSuccessTimerRef.current);
    batchSuccessTimerRef.current = setTimeout(() => setBatchSuccess(null), 3000);
  }, []);

  const handleBatchApply = useCallback(async () => {
    if (!batchField) { setBatchError("请选择字段"); return; }
    const rowsToEdit = filteredRows.filter(r => selectedKeys.has(r.key));
    if (rowsToEdit.length === 0) return;
    setBatchApplying(true);
    setBatchError(null);
    const failedKeys: string[] = [];
    for (const row of rowsToEdit) {
      const existing = row.fields.find(f => f.name === batchField)?.value ?? { kind: "Null" as const };
      const newValue = parseFieldValue(batchValue, existing);
      try {
        await onWriteField(sessionId, filePath, row.key, [{ kind: "Field", name: batchField }], newValue, existing);
      } catch {
        failedKeys.push(row.key);
      }
    }
    setBatchApplying(false);
    if (failedKeys.length > 0) {
      const preview = failedKeys.length <= 3
        ? failedKeys.join(", ")
        : failedKeys.slice(0, 3).join(", ") + ` 等 ${failedKeys.length} 条`;
      setBatchError(`写入失败: ${preview}`);
    } else {
      const edited = rowsToEdit.length;
      setSelectedKeys(new Set());
      setBatchField("");
      setBatchValue("");
      showBatchSuccess(`已写入 ${edited} 条记录的 ${batchField} 字段`);
    }
  }, [batchField, batchValue, filteredRows, selectedKeys, sessionId, filePath, onWriteField, showBatchSuccess]);

  const handleBatchDelete = useCallback(async () => {
    const keysToDelete = filteredRows.filter(r => selectedKeys.has(r.key)).map(r => r.key);
    if (keysToDelete.length === 0) return;
    setBatchDeleting(true);
    setBatchError(null);
    const failedKeys: string[] = [];
    for (const key of keysToDelete) {
      try {
        await onDeleteRecord(sessionId, filePath, key);
      } catch {
        failedKeys.push(key);
      }
    }
    setBatchDeleting(false);
    setBatchDeleteConfirm(false);
    if (failedKeys.length > 0) {
      const preview = failedKeys.length <= 3
        ? failedKeys.join(", ")
        : failedKeys.slice(0, 3).join(", ") + ` 等 ${failedKeys.length} 条`;
      setBatchError(`删除失败: ${preview}`);
    } else {
      const deleted = keysToDelete.length;
      setSelectedKeys(new Set());
      showBatchSuccess(`已删除 ${deleted} 条记录`);
    }
  }, [filteredRows, selectedKeys, sessionId, filePath, onDeleteRecord, showBatchSuccess]);

  const handleCreateRecord = async (createAnother?: boolean) => {
    if (!newRecord.key.trim()) { setNewRecord(r => ({ ...r, error: "Key cannot be empty" })); return; }
    if (!newRecord.typeName) { setNewRecord(r => ({ ...r, error: "Type is required" })); return; }
    const key = newRecord.key.trim();
    const typeName = newRecord.typeName;
    setCreating(true);
    setNewRecord(r => ({ ...r, error: null }));
    try {
      await onWriteField(sessionId, filePath, key, [], {
        kind: "Object",
        actual_type: typeName,
        fields: [],
      });
      if (createAnother) {
        // Stay in the modal; reset key for another record of the same type
        setNewRecord({ key: "", typeName, error: null });
      } else {
        setShowNewRecord(false);
        setNewRecord({ key: "", typeName: activeType, error: null });
        onNavigate({ view: "record", file: filePath, recordKey: key });
      }
    } catch (e) {
      setNewRecord(r => ({ ...r, error: String(e) }));
    } finally {
      setCreating(false);
    }
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", flex: 1, overflow: "hidden" }}>
      {/* Tab bar */}
      <div style={{
        display: "flex",
        gap: 2,
        padding: "4px 8px",
        borderBottom: "1px solid var(--border)",
        background: "var(--bg2)",
        flexShrink: 0,
      }}>
        {fileRecords.type_names.map(typeName => (
          <button
            key={typeName}
            onClick={() => { setActiveType(typeName); onTypeChange?.(typeName); }}
            style={{
              padding: "3px 10px",
              fontSize: 12,
              background: activeType === typeName ? "var(--bg3)" : "transparent",
              color: activeType === typeName ? "var(--text)" : "var(--text-muted)",
              border: activeType === typeName ? "1px solid var(--border)" : "1px solid transparent",
              borderRadius: 4,
              cursor: "pointer",
            }}
          >
            {typeName}
            <span style={{ marginLeft: 4, color: "var(--text-muted)", fontSize: 11 }}>
              ({fileRecords.records.filter(r => r.actual_type === typeName).length})
            </span>
          </button>
        ))}
      </div>

      {/* Search bar */}
      <div style={{
        display: "flex",
        alignItems: "center",
        gap: 6,
        padding: "4px 8px",
        borderBottom: "1px solid var(--border)",
        background: "var(--bg2)",
        flexShrink: 0,
        position: "relative",
      }}>
        <input
          ref={searchRef}
          value={search}
          onChange={e => setSearch(e.target.value)}
          onKeyDown={e => {
            if (e.key === "Escape") { setSearch(""); e.stopPropagation(); }
            else if (e.key === "Enter" && filteredRows.length > 0) {
              e.preventDefault();
              const row = filteredRows[focusedRowIndex ?? 0];
              if (row) onNavigate({ view: "record", file: filePath, recordKey: row.key });
            }
          }}
          placeholder="Search key or value… (field:value, Ctrl+F)"
          style={{
            flex: 1,
            background: "var(--bg3)",
            border: "1px solid var(--border)",
            borderRadius: 4,
            color: "var(--text)",
            padding: "3px 8px",
            fontSize: 12,
            outline: "none",
          }}
        />
        {search && (
          <button onClick={() => setSearch("")} style={{ fontSize: 11, padding: "2px 6px" }}>✕</button>
        )}
        {search && (
          <span style={{ color: "var(--text-muted)", fontSize: 11, whiteSpace: "nowrap" }}>
            {filteredRows.length} / {fileRecords.records.filter(r => r.actual_type === activeType).length}
          </span>
        )}
        {/* Required-null filter */}
        {requiredFieldNames.size > 0 && (
          <button
            onClick={() => setShowRequiredOnly(v => !v)}
            title={showRequiredOnly ? "显示全部记录" : "只显示含空必填字段的记录"}
            style={{
              fontSize: 11,
              padding: "2px 8px",
              background: showRequiredOnly ? "var(--warning)" : "transparent",
              border: `1px solid ${showRequiredOnly ? "var(--warning)" : "var(--border)"}`,
              borderRadius: 4,
              color: showRequiredOnly ? "#000" : "var(--warning)",
              cursor: "pointer",
              whiteSpace: "nowrap",
              fontWeight: showRequiredOnly ? 600 : undefined,
            }}
          >
            ⚠ 必填
          </button>
        )}
        {/* Column visibility toggle */}
        {fieldNames.length > 0 && (
          <div ref={columnPickerRef} style={{ position: "relative" }}>
            <button
              onClick={() => setShowColumnPicker(v => !v)}
              title="显示/隐藏列"
              style={{
                fontSize: 11,
                padding: "2px 8px",
                background: showColumnPicker ? "var(--bg3)" : "transparent",
                border: "1px solid var(--border)",
                borderRadius: 4,
                color: "var(--text-muted)",
                cursor: "pointer",
                whiteSpace: "nowrap",
              }}
            >
              ⊞ 列
              {Object.values(columnVisibility).some(v => v === false) && (
                <span style={{ marginLeft: 4, color: "var(--accent)", fontSize: 10 }}>●</span>
              )}
            </button>
            {showColumnPicker && (
              <div style={{
                position: "absolute",
                top: "100%",
                right: 0,
                marginTop: 4,
                background: "var(--bg2)",
                border: "1px solid var(--border)",
                borderRadius: 6,
                padding: "6px 0",
                zIndex: 1000,
                minWidth: 160,
                maxHeight: 320,
                overflowY: "auto",
                boxShadow: "0 4px 16px rgba(0,0,0,0.4)",
              }}>
                <div style={{ padding: "2px 10px 6px", display: "flex", gap: 6 }}>
                  <button
                    onClick={() => {
                      const vis: VisibilityState = {};
                      fieldNames.forEach(n => { vis[n] = true; });
                      setColumnVisibility(vis);
                    }}
                    style={{ fontSize: 10, padding: "1px 6px", flex: 1 }}
                  >全显</button>
                  <button
                    onClick={() => {
                      const vis: VisibilityState = {};
                      fieldNames.forEach(n => { vis[n] = false; });
                      setColumnVisibility(vis);
                    }}
                    style={{ fontSize: 10, padding: "1px 6px", flex: 1 }}
                  >全隐</button>
                </div>
                {table.getAllLeafColumns().filter(col => col.id !== "key").map(col => (
                  <label
                    key={col.id}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: 8,
                      padding: "3px 10px",
                      cursor: "pointer",
                      fontSize: 12,
                      color: "var(--text)",
                    }}
                    onMouseEnter={e => (e.currentTarget.style.background = "var(--bg3)")}
                    onMouseLeave={e => (e.currentTarget.style.background = "transparent")}
                  >
                    <input
                      type="checkbox"
                      checked={col.getIsVisible()}
                      onChange={col.getToggleVisibilityHandler()}
                      style={{ margin: 0, cursor: "pointer" }}
                    />
                    <span style={{ fontFamily: "monospace" }}>{col.id}</span>
                    {fieldSchemas.find(s => s.name === col.id) && (
                      <span style={{ color: "var(--text-muted)", fontSize: 10, marginLeft: "auto" }}>
                        {fieldSchemas.find(s => s.name === col.id)!.type_str}
                      </span>
                    )}
                  </label>
                ))}
              </div>
            )}
          </div>
        )}
      </div>

      {/* Table */}
      <div
        ref={parentRef}
        tabIndex={0}
        style={{ flex: 1, overflow: "auto", outline: "none" }}
        onKeyDown={e => {
          if (editingCell) return;
          const rowCount = rows.length;
          if (rowCount === 0) return;
          if (e.key === "ArrowDown") {
            e.preventDefault();
            setFocusedRowIndex(idx => {
              const next = idx === null ? 0 : Math.min(idx + 1, rowCount - 1);
              virtualizer.scrollToIndex(next, { align: "auto" });
              return next;
            });
          } else if (e.key === "ArrowUp") {
            e.preventDefault();
            setFocusedRowIndex(idx => {
              const next = idx === null ? 0 : Math.max(idx - 1, 0);
              virtualizer.scrollToIndex(next, { align: "auto" });
              return next;
            });
          } else if ((e.key === "Enter" || e.key === " ") && focusedRowIndex !== null) {
            e.preventDefault();
            const focusedRow = rows[focusedRowIndex];
            if (focusedRow) {
              onNavigate({ view: "record", file: filePath, recordKey: focusedRow.original.key });
            }
          }
        }}
      >
        <table style={{
          width: "100%",
          borderCollapse: "collapse",
          tableLayout: "fixed",
          fontSize: 12,
        }}>
          <thead style={{ position: "sticky", top: 0, zIndex: 1, background: "var(--bg2)" }}>
            {table.getHeaderGroups().map(headerGroup => (
              <tr key={headerGroup.id}>
                {headerGroup.headers.map(header => (
                  <th
                    key={header.id}
                    onClick={header.column.getCanSort() ? header.column.getToggleSortingHandler() : undefined}
                    style={{
                      position: "relative",
                      width: header.getSize(),
                      padding: "6px 8px",
                      textAlign: "left",
                      fontWeight: 600,
                      color: "var(--text-muted)",
                      borderBottom: "1px solid var(--border)",
                      userSelect: "none",
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                      cursor: header.column.getCanSort() ? "pointer" : "default",
                    }}
                    title={header.id}
                  >
                    {flexRender(header.column.columnDef.header, header.getContext())}
                    {header.column.getIsSorted() === "asc" && (
                      <span style={{ marginLeft: 4, fontSize: 10 }}>▲</span>
                    )}
                    {header.column.getIsSorted() === "desc" && (
                      <span style={{ marginLeft: 4, fontSize: 10 }}>▼</span>
                    )}
                    {header.column.getCanSort() && !header.column.getIsSorted() && (
                      <span style={{ marginLeft: 4, fontSize: 10, opacity: 0.3 }}>⇅</span>
                    )}
                    {header.column.getCanResize() && (
                      <div
                        onMouseDown={header.getResizeHandler()}
                        onTouchStart={header.getResizeHandler()}
                        onClick={e => e.stopPropagation()}
                        style={{
                          position: "absolute",
                          right: 0,
                          top: 0,
                          height: "100%",
                          width: 4,
                          cursor: "col-resize",
                          background: header.column.getIsResizing() ? "var(--accent)" : "transparent",
                          userSelect: "none",
                          touchAction: "none",
                        }}
                      />
                    )}
                  </th>
                ))}
              </tr>
            ))}
          </thead>
          <tbody style={{ position: "relative" }}>
            {/* Spacer top */}
            {virtualItems.length > 0 && virtualItems[0].start > 0 && (
              <tr>
                <td style={{ height: virtualItems[0].start, padding: 0 }} colSpan={columns.length} />
              </tr>
            )}
            {virtualItems.map(vItem => {
              const row = rows[vItem.index];
              return (
                <tr
                  key={row.id}
                  data-index={vItem.index}
                  ref={virtualizer.measureElement}
                  onContextMenu={e => handleRowContextMenu(e, row.original)}
                  onClick={() => {
                    // Dismiss editing if clicking outside an active cell input
                    if (editingCell && editingCell.rowKey !== row.original.key) setEditingCell(null);
                    setFocusedRowIndex(vItem.index);
                  }}
                  style={{
                    height: vItem.size,
                    cursor: "pointer",
                    background: selectedKeys.has(row.original.key)
                      ? "color-mix(in srgb, var(--accent) 12%, var(--bg))"
                      : focusedRowIndex === vItem.index
                        ? "var(--bg3)"
                        : "transparent",
                    outline: focusedRowIndex === vItem.index ? "1px solid color-mix(in srgb, var(--accent) 40%, transparent)" : "none",
                    outlineOffset: -1,
                  }}
                  onMouseEnter={e => {
                    if (!selectedKeys.has(row.original.key) && focusedRowIndex !== vItem.index) e.currentTarget.style.background = "var(--bg3)";
                  }}
                  onMouseLeave={e => {
                    if (selectedKeys.has(row.original.key)) {
                      e.currentTarget.style.background = "color-mix(in srgb, var(--accent) 12%, var(--bg))";
                    } else if (focusedRowIndex === vItem.index) {
                      e.currentTarget.style.background = "var(--bg3)";
                    } else {
                      e.currentTarget.style.background = "transparent";
                    }
                  }}
                  onDoubleClick={e => {
                    if ((e.target as HTMLElement).tagName === "INPUT") return;
                    onNavigate({ view: "record", file: filePath, recordKey: row.original.key });
                  }}
                >
                  {row.getVisibleCells().map(cell => {
                    const colId = cell.column.id;
                    const isSelCol = colId === "__sel__";
                    const isKeyCol = colId === "key";
                    const cellValue = (isKeyCol || isSelCol)
                      ? null
                      : (row.original.fields.find(f => f.name === colId)?.value ?? null);
                    const isSpreadField = !isKeyCol && !isSelCol && row.original.spread_fields.includes(colId);
                    const isEditing =
                      !isKeyCol &&
                      !isSelCol &&
                      !isSpreadField &&
                      editingCell?.rowKey === row.original.key &&
                      editingCell?.fieldName === colId;

                    if (isSelCol) {
                      return (
                        <td
                          key={cell.id}
                          style={{
                            padding: "4px 6px",
                            borderBottom: "1px solid var(--bg3)",
                            width: cell.column.getSize(),
                            textAlign: "center",
                          }}
                        >
                          {flexRender(cell.column.columnDef.cell, cell.getContext())}
                        </td>
                      );
                    }

                    return (
                      <td
                        key={cell.id}
                        onClick={e => {
                          if (isKeyCol || isSpreadField) return;
                          if (!cellValue) return;
                          if (!SCALAR_KINDS.includes(cellValue.kind)) return;
                          e.stopPropagation();
                          handleCellClick(row.original.key, colId, cellValue);
                        }}
                        onContextMenu={e => {
                          if (isKeyCol) return;
                          if (!cellValue) return;
                          const items: { label: string; onClick: () => void }[] = [];
                          // Copy scalar value
                          let copyText: string | null = null;
                          const cv = cellValue;
                          switch (cv.kind) {
                            case "Null": copyText = "null"; break;
                            case "Bool": copyText = String(cv.v); break;
                            case "Int": case "Float": copyText = String(cv.v); break;
                            case "Str": copyText = cv.v; break;
                            case "Enum": copyText = cv.variant; break;
                            case "Ref": copyText = cv.target_key; break;
                          }
                          if (copyText !== null) {
                            const text = copyText;
                            items.push({ label: "复制值", onClick: () => navigator.clipboard.writeText(text).catch(e => onError?.(`复制失败: ${e}`)) });
                          }
                          if (cv.kind === "Ref") {
                            const refValue = cv;
                            items.push({ label: "跳转到引用记录", onClick: () => onNavigate({ view: "record", file: refValue.target_file ?? filePath, recordKey: refValue.target_key }) });
                          }
                          if (items.length > 0) {
                            e.preventDefault();
                            e.stopPropagation();
                            setContextMenu({ x: e.clientX, y: e.clientY, items });
                          }
                        }}
                        title={isSpreadField ? "来自 spread — 请前往源记录编辑" : undefined}
                        style={{
                          padding: isEditing ? 0 : "4px 8px",
                          borderBottom: "1px solid var(--bg3)",
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          whiteSpace: "nowrap",
                          width: cell.column.getSize(),
                          maxWidth: cell.column.getSize(),
                          opacity: isSpreadField ? 0.6 : 1,
                          cursor: isKeyCol || isSpreadField ? "default" :
                            (cellValue?.kind === "Ref" || cellValue?.kind === "Bool" || cellValue?.kind === "Enum" ? "pointer" :
                             (cellValue && SCALAR_KINDS.includes(cellValue.kind) ? "text" : "default")),
                        }}
                      >
                        {isEditing && editingCell ? (
                          <CellEditor
                            value={editingCell.value}
                            sessionId={sessionId}
                            onCommit={raw => handleCellCommit(row.original.key, colId, raw, editingCell.value)}
                            onCancel={() => setEditingCell(null)}
                            onTabCommit={raw => handleCellTabCommit(row.original.key, colId, raw, editingCell.value)}
                          />
                        ) : (
                          flexRender(cell.column.columnDef.cell, cell.getContext())
                        )}
                      </td>
                    );
                  })}
                </tr>
              );
            })}
            {/* Spacer bottom */}
            {virtualItems.length > 0 && (() => {
              const lastItem = virtualItems[virtualItems.length - 1];
              const remaining = totalHeight - lastItem.end;
              if (remaining > 0) {
                return (
                  <tr>
                    <td style={{ height: remaining, padding: 0 }} colSpan={columns.length} />
                  </tr>
                );
              }
              return null;
            })()}
          </tbody>
        </table>

        {filteredRows.length === 0 && (
          <div style={{ padding: 32, textAlign: "center", color: "var(--text-muted)", display: "flex", flexDirection: "column", alignItems: "center", gap: 12 }}>
            <span>
              {activeType
                ? <>No records of type <strong style={{ color: "var(--text)" }}>{activeType}</strong></>
                : <>This file has no records yet</>
              }
            </span>
            <button
              className="primary"
              onClick={() => { setNewRecord(r => ({ ...r, typeName: activeType || (allTypeNames[0] ?? ""), error: null })); setShowNewRecord(true); }}
              style={{ fontSize: 12 }}
            >
              + 创建第一条记录
            </button>
          </div>
        )}
      </div>

      {/* Batch edit bar (shown when rows are selected) */}
      {selectedKeys.size > 0 && (
        <div style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          padding: "6px 12px",
          borderTop: "1px solid var(--accent)",
          background: "var(--bg2)",
          flexShrink: 0,
        }}>
          <span style={{ color: "var(--accent)", fontWeight: 600, fontSize: 12, whiteSpace: "nowrap" }}>
            {selectedKeys.size} 行已选
          </span>
          <select
            value={batchField}
            onChange={e => { setBatchField(e.target.value); setBatchError(null); }}
            style={{
              background: "var(--bg3)",
              border: "1px solid var(--border)",
              borderRadius: 4,
              color: batchField ? "var(--text)" : "var(--text-muted)",
              padding: "3px 6px",
              fontSize: 12,
              outline: "none",
            }}
          >
            <option value="">选择字段…</option>
            {fieldNames.map(n => (
              <option key={n} value={n}>{n}</option>
            ))}
          </select>
          <input
            value={batchValue}
            onChange={e => { setBatchValue(e.target.value); setBatchError(null); }}
            placeholder="新值…"
            onKeyDown={e => { if (e.key === "Enter") { e.preventDefault(); handleBatchApply(); } e.stopPropagation(); }}
            style={{
              flex: 1,
              background: "var(--bg3)",
              border: batchError ? "1px solid var(--error)" : "1px solid var(--border)",
              borderRadius: 4,
              color: "var(--text)",
              padding: "3px 8px",
              fontSize: 12,
              fontFamily: "monospace",
              outline: "none",
            }}
          />
          {batchError && (
            <span style={{ color: "var(--error)", fontSize: 11 }}>{batchError}</span>
          )}
          {batchSuccess && !batchError && (
            <span style={{ color: "var(--accent)", fontSize: 11 }}>✓ {batchSuccess}</span>
          )}
          <button
            className="primary"
            disabled={batchApplying || !batchField}
            onClick={handleBatchApply}
            style={{ fontSize: 12, whiteSpace: "nowrap" }}
          >
            {batchApplying ? "应用中…" : "批量应用"}
          </button>
          {batchDeleteConfirm ? (
            <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <span style={{ color: "var(--error)", fontSize: 12, whiteSpace: "nowrap" }}>
                确定删除 {selectedKeys.size} 条记录？
              </span>
              <button
                className="danger"
                disabled={batchDeleting}
                onClick={handleBatchDelete}
                style={{ fontSize: 12, whiteSpace: "nowrap" }}
              >
                {batchDeleting ? "删除中…" : "确认删除"}
              </button>
              <button
                onClick={() => setBatchDeleteConfirm(false)}
                style={{ fontSize: 12, whiteSpace: "nowrap" }}
              >
                取消
              </button>
            </div>
          ) : (
            <button
              disabled={batchApplying}
              onClick={() => setBatchDeleteConfirm(true)}
              style={{ fontSize: 12, whiteSpace: "nowrap", color: "var(--error)", border: "1px solid var(--error)", background: "transparent", borderRadius: 4, padding: "3px 8px", cursor: "pointer" }}
            >
              🗑 批量删除
            </button>
          )}
          <button
            onClick={() => { setSelectedKeys(new Set()); setBatchError(null); setBatchDeleteConfirm(false); }}
            style={{ fontSize: 12, whiteSpace: "nowrap" }}
          >
            取消选择
          </button>
        </div>
      )}

      {/* Bottom bar */}
      <div style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        padding: "6px 12px",
        borderTop: "1px solid var(--border)",
        background: "var(--bg2)",
        flexShrink: 0,
      }}>
        <span style={{ color: "var(--text-muted)", fontSize: 12 }}>
          {filteredRows.length} record{filteredRows.length !== 1 ? "s" : ""}
          {selectedKeys.size > 0 && (
            <span style={{ color: "var(--accent)", marginLeft: 8 }}>{selectedKeys.size} selected</span>
          )}
        </span>
        <div style={{ display: "flex", gap: 6 }}>
          <button
            onClick={() => {
              // Export visible columns for current type as CSV
              const visibleCols = table.getAllLeafColumns().filter(c => c.getIsVisible() && c.id !== "__sel__");
              const header = visibleCols.map(c => c.id).join(",");
              const rows = filteredRows.map(row => {
                return visibleCols.map(col => {
                  const cell = col.id === "key" ? row.key : (row.fields.find(f => f.name === col.id)?.value);
                  if (!cell || cell === row.key) return `"${row.key}"`;
                  if (typeof cell === "string") return `"${cell}"`;
                  const v = cell as FieldValue;
                  switch (v.kind) {
                    case "Null": return "";
                    case "Bool": return String(v.v);
                    case "Int": case "Float": return String(v.v);
                    case "Str": return `"${v.v.replace(/"/g, '""')}"`;
                    case "Enum": return v.variant;
                    case "Ref": return v.target_key;
                    default: return "";
                  }
                }).join(",");
              });
              const csv = [header, ...rows].join("\n");
              const blob = new Blob([csv], { type: "text/csv" });
              const url = URL.createObjectURL(blob);
              const a = document.createElement("a");
              a.href = url;
              a.download = `${activeType || "records"}.csv`;
              a.click();
              URL.revokeObjectURL(url);
            }}
            title="Export visible columns as CSV"
            style={{ fontSize: 12 }}
          >
            ↓ CSV
          </button>
          <button
            onClick={() => setPasteModal({ source: "", error: null, importing: false })}
            title="Paste CFD source text to import records"
            style={{ fontSize: 12 }}
          >
            ⎘ 粘贴 CFD
          </button>
          {onSortFile && (
            <button
              onClick={onSortFile}
              title="Sort all records in this file alphabetically by key"
              style={{ fontSize: 12 }}
            >
              ⇅ 排序
            </button>
          )}
          <button onClick={() => { setNewRecord(r => ({ ...r, typeName: activeType, error: null })); setShowNewRecord(true); }} style={{ fontSize: 12 }}>
            + 新建记录
          </button>
        </div>
      </div>

      {/* New record modal */}
      {showNewRecord && (
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
          onClick={() => setShowNewRecord(false)}
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
            <h3 style={{ margin: 0, fontSize: 15 }}>新建记录</h3>
            {newRecord.error && (
              <div style={{ color: "#ff5555", fontSize: 12, background: "#ff555522", border: "1px solid #ff555544", borderRadius: 4, padding: "4px 8px" }}>
                {newRecord.error}
              </div>
            )}
            <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 13 }}>
              <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
                Key
                <button
                  type="button"
                  onClick={() => {
                    const typeName = newRecord.typeName || activeType || "";
                    const prefix = typeName.replace(/([A-Z])/g, m => `_${m.toLowerCase()}`).replace(/^_/, "").toLowerCase();
                    const existingKeys = new Set(filteredRows.map(r => r.key));
                    let n = 1;
                    while (existingKeys.has(`${prefix}_${String(n).padStart(3, "0")}`)) n++;
                    setNewRecord(r => ({ ...r, key: `${prefix}_${String(n).padStart(3, "0")}`, error: null }));
                  }}
                  style={{ fontSize: 11, padding: "1px 6px", background: "transparent", border: "1px solid var(--border)", borderRadius: 3, color: "var(--text-muted)", cursor: "pointer" }}
                >
                  ✦ 建议
                </button>
              </div>
              <input
                value={newRecord.key}
                onChange={e => setNewRecord(r => ({ ...r, key: e.target.value, error: null }))}
                onKeyDown={e => {
                  if (e.key === "Enter") { e.preventDefault(); handleCreateRecord(false); }
                  if (e.key === "Escape") setShowNewRecord(false);
                }}
                style={{
                  background: "var(--bg3)",
                  border: newRecord.error ? "1px solid #ff5555" : "1px solid var(--border)",
                  borderRadius: 4,
                  color: "var(--text)",
                  padding: "4px 8px",
                  fontSize: 13,
                  fontFamily: "monospace",
                  outline: "none",
                }}
                placeholder="record_key"
                autoFocus
              />
            </label>
            <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 13 }}>
              Type
              <select
                value={newRecord.typeName}
                onChange={e => setNewRecord(r => ({ ...r, typeName: e.target.value }))}
                style={{
                  background: "var(--bg3)",
                  border: "1px solid var(--border)",
                  borderRadius: 4,
                  color: "var(--text)",
                  padding: "4px 8px",
                  fontSize: 13,
                  outline: "none",
                }}
              >
                {allTypeNames.map(t => (
                  <option key={t} value={t}>{t}</option>
                ))}
              </select>
            </label>
            <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
              <button onClick={() => { setShowNewRecord(false); setNewRecord(r => ({ ...r, error: null })); }}>取消</button>
              <button
                onClick={() => handleCreateRecord(true)}
                disabled={creating || !newRecord.key.trim()}
                title="创建记录并保持对话框打开以创建更多"
              >
                {creating ? "创建中…" : "再创建一条"}
              </button>
              <button
                className="primary"
                onClick={() => handleCreateRecord(false)}
                disabled={creating || !newRecord.key.trim()}
              >
                {creating ? "创建中…" : "创建并打开"}
              </button>
            </div>
          </div>
        </div>
      )}

      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          items={contextMenu.items}
          onClose={() => setContextMenu(null)}
        />
      )}

      {/* Rename record modal */}
      {renameModal && (
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
          onClick={() => setRenameModal(null)}
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
            <h3 style={{ margin: 0, fontSize: 15 }}>重命名记录 Key</h3>
            <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 13 }}>
              新 Key
              <input
                value={renameModal.draft}
                onChange={e => setRenameModal(m => m && ({ ...m, draft: e.target.value, error: null }))}
                onKeyDown={e => {
                  if (e.key === "Enter") { e.preventDefault(); handleRenameCommit(); }
                  if (e.key === "Escape") setRenameModal(null);
                  e.stopPropagation();
                }}
                style={{
                  background: "var(--bg3)",
                  border: renameModal.error ? "1px solid #ff5555" : "1px solid var(--border)",
                  borderRadius: 4,
                  color: "var(--text)",
                  padding: "4px 8px",
                  fontSize: 13,
                  fontFamily: "monospace",
                  outline: "none",
                }}
                autoFocus
              />
              {renameModal.error && (
                <span style={{ color: "#ff5555", fontSize: 11 }}>{renameModal.error}</span>
              )}
            </label>
            <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
              <button onClick={() => setRenameModal(null)}>取消</button>
              <button
                className="primary"
                onClick={handleRenameCommit}
                disabled={!renameModal.draft.trim()}
              >
                重命名
              </button>
            </div>
          </div>
        </div>
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
              确认删除记录 <code style={{ fontFamily: "monospace" }}>{deleteModal.rowKey}</code>？此操作不可撤销。
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

      {/* Paste CFD source modal */}
      {pasteModal && (
        <div
          style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,0.6)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 2000 }}
          onClick={() => !pasteModal.importing && setPasteModal(null)}
        >
          <div
            style={{ background: "var(--bg2)", border: "1px solid var(--border)", borderRadius: 8, padding: 24, width: 560, display: "flex", flexDirection: "column", gap: 12 }}
            onClick={e => e.stopPropagation()}
          >
            <h3 style={{ margin: 0, fontSize: 15 }}>粘贴 CFD 源码导入</h3>
            {pasteModal.importedKeys ? (
              <>
                <div style={{ color: "var(--success, #50fa7b)", fontSize: 13, background: "rgba(80,250,123,0.08)", border: "1px solid rgba(80,250,123,0.3)", borderRadius: 4, padding: "8px 12px" }}>
                  ✓ 已导入 {pasteModal.importedKeys.length} 条记录
                </div>
                <div style={{ display: "flex", flexDirection: "column", gap: 4, maxHeight: 160, overflowY: "auto" }}>
                  {pasteModal.importedKeys.map(k => (
                    <button
                      key={k}
                      onClick={() => { setPasteModal(null); onNavigate({ view: "record", file: filePath, recordKey: k }); }}
                      style={{ textAlign: "left", fontFamily: "monospace", fontSize: 12, padding: "2px 8px" }}
                    >
                      → {k}
                    </button>
                  ))}
                </div>
                <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
                  <button className="primary" onClick={() => setPasteModal(null)}>关闭</button>
                </div>
              </>
            ) : (
              <>
                <div style={{ fontSize: 12, color: "var(--text-muted)" }}>
                  粘贴一条或多条 CFD 格式记录，将追加到 <code style={{ fontFamily: "monospace" }}>{filePath}</code>。
                </div>
                {pasteModal.error && (
                  <div style={{ color: "#ff5555", fontSize: 12, background: "#ff555522", border: "1px solid #ff555544", borderRadius: 4, padding: "4px 8px" }}>
                    {pasteModal.error}
                  </div>
                )}
                <textarea
                  value={pasteModal.source}
                  onChange={e => setPasteModal(m => m && ({ ...m, source: e.target.value, error: null }))}
                  onKeyDown={e => {
                    if ((e.ctrlKey || e.metaKey) && e.key === "Enter" && !pasteModal.importing && pasteModal.source.trim()) {
                      e.preventDefault();
                      handlePasteImport();
                    }
                  }}
                  rows={12}
                  spellCheck={false}
                  placeholder={"sword: Weapon {\n  name: \"Fire Sword\"\n  power: 100\n}"}
                  style={{
                    background: "var(--bg3)",
                    border: pasteModal.error ? "1px solid #ff5555" : "1px solid var(--border)",
                    borderRadius: 4,
                    color: "var(--text)",
                    padding: "8px",
                    fontSize: 12,
                    fontFamily: "monospace",
                    outline: "none",
                    resize: "vertical",
                  }}
                />
                <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
                  <button onClick={() => setPasteModal(null)} disabled={pasteModal.importing}>取消</button>
                  <button
                    className="primary"
                    onClick={handlePasteImport}
                    disabled={pasteModal.importing || !pasteModal.source.trim()}
                  >
                    {pasteModal.importing ? "导入中…" : "导入"}
                  </button>
                </div>
              </>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
