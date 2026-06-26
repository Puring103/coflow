import type { FlatDiagnostic } from "./FlatDiagnostic";
import type { RecordCoordinate } from "./RecordCoordinate";
import type { RecordRow } from "./RecordRow";

export type RenameRecordOutcome = {
  row: RecordRow,
  diagnostics: Array<FlatDiagnostic>,
  renamed: RecordCoordinate,
};
