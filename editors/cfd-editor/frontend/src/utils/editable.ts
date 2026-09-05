import type { FileRecords } from '../bindings/FileRecords'
import type { WriterCapabilities } from '../bindings/WriterCapabilities'

/**
 * Decide whether the front-end is allowed to edit fields in a file.
 *
 * Two call shapes:
 * - With a loaded `FileRecords`: consult its `capabilities.can_edit_field`,
 *   the back-end's authoritative answer.
 * - With a path string (e.g. from a graph node where the records aren't
 *   loaded): fall back to extension-based heuristics so the UI can still
 *   render the right affordances. The extension list mirrors the
 *   CFD-only editor backend contract.
 */
export function isEditableFile(input: FileRecords | string | null | undefined): boolean {
  if (!input) return false
  if (typeof input === 'string') {
    const lower = input.toLowerCase()
    return lower.endsWith('.cfd')
  }
  return isEditableCapabilities(input.capabilities)
}

export function isEditableCapabilities(input: WriterCapabilities | null | undefined): boolean {
  return !!input?.can_edit_field
}

export function canRenameKey(input: FileRecords | null | undefined): boolean {
  return !!input?.capabilities.can_edit_key
}
