import type { FileRecords } from '../bindings/index'

/**
 * Decide whether the front-end is allowed to edit fields in a file.
 *
 * Two call shapes:
 * - With a loaded `FileRecords`: consult its `capabilities.can_edit_field`,
 *   the back-end's authoritative answer.
 * - With a path string (e.g. from a graph node where the records aren't
 *   loaded): fall back to extension-based heuristics so the UI can still
 *   render the right affordances. The extension list mirrors the
 *   provider ids in the Tauri editor backend's session builder
 *   `default_provider_registry`.
 */
export function isEditableFile(input: FileRecords | string | null | undefined): boolean {
  if (!input) return false
  if (typeof input === 'string') {
    const lower = input.toLowerCase()
    return lower.endsWith('.cfd') || lower.endsWith('.xlsx')
  }
  return input.capabilities.can_edit_field
}
