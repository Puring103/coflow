import { invoke } from "@tauri-apps/api/core";
import type {
  ProjectSnapshot, FileRecords, RecordRow, GraphData,
  FileTreeNode, FieldValue, FieldPathSegment
} from "./bindings";

export const api = {
  loadProject: (yamlPath: string) =>
    invoke<ProjectSnapshot>("load_project", { yamlPath }),

  getFileRecords: (sessionId: number, filePath: string) =>
    invoke<FileRecords>("get_file_records", { sessionId, filePath }),

  getRecord: (sessionId: number, filePath: string, recordKey: string) =>
    invoke<RecordRow>("get_record", { sessionId, filePath, recordKey }),

  getGraph: (sessionId: number, filePath: string) =>
    invoke<GraphData>("get_graph", { sessionId, filePath }),

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
};
