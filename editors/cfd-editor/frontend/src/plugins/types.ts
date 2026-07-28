import type { FieldValue } from '../wire'
import type { PluginSchemaType } from '../bindings/PluginSchemaType'
import type { RecordRow } from '../bindings/RecordRow'

/** Places where the host currently exposes a value renderer. */
export type FieldRenderSurface = 'table-cell' | 'record-foldout-header'

export interface FieldValueTarget {
  kind: 'field-value'
  /** The declared CFD type handled by this renderer, e.g. ChemicalExpression. */
  type: string
  surfaces: FieldRenderSurface[]
}

export interface ReadRenderContext {
  value: FieldValue
  type: string
  /** Whether the declared type has an outer nullable wrapper. */
  nullable: boolean
  surface: FieldRenderSurface
}

export interface PluginOutlet {
  element: HTMLElement
  signal: AbortSignal
  replace(content: Node | string): void
}

export interface FieldRenderer {
  id: string
  target: FieldValueTarget
  mount(context: ReadRenderContext, outlet: PluginOutlet): void | (() => void)
}

export interface ExtensionHost {
  apiVersion: 1
  schema: {
    getTypes(): Promise<PluginSchemaType[]>
  }
  records: {
    /** Returns rows whose actual type exactly matches `typeName`, across source files. */
    getByType(typeName: string): Promise<RecordRow[]>
  }
  renderers: {
    register(renderer: FieldRenderer): () => void
  }
}

export interface ExtensionDefinition {
  dispose?(): void
}

export type ExtensionActivate = (host: ExtensionHost) => ExtensionDefinition | void | Promise<ExtensionDefinition | void>

export interface ReadPlugin {
  id: string
  name: string
  description: string
  version: string
  renderers: FieldRenderer[]
  dispose?: () => void
  origin: 'global' | 'project'
  manifestPath: string
  enabled: boolean
}
