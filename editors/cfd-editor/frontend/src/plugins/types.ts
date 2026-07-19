import type { FieldValue } from '../wire'

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
  origin: 'local'
  manifestPath: string
}
