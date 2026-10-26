import type { FileRecords } from '../bindings/FileRecords'
import type { FileRecordsTransfer } from '../bindings/FileRecordsTransfer'
import type { FieldAnnotation } from '../bindings/FieldAnnotation'

/** 描述按子节点在前的顺序恢复一次；所有引用同一描述的字段共享对象身份。 */
export function decodeFileRecords(transfer: FileRecordsTransfer): FileRecords {
  const annotations: FieldAnnotation[] = []
  for (const descriptor of transfer.annotations) {
    annotations.push({
      ...descriptor.metadata,
      ...(descriptor.item === null ? {} : { item_annotation: annotations[descriptor.item] }),
      ...(descriptor.key === null ? {} : { key_annotation: annotations[descriptor.key] }),
      ...(Object.keys(descriptor.children).length ? { children: Object.fromEntries(Object.entries(descriptor.children).map(([name, index]) => [name, annotations[index!]])) } : {}),
    })
  }
  return { ...transfer.snapshot, records: transfer.snapshot.records.map((row, rowIndex) => ({
    ...row,
    fields: row.fields.map((field, fieldIndex) => {
      const index = transfer.annotation_ids[rowIndex][fieldIndex]
      return index === null ? field : { ...field, annotation: annotations[index] }
    }),
  })) }
}
