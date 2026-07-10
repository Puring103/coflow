import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from 'react'
import type { CreateRecordDraft } from '../bindings/CreateRecordDraft'
import type { FieldValue } from '../wire'
import * as api from '../api'
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
  sessionId: number | null
  openObjectDraft: (opts: OpenOptions) => void
}

const Ctx = createContext<ObjectDraftHostValue | null>(null)

/** Provides an imperative `openObjectDraft` for any descendant to launch the
 *  shared object-draft dialog. Rendered once near the top of the app so the
 *  overlay always paints above the record view. */
export function ObjectDraftHost({
  sessionId,
  children,
}: {
  sessionId: number | null
  children: ReactNode
}) {
  const [request, setRequest] = useState<(OpenOptions & { currentType: string }) | null>(null)

  const openObjectDraft = useCallback((opts: OpenOptions) => {
    setRequest({ ...opts, currentType: opts.actualType })
  }, [])

  const loadDraft = useCallback(async (typeName: string): Promise<CreateRecordDraft> => {
    if (sessionId === null) throw new Error('未打开会话')
    return api.createRecordDraft(sessionId, typeName)
  }, [sessionId])

  const value = useMemo<ObjectDraftHostValue>(
    () => ({ sessionId, openObjectDraft }),
    [sessionId, openObjectDraft],
  )

  return (
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
  )
}

export function useObjectDraft(): ObjectDraftHostValue {
  const ctx = useContext(Ctx)
  if (!ctx) throw new Error('useObjectDraft used outside ObjectDraftHost')
  return ctx
}
