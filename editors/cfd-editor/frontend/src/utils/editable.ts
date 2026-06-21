export function isEditableFile(path: string | null | undefined): boolean {
  return !!path && path.toLowerCase().endsWith('.cfd')
}
