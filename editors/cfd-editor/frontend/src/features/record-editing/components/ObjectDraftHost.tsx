import { useMemo, type ReactNode } from 'react'
import type { EditorLookupController } from '../../workspace/state/editorLookups'
import { EditorLookupContext, EditorNavigationContext } from '../../workspace/context/editContext'

/** 为值编辑组件提供 generation-aware lookup 与引用跳转能力。 */
export function ObjectDraftHost({
  lookups,
  generationKey,
  sessionId = 0,
  onOpenReference,
  onLookupError,
  children,
}: {
  lookups: EditorLookupController
  generationKey: string
  sessionId?: number
  onOpenReference: (targetType: string, recordId: string) => void
  onLookupError?: (key: string) => void
  children: ReactNode
}) {
  const lookupAccess = useMemo(() => ({
    sessionId,
    cachedEnumVariants: (enumName: string) => lookups.cachedEnumVariants(enumName),
    cachedRefTargets: (targetType: string) => lookups.cachedRefTargets(targetType),
    loadEnumVariants: (enumName: string) => lookups.loadEnumVariants(enumName),
    loadRefTargets: (targetType: string) => lookups.loadRefTargets(targetType),
    makeDefaultObject: (typeName: string) => lookups.makeDefaultObject(typeName),
    createRecordDraft: (actualType: string) => lookups.createRecordDraft(actualType),
  }), [generationKey, lookups, sessionId])

  return (
    <EditorLookupContext.Provider value={lookupAccess}>
      <EditorNavigationContext.Provider value={{ openReference: onOpenReference, focusLookupError: onLookupError }}>
        {children}
      </EditorNavigationContext.Provider>
    </EditorLookupContext.Provider>
  )
}
