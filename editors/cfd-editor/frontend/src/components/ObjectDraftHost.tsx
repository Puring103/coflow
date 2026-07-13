import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from 'react'
import type { CreateRecordDraft } from '../bindings/CreateRecordDraft'
import type { FieldValue } from '../wire'
import type { EditorLookupController } from '../state/editorLookups'
import { EditorLookupContext } from '../utils/editContext'
import { ObjectDraftDialog } from './ObjectDraftDialog'

interface OpenOptions {
  /** Dialog title, e.g. "创建 HealEffect" / "切换类型". */
  title: string
  /** Initial concrete type. */
  actualType: string
  /** When >=2, dialog shows a type <select> so the user can pivot. */
  polymorphicTypes?: string[]
  /** Called with the finalized CfdValue::Object; dialog closes afterwards. */
  onConfirm: (value: FieldValue) => void
  confirmLabel?: string
}

interface ObjectDraftHostValue {
  openObjectDraft: (opts: OpenOptions) => void
}

const Ctx = createContext<ObjectDraftHostValue | null>(null)

/** Provides an imperative `openObjectDraft` for any descendant to launch the
 *  shared object-draft dialog. Rendered once near the top of the app so the
 *  overlay always paints above the record view. */
export function ObjectDraftHost({
  lookups,
  generationKey,
  children,
}: {
  lookups: EditorLookupController
  generationKey: string
  children: ReactNode
}) {
  const [request, setRequest] = useState<(OpenOptions & { currentType: string }) | null>(null)

  const openObjectDraft = useCallback((opts: OpenOptions) => {
    setRequest({ ...opts, currentType: opts.actualType })
  }, [])

  const lookupAccess = useMemo(() => ({
    loadEnumVariants: (enumName: string) => lookups.loadEnumVariants(enumName),
    loadRefTargets: (targetType: string) => lookups.loadRefTargets(targetType),
    makeDefaultObject: (typeName: string) => lookups.makeDefaultObject(typeName),
    createRecordDraft: (actualType: string) => lookups.createRecordDraft(actualType),
  }), [generationKey, lookups])

  const loadDraft = useCallback(async (typeName: string): Promise<CreateRecordDraft> => {
    const result = await lookupAccess.createRecordDraft(typeName)
    if (result.ok) return result.value
    if (result.reason === 'failed') throw new Error(result.error ?? '创建记录草稿失败')
    throw new Error('编辑器 generation 已更新')
  }, [lookupAccess])

  const value = useMemo<ObjectDraftHostValue>(
    () => ({ openObjectDraft }),
    [openObjectDraft],
  )

  return (
    <EditorLookupContext.Provider value={lookupAccess}>
      <Ctx.Provider value={value}>
        {children}
        {request && (
          <ObjectDraftDialog
            title={request.title}
            actualType={request.currentType}
            polymorphicTypes={request.polymorphicTypes ?? []}
            onTypeChange={next => setRequest(r => r ? { ...r, currentType: next } : r)}
            onLoadDraft={loadDraft}
            confirmLabel={request.confirmLabel ?? '确定'}
            onConfirm={payload => {
              request.onConfirm(payload)
              setRequest(null)
            }}
            onClose={() => setRequest(null)}
          />
        )}
      </Ctx.Provider>
    </EditorLookupContext.Provider>
  )
}

export function useObjectDraft(): ObjectDraftHostValue {
  const ctx = useContext(Ctx)
  if (!ctx) throw new Error('useObjectDraft used outside ObjectDraftHost')
  return ctx
}
