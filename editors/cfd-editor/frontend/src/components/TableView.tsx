import { useState, useRef, useCallback, useEffect } from "react";
import {
  useReactTable,
  getCoreRowModel,
  createColumnHelper,
  flexRender,
} from "@tanstack/react-table";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { FileRecords, RecordRow, FieldValue, FieldPathSegment } from "../bindings";
import type { Route } from "../router";
import { DataCard } from "./DataCard";
import { ContextMenu, type ContextMenuState } from "./ContextMenu";

interface TableViewProps {
  fileRecords: FileRecords;
  sessionId: number;
  filePath: string;
  initialTypeFilter?: string;
  onWriteField: (
    sessionId: number,
    filePath: string,
    recordKey: string,
    fieldPath: FieldPathSegment[],
    newValue: FieldValue
  ) => Promise<void>;
  onDeleteRecord: (sessionId: number, filePath: string, recordKey: string) => Promise<void>;
  onNavigate: (route: Route) => void;
}

interface NewRecordForm {
  key: string;
  typeName: string;
}

type RowData = RecordRow & { _filePath: string };

const columnHelper = createColumnHelper<RowData>();

export function TableView({
  fileRecords,
  sessionId,
  filePath,
  initialTypeFilter,
  onWriteField,
  onDeleteRecord,
  onNavigate,
}: TableViewProps) {
  const [activeType, setActiveType] = useState<string>(
    initialTypeFilter && fileRecords.type_names.includes(initialTypeFilter)
      ? initialTypeFilter
      : fileRecords.type_names[0] ?? ""
  );
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);

  // Keep activeType valid when the type list changes after reload
  useEffect(() => {
    if (fileRecords.type_names.length > 0 && !fileRecords.type_names.includes(activeType)) {
      setActiveType(fileRecords.type_names[0]);
    }
  }, [fileRecords.type_names, activeType]);
  const [showNewRecord, setShowNewRecord] = useState(false);
  const [newRecord, setNewRecord] = useState<NewRecordForm>({ key: "", typeName: fileRecords.type_names[0] ?? "" });
  const [creating, setCreating] = useState(false);
  const parentRef = useRef<HTMLDivElement>(null);

  const filteredRows: RowData[] = fileRecords.records
    .filter(r => r.actual_type === activeType)
    .map(r => ({ ...r, _filePath: filePath }));

  // Determine columns from first record of active type
  const firstRow = filteredRows[0];
  const fieldNames: string[] = firstRow ? firstRow.fields.map(f => f.name) : [];

  const columns = [
    columnHelper.accessor("key", {
      header: "key",
      size: 160,
      cell: info => (
        <span style={{ fontFamily: "monospace", fontWeight: 600, fontSize: 12 }}>
          {info.getValue()}
        </span>
      ),
    }),
    ...fieldNames.map(name =>
      columnHelper.accessor(
        row => row.fields.find(f => f.name === name)?.value ?? { kind: "Null" as const },
        {
          id: name,
          header: name,
          size: 160,
          cell: info => (
            <DataCard mode="compact" value={info.getValue()} />
          ),
        }
      )
    ),
  ];

  const table = useReactTable({
    data: filteredRows,
    columns,
    getCoreRowModel: getCoreRowModel(),
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
          label: "删除记录",
          danger: true,
          onClick: async () => {
            if (window.confirm(`Delete record "${row.key}"?`)) {
              await onDeleteRecord(sessionId, filePath, row.key);
            }
          },
        },
      ],
    });
  }, [filePath, sessionId, onNavigate, onDeleteRecord]);

  const handleColHeaderContextMenu = useCallback((
    e: React.MouseEvent,
    fieldName: string
  ) => {
    // Check if this is a ref field in the first row
    const firstWithField = filteredRows.find(r => r.fields.some(f => f.name === fieldName && f.value.kind === "Ref"));
    if (!firstWithField) return;
    e.preventDefault();
    const refField = firstWithField.fields.find(f => f.name === fieldName);
    if (!refField || refField.value.kind !== "Ref") return;
    const refValue = refField.value;
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
  }, [filteredRows, filePath, onNavigate]);

  const handleCreateRecord = async () => {
    if (!newRecord.key.trim() || !newRecord.typeName) return;
    setCreating(true);
    try {
      // API call will be handled by parent via onWriteField pattern
      // For now we call the create api directly through a passed prop
      // The parent reloads via markDirty — we just close the modal
      // We pass through a special path to signal creation
      await onWriteField(sessionId, filePath, newRecord.key, [], {
        kind: "Object",
        actual_type: newRecord.typeName,
        fields: [],
      });
    } finally {
      setCreating(false);
      setShowNewRecord(false);
      setNewRecord({ key: "", typeName: fileRecords.type_names[0] ?? "" });
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
            onClick={() => setActiveType(typeName)}
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
                    style={{
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
                    }}
                    onContextMenu={e => handleColHeaderContextMenu(e, header.id)}
                  >
                    {flexRender(header.column.columnDef.header, header.getContext())}
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
                  style={{
                    height: vItem.size,
                    cursor: "pointer",
                  }}
                  onMouseEnter={e => (e.currentTarget.style.background = "var(--bg3)")}
                  onMouseLeave={e => (e.currentTarget.style.background = "transparent")}
                  onDoubleClick={() => onNavigate({ view: "record", file: filePath, recordKey: row.original.key })}
                >
                  {row.getVisibleCells().map(cell => (
                    <td
                      key={cell.id}
                      style={{
                        padding: "4px 8px",
                        borderBottom: "1px solid var(--bg3)",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                        maxWidth: cell.column.getSize(),
                      }}
                    >
                      {flexRender(cell.column.columnDef.cell, cell.getContext())}
                    </td>
                  ))}
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
          <div style={{ padding: 24, textAlign: "center", color: "var(--text-muted)" }}>
            No records of type <strong>{activeType}</strong>
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
        <button onClick={() => setShowNewRecord(true)} style={{ fontSize: 12 }}>
          + 新建记录
        </button>
      </div>

      {/* New record modal */}
      {showNewRecord && (
        <div style={{
          position: "fixed",
          inset: 0,
          background: "rgba(0,0,0,0.6)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          zIndex: 2000,
        }}>
          <div style={{
            background: "var(--bg2)",
            border: "1px solid var(--border)",
            borderRadius: 8,
            padding: 24,
            width: 360,
            display: "flex",
            flexDirection: "column",
            gap: 12,
          }}>
            <h3 style={{ margin: 0, fontSize: 15 }}>新建记录</h3>
            <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 13 }}>
              Key
              <input
                value={newRecord.key}
                onChange={e => setNewRecord(r => ({ ...r, key: e.target.value }))}
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
                {fileRecords.type_names.map(t => (
                  <option key={t} value={t}>{t}</option>
                ))}
              </select>
            </label>
            <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
              <button onClick={() => setShowNewRecord(false)}>取消</button>
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
    </div>
  );
}
