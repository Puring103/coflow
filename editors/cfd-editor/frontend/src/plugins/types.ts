import type { BatchWriteFieldInput } from '../bindings/BatchWriteFieldInput'
import type { CollectionEdit } from '../bindings/CollectionEdit'
import type { FileRecords } from '../bindings/FileRecords'
import type { PluginSchemaType } from '../bindings/PluginSchemaType'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { RecordRow } from '../bindings/RecordRow'
import type { FieldPathSegment, FieldValue } from '../wire'

export type PluginOrigin = 'built-in' | 'global' | 'project'
export type PluginPresentationSlot = 'cell' | 'inspector' | 'summary'
export type PluginSidebarIcon = 'extensions' | 'search'

export interface PluginIdentity {
  sessionId: number
  revision: number
}

export interface PluginOutlet {
  element: HTMLElement
  signal: AbortSignal
  replace(content: Node | string): void
}

export interface PluginUiInstance<TContext> {
  update?(context: TContext): void
  dispose?(): void
}

export type PluginMountResult<TContext> = void | (() => void) | PluginUiInstance<TContext>

export interface PluginContributionBase {
  id: string
  title: string
}

export interface PluginPageContext {
  identity: PluginIdentity | null
  pageId: string
  openPage(pageId: string): void
  closePage(pageId?: string): void
}

export interface PluginPageContribution extends PluginContributionBase {
  mount(context: PluginPageContext, outlet: PluginOutlet): PluginMountResult<PluginPageContext>
}

export interface PluginRecordData {
  filePath: string
  coordinate: RecordCoordinate
  fields?: RecordRow['fields']
}

export interface PluginViewContext {
  identity: PluginIdentity
  filePath: string
  typeName: string
  records: PluginRecordData[]
  readOnly: boolean
  openPage(pageId: string): void
}

export interface PluginViewContribution extends PluginContributionBase {
  types: string[]
  includeFieldValues?: boolean
  readOnly?: boolean
  default?: boolean
  mount(context: PluginViewContext, outlet: PluginOutlet): PluginMountResult<PluginViewContext>
}

export interface PluginSidebarContext {
  identity: PluginIdentity | null
  sidebarId: string
  openPage(pageId: string): void
  openRecord(filePath: string, coordinate: RecordCoordinate, fieldPath?: string | null): void
  closeSidebar(): void
}

export interface PluginSidebarContribution extends PluginContributionBase {
  icon?: PluginSidebarIcon
  mount(context: PluginSidebarContext, outlet: PluginOutlet): PluginMountResult<PluginSidebarContext>
}

export interface PluginPresentationContext {
  identity: PluginIdentity
  filePath: string
  coordinate: RecordCoordinate
  actualType: string
  fieldPath: FieldPathSegment[] | null
  declaredType: string
  value?: FieldValue
}

interface PluginPresentationBase extends Omit<PluginContributionBase, 'title'> {
  types: string[]
  slot: PluginPresentationSlot
  default?: boolean
}

export interface PluginMountedPresentation extends PluginPresentationBase {
  slot: 'cell' | 'inspector'
  mount(context: PluginPresentationContext, outlet: PluginOutlet): PluginMountResult<PluginPresentationContext>
}

export interface PluginSummaryPresentation extends PluginPresentationBase {
  slot: 'summary'
  render(context: PluginPresentationContext): string
}

export type PluginPresentationContribution = PluginMountedPresentation | PluginSummaryPresentation

export interface PluginActiveContext {
  identity: PluginIdentity | null
  filePath: string | null
  typeName: string | null
  selection: PluginSelection | null
  surface: 'editor' | 'page' | 'sidebar'
}

export type PluginSelection =
  | { kind: 'record'; filePath: string; coordinates: RecordCoordinate[] }
  | { kind: 'field'; filePath: string; coordinate: RecordCoordinate; fieldPath: FieldPathSegment[] }

export interface PluginCommandContext extends PluginActiveContext {
  openPage(pageId: string): void
}

export interface PluginCommandContribution extends PluginContributionBase {
  run(context: PluginCommandContext): void | Promise<void>
}

export interface PluginKeybindingContribution {
  command: string
  key: string
  when?(context: PluginActiveContext): boolean
}

export interface PluginEventMap {
  project: { identity: PluginIdentity | null }
  data: PluginIdentity
  selection: { selection: PluginSelection | null }
  surface: {
    kind: 'document' | 'view' | 'sidebar'
    id: string | null
    filePath?: string
    typeName?: string
  }
}

export interface PluginSnapshot<T> extends PluginIdentity {
  data: T
}

export type PluginSearchMode = 'key' | 'full_text'

export interface PluginSearchHit {
  filePath: string
  coordinate: RecordCoordinate
  fieldPath: string | null
  preview: string | null
}

export interface PluginSearchResults {
  hits: PluginSearchHit[]
  truncated: boolean
}

export type PluginMutationRequest =
  | { kind: 'write_field'; filePath: string; coordinate: RecordCoordinate; fieldPath: FieldPathSegment[]; value: FieldValue }
  | { kind: 'write_fields'; filePath: string; coordinates: RecordCoordinate[]; fieldPath: FieldPathSegment[]; value: FieldValue }
  | { kind: 'write_field_batch'; filePath: string; writes: BatchWriteFieldInput[] }
  | { kind: 'edit_collection'; filePath: string; coordinate: RecordCoordinate; fieldPath: FieldPathSegment[]; edit: CollectionEdit }
  | { kind: 'rename_record'; filePath: string; coordinate: RecordCoordinate; newKey: string }
  | { kind: 'insert_record'; filePath: string; recordKey: string; actualType: string; fields: FieldValue }
  | { kind: 'delete_record'; filePath: string; coordinate: RecordCoordinate }
  | { kind: 'swap_records'; filePath: string; first: RecordCoordinate; second: RecordCoordinate }
  | { kind: 'move_record'; filePath: string; coordinate: RecordCoordinate; targetIndex: number }
  | { kind: 'transfer_record'; sourceFile: string; destinationFile: string; coordinate: RecordCoordinate; targetIndex: number }

export interface PluginDataApi {
  current(): PluginIdentity | null
  getSchema(): Promise<PluginSnapshot<PluginSchemaType[]>>
  getRecordsByType(typeName: string, options?: { includeFieldValues?: boolean }): Promise<PluginSnapshot<PluginRecordData[]>>
  getRecord(filePath: string, coordinate: RecordCoordinate): Promise<PluginSnapshot<PluginRecordData | null>>
  getField(filePath: string, coordinate: RecordCoordinate, fieldPath: FieldPathSegment[]): Promise<PluginSnapshot<FieldValue | null>>
  searchRecords(query: string, options?: { mode?: PluginSearchMode; limit?: number }): Promise<PluginSnapshot<PluginSearchResults>>
  mutate(request: PluginMutationRequest): Promise<PluginIdentity>
}

export interface PluginRegistrationApi {
  page(contribution: PluginPageContribution): () => void
  view(contribution: PluginViewContribution): () => void
  sidebar(contribution: PluginSidebarContribution): () => void
  presentation(contribution: PluginPresentationContribution): () => void
  command(contribution: PluginCommandContribution): () => void
  keybinding(contribution: PluginKeybindingContribution): () => void
  openPage(pageId: string): void
  openSidebar(sidebarId: string): void
}

export interface PluginEventApi {
  on<K extends keyof PluginEventMap>(event: K, listener: (payload: PluginEventMap[K]) => void | Promise<void>): () => void
}

export interface EditorPluginHost {
  register: PluginRegistrationApi
  events: PluginEventApi
  data: PluginDataApi
}

export interface EditorPluginDefinition {
  dispose?(): void
}

export type EditorPluginActivate = (
  host: EditorPluginHost,
) => EditorPluginDefinition | void | Promise<EditorPluginDefinition | void>

export interface PluginMetadata {
  id: string
  name: string
  description: string
  version: string
  origin: PluginOrigin
  manifestPath: string
  enabled: boolean
}

export interface PluginProjectDefaults {
  views: Record<string, string | undefined>
  presentations: Record<string, Partial<Record<PluginPresentationSlot, string>> | undefined>
}

export interface PluginDataBridge {
  currentIdentity(): PluginIdentity | null
  getSchema(sessionId: number): Promise<PluginSchemaType[]>
  getRecordsByType(sessionId: number, typeName: string): Promise<RecordRow[]>
  getFileRecords(sessionId: number, filePath: string): Promise<FileRecords>
  searchRecords(sessionId: number, query: string, mode: PluginSearchMode, limit: number): Promise<PluginSnapshot<PluginSearchResults>>
  mutate(request: PluginMutationRequest): Promise<void>
}

export interface PluginUiBridge {
  openPage(pluginId: string, pageId: string): void
  closePage(pluginId: string, pageId?: string): void
  openSidebar(pluginId: string, sidebarId: string): void
  closeSidebar(pluginId: string, sidebarId: string): void
  openRecord(filePath: string, coordinate: RecordCoordinate, fieldPath?: string | null): void
  reportError(message: string): void
}
