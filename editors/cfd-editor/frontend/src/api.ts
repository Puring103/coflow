import { invoke } from "@tauri-apps/api/core";
import type {
  ProjectSnapshot, FileRecords, RecordRow, GraphData,
  FileTreeNode, FieldValue, FieldPathSegment, DiagnosticItem, RecordBrief, FieldSchema
} from "./bindings";

export const api = {
  loadProject: (yamlPath: string) =>
    invoke<ProjectSnapshot>("load_project", { yamlPath }),

  getFileRecords: (sessionId: number, filePath: string) =>
    invoke<FileRecords>("get_file_records", { sessionId, filePath }),

  getRecord: (sessionId: number, filePath: string, recordKey: string) =>
    invoke<RecordRow>("get_record", { sessionId, filePath, recordKey }),

  getGraph: (sessionId: number, filePath: string, expandedKeys?: string[]) =>
    invoke<GraphData>("get_graph", { sessionId, filePath, expandedKeys: expandedKeys ?? [] }),

  closeSession: (sessionId: number) =>
    invoke<void>("close_session", { sessionId }),

  writeField: (
    sessionId: number,
    filePath: string,
    recordKey: string,
    fieldPath: FieldPathSegment[],
    newValue: FieldValue
  ) => invoke<void>("write_field", { sessionId, filePath, recordKey, fieldPath, newValue }),

  createRecord: (sessionId: number, filePath: string, key: string, typeName: string) =>
    invoke<RecordRow>("create_record", { sessionId, filePath, key, typeName }),

  deleteRecord: (sessionId: number, filePath: string, recordKey: string) =>
    invoke<void>("delete_record", { sessionId, filePath, recordKey }),

  createFile: (sessionId: number, relPath: string) =>
    invoke<FileTreeNode>("create_file", { sessionId, relPath }),

  deleteFile: (sessionId: number, relPath: string) =>
    invoke<void>("delete_file", { sessionId, relPath }),

  getDiagnostics: (sessionId: number) =>
    invoke<DiagnosticItem[]>("get_diagnostics", { sessionId }),

  renameRecord: (sessionId: number, filePath: string, oldKey: string, newKey: string) =>
    invoke<void>("rename_record", { sessionId, filePath, oldKey, newKey }),

  getAllTypeNames: (sessionId: number) =>
    invoke<string[]>("get_all_type_names", { sessionId }),

  renameFile: (sessionId: number, oldRelPath: string, newRelPath: string) =>
    invoke<void>("rename_file", { sessionId, oldRelPath, newRelPath }),

  getEnumVariants: (sessionId: number, enumName: string) =>
    invoke<string[]>("get_enum_variants", { sessionId, enumName }),

  getRefTargets: (sessionId: number, expectedType: string) =>
    invoke<string[]>("get_ref_targets", { sessionId, expectedType }),

  duplicateRecord: (sessionId: number, filePath: string, srcKey: string, newKey: string) =>
    invoke<RecordRow>("duplicate_record", { sessionId, filePath, srcKey, newKey }),

  getAllRecordsBrief: (sessionId: number) =>
    invoke<RecordBrief[]>("get_all_records_brief", { sessionId }),

  getFieldSchemas: (sessionId: number, typeName: string) =>
    invoke<FieldSchema[]>("get_field_schemas", { sessionId, typeName }),

  getRecordSource: (sessionId: number, filePath: string, recordKey: string) =>
    invoke<string>("get_record_source", { sessionId, filePath, recordKey }),
};
