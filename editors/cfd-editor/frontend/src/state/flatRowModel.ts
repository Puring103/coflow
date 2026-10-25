import { createRow, type Row, type RowModel, type Table } from '@tanstack/react-table'

/** 表格的顶层记录模型；手动分组在展示层处理，不使用 subRows。输入记录必须不可变。 */
export function getSharedFlatRowModel<T>() {
  return (table: Table<T>): (() => RowModel<T>) => {
    let previousData: T[] | undefined
    let previousColumns = table.options.columns
    let model: RowModel<T> = { rows: [], flatRows: [], rowsById: {} }
    return () => {
      const data = table.options.data
      if (data === previousData) return model
      const columnsChanged = previousColumns !== table.options.columns
      const rows: Row<T>[] = []
      const rowsById: Record<string, Row<T>> = {}
      for (let index = 0; index < data.length; index++) {
        const original = data[index]
        const id = table.options.getRowId?.(original, index) ?? String(index)
        const previous = model.rowsById[id]
        // 行方法捕获 original 和 index；移动、重命名和内容变化都必须创建新行。
        const row = previous?.original === original && previous.index === index
          ? previous
          : createRow(table, id, original, index, 0)
        if (row === previous && columnsChanged) {
          // 列访问器可能随 schema/显示配置改变，复用行不能保留旧的排序和过滤值。
          row._valuesCache = {}
          row._uniqueValuesCache = {}
        }
        rows.push(row)
        rowsById[id] = row
      }
      previousData = data
      previousColumns = table.options.columns
      model = { rows, flatRows: rows, rowsById }
      table._autoResetPageIndex()
      return model
    }
  }
}
