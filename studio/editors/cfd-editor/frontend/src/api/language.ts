import type { FlatDiagnostic } from '../bindings/FlatDiagnostic'
import type { FunctionDocumentState } from '../bindings/FunctionDocumentState'
import type { LanguageCompletion } from '../bindings/LanguageCompletion'
import type { LanguageDocumentState } from '../bindings/LanguageDocumentState'
import type { LanguageFormattingResult } from '../bindings/LanguageFormattingResult'
import type { LanguagePosition } from '../bindings/LanguagePosition'
import type { ProjectBootstrap } from '../bindings/ProjectBootstrap'
import { invokeCommand } from './tauriEnv'

export async function readSourceText(sessionId: number, filePath: string): Promise<string> {
  return invokeCommand<string>('read_source_text', { sessionId, filePath })
}

export async function highlightSourceSnapshot(sessionId: number, filePath: string, source: string): Promise<LanguageDocumentState> {
  return invokeCommand<LanguageDocumentState>('highlight_source_snapshot', { sessionId, filePath, source })
}

export async function syncLanguageDocument(
  sessionId: number,
  filePath: string,
  source: string,
  version: number,
): Promise<LanguageDocumentState> {
  return invokeCommand<LanguageDocumentState>('sync_language_document', {
    sessionId,
    filePath,
    source,
    version,
  })
}

export async function validateSourceText(
  sessionId: number,
  filePath: string,
  source: string,
): Promise<FlatDiagnostic[]> {
  return invokeCommand<FlatDiagnostic[]>('validate_source_text', { sessionId, filePath, source })
}

export async function formatLanguageDocument(
  sessionId: number,
  filePath: string,
  source: string,
  version: number,
): Promise<LanguageFormattingResult> {
  return invokeCommand<LanguageFormattingResult>('format_language_document', {
    sessionId,
    filePath,
    source,
    version,
  })
}

export async function completeLanguageDocument(
  sessionId: number,
  filePath: string,
  source: string,
  version: number,
  position: LanguagePosition,
): Promise<LanguageCompletion[]> {
  return invokeCommand<LanguageCompletion[]>('complete_language_document', {
    sessionId,
    filePath,
    source,
    version,
    position,
  })
}

export async function closeLanguageDocument(sessionId: number, filePath: string): Promise<void> {
  return invokeCommand('close_language_document', { sessionId, filePath })
}

export async function functionDocument(
  sessionId: number,
  source: string,
  body?: string,
): Promise<FunctionDocumentState> {
  return invokeCommand<FunctionDocumentState>('function_document', { sessionId, source, body })
}

export async function writeSourceText(
  sessionId: number,
  filePath: string,
  source: string,
  expectedSource: string,
): Promise<ProjectBootstrap> {
  return invokeCommand<ProjectBootstrap>('write_source_text', { sessionId, filePath, source, expectedSource })
}

