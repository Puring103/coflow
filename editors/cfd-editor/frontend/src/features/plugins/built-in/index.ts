import type { BuiltInPluginDefinition } from '../runtime'
import { projectSearchPlugin } from './projectSearch'

export const builtInPlugins: readonly BuiltInPluginDefinition[] = [
  projectSearchPlugin,
]

export { searchMockRecords } from './projectSearch'
