import { createContext, useContext } from 'react'
import type { CreateRecordDraft } from '../bindings/CreateRecordDraft'
import type { RefTarget } from '../bindings/RefTarget'
import type { FieldValue } from '../wire'
import type { LookupResult } from '../state/editorLookups'
import type { EnumVariantOption } from '../bindings/EnumVariantOption'

export interface EditorLookupAccess {
  loadEnumVariants: (enumName: string) => Promise<LookupResult<EnumVariantOption[]>>
  loadRefTargets: (targetType: string) => Promise<LookupResult<RefTarget[]>>
  makeDefaultObject: (typeName: string) => Promise<LookupResult<FieldValue>>
  createRecordDraft: (actualType: string) => Promise<LookupResult<CreateRecordDraft>>
}

export const EditorLookupContext = createContext<EditorLookupAccess | null>(null)

export function useEditorLookups(): EditorLookupAccess {
  const lookups = useContext(EditorLookupContext)
  if (!lookups) throw new Error('useEditorLookups used outside EditorLookupContext')
  return lookups
}
