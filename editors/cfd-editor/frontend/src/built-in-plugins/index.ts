import type { BuiltInPluginDefinition } from '../plugins'
import { projectSearchPlugin } from './projectSearch'

export const builtInPlugins: readonly BuiltInPluginDefinition[] = [
  projectSearchPlugin,
]

export { searchMockRecords } from './projectSearch'
