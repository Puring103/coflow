import type { FrontendPluginBundle, FrontendPluginProjectState } from '../../../api'
import { errorMessage } from '../../../wire'
import { builtInPlugins } from '../built-in'
import { replaceFrontendPlugins, setPluginProjectDefaults } from '../runtime'

export function emptyProjectPlugins(): FrontendPluginProjectState {
  return { plugins: [], errors: [], defaults: { views: {}, presentations: {} } }
}

/** 项目读取失败也完成作用域切换，不能让上一项目的插件和默认贡献留在新项目。 */
export async function replaceProjectPlugins(
  pending: Promise<FrontendPluginProjectState>,
  globalBundles: FrontendPluginBundle[],
  globalErrors: string[],
  isCurrent: () => boolean,
): Promise<{ projectLoaded: boolean; error: string | null } | null> {
  let state: FrontendPluginProjectState
  let loadError: string | null = null
  try { state = await pending } catch (cause) {
    loadError = errorMessage(cause)
    state = emptyProjectPlugins()
  }
  if (!isCurrent()) return null
  setPluginProjectDefaults(state.defaults)
  const activationErrors = await replaceFrontendPlugins(builtInPlugins, [...globalBundles, ...state.plugins])
  if (!isCurrent()) return null
  const errors = [...globalErrors, ...state.errors, ...activationErrors]
  if (loadError !== null) errors.unshift(`加载项目插件失败：${loadError}`)
  return {
    projectLoaded: loadError === null,
    error: errors.length ? errors.join('; ') : null,
  }
}
