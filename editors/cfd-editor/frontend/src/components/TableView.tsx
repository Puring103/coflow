import { useState, useRef, useCallback, useEffect } from "react";
import {
  useReactTable,
  getCoreRowModel,
  getSortedRowModel,
  createColumnHelper,
  flexRender,
  type SortingState,
  type ColumnResizeMode,
} from "@tanstack/react-table";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { FileRecords, RecordRow, FieldValue, FieldPathSegment, FieldSchema } from "../bindings";
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
  onNavigate: (route: Route) => void;
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

const columnHelper = createColumnHelper<RowData>();

interface CellEditorProps {
  value: FieldValue;
  sessionId: number;
  onCommit: (raw: string) => void;
  onCancel: () => void;
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

function CellEditor({ value, sessionId, onCommit, onCancel }: CellEditorProps) {
  const [text, setText] = useState(() => fieldValueToString(value));
  const inputRef = useRef<HTMLInputElement>(null);
  const [enumVariants, setEnumVariants] = useState<string[] | null>(null);
  const [refTargets, setRefTargets] = useState<string[]>([]);
  const listId = useRef(`cl-${Math.random().toString(36).slice(2)}`).current;

  useEffect(() => {
    if (value.kind === "Enum") {
      api.getEnumVariants(sessionId, value.enum_name).then(vs => {
        setEnumVariants(vs.length > 0 ? vs : []);
      }).catch(() => { setEnumVariants([]); });
    } else if (value.kind === "Ref" && value.target_type) {
      api.getRefTargets(sessionId, value.target_type).then(keys => {
        if (keys.length > 0) setRefTargets(keys);
      }).catch(() => {});
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
  onNavigate,
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

  // Fetch field schemas for the active type to enable required-null highlighting
  useEffect(() => {
    if (!activeType) { setFieldSchemas([]); return; }
    let cancelled = false;
    api.getFieldSchemas(sessionId, activeType)
      .then(s => { if (!cancelled) setFieldSchemas(s); })
      .catch(() => { if (!cancelled) setFieldSchemas([]); });
    return () => { cancelled = true; };
  }, [sessionId, activeType]);

  // Reset sorting and search when type changes (columns are different per type)
  useEffect(() => { setSorting([]); setSearch(""); }, [activeType]);
  const [showNewRecord, setShowNewRecord] = useState(false);
  const [newRecord, setNewRecord] = useState<NewRecordForm>({ key: "", typeName: activeType ?? fileRecords.type_names[0] ?? "", error: null });
  const [creating, setCreating] = useState(false);
  const parentRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  // Keyboard shortcuts: Ctrl+N opens new-record modal; Ctrl+F focuses search; Escape clears/closes
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "n") {
        e.preventDefault();
        setNewRecord(r => ({ ...r, typeName: activeType ?? r.typeName }));
        setShowNewRecord(true);
      }
      if ((e.ctrlKey || e.metaKey) && e.key === "f") {
        e.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
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
  }, []);

  const filteredRows: RowData[] = fileRecords.records
    .filter(r => {
      if (r.actual_type !== activeType) return false;
      if (!search) return true;
      const q = search.toLowerCase();
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
    columnHelper.accessor("key", {
      header: "key",
      size: 160,
      enableSorting: true,
      cell: info => {
        const row = info.row.original;
        return (
          <span style={{ fontFamily: "monospace", fontWeight: 600, fontSize: 12, color: row.is_fallback ? "var(--warning)" : undefined }}
            title={row.is_fallback ? "Model build failed — record may have missing required fields" : undefined}>
            {info.getValue()}
            {row.is_fallback && <span style={{ fontSize: 9, marginLeft: 3, opacity: 0.7 }}>⚠</span>}
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
    state: { sorting },
    onSortingChange: setSorting,
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
          label: "复制 Key",
          onClick: () => navigator.clipboard.writeText(row.key).catch(() => {}),
        },
        {
          label: "复制为 CFD 源码",
          onClick: () => api.getRecordSource(sessionId, filePath, row.key).then(src => navigator.clipboard.writeText(src)).catch(() => {}),
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
        {
          label: "删除记录",
          danger: true,
          onClick: () => setDeleteModal({ rowKey: row.key }),
        },
      ],
    });
  }, [filePath, sessionId, onNavigate, onDeleteRecord, onRenameRecord, onDuplicateRecord, onMoveRecord]);


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

  const handleCreateRecord = async () => {
    if (!newRecord.key.trim()) { setNewRecord(r => ({ ...r, error: "Key cannot be empty" })); return; }
    if (!newRecord.typeName) { setNewRecord(r => ({ ...r, error: "Type is required" })); return; }
    const key = newRecord.key.trim();
    setCreating(true);
    setNewRecord(r => ({ ...r, error: null }));
    try {
      await onWriteField(sessionId, filePath, key, [], {
        kind: "Object",
        actual_type: newRecord.typeName,
        fields: [],
      });
      setShowNewRecord(false);
      setNewRecord({ key: "", typeName: activeType, error: null });
      // Navigate to the new record so the user can fill in fields immediately
      onNavigate({ view: "record", file: filePath, recordKey: key });
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
      }}>
        <input
          ref={searchRef}
          value={search}
          onChange={e => setSearch(e.target.value)}
          placeholder="Search key or value… (Ctrl+F)"
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
      </div>

      {/* Table */}
      <div
        ref={parentRef}
        style={{ flex: 1, overflow: "auto" }}
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
                  }}
                  style={{
                    height: vItem.size,
                    cursor: "pointer",
                  }}
                  onMouseEnter={e => (e.currentTarget.style.background = "var(--bg3)")}
                  onMouseLeave={e => (e.currentTarget.style.background = "transparent")}
                  onDoubleClick={e => {
                    if ((e.target as HTMLElement).tagName === "INPUT") return;
                    onNavigate({ view: "record", file: filePath, recordKey: row.original.key });
                  }}
                >
                  {row.getVisibleCells().map(cell => {
                    const colId = cell.column.id;
                    const isKeyCol = colId === "key";
                    const cellValue = isKeyCol
                      ? null
                      : (row.original.fields.find(f => f.name === colId)?.value ?? null);
                    const isSpreadField = !isKeyCol && row.original.spread_fields.includes(colId);
                    const isEditing =
                      !isKeyCol &&
                      !isSpreadField &&
                      editingCell?.rowKey === row.original.key &&
                      editingCell?.fieldName === colId;

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
                            items.push({ label: "复制值", onClick: () => navigator.clipboard.writeText(text).catch(() => {}) });
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
        </span>
        <button onClick={() => { setNewRecord(r => ({ ...r, typeName: activeType, error: null })); setShowNewRecord(true); }} style={{ fontSize: 12 }}>
          + 新建记录
        </button>
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
              Key
              <input
                value={newRecord.key}
                onChange={e => setNewRecord(r => ({ ...r, key: e.target.value, error: null }))}
                onKeyDown={e => {
                  if (e.key === "Enter") { e.preventDefault(); handleCreateRecord(); }
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
                className="primary"
                onClick={handleCreateRecord}
                disabled={creating || !newRecord.key.trim()}
              >
                {creating ? "创建中…" : "创建"}
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
    </div>
  );
}
