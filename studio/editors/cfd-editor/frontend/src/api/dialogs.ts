import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { isTauri } from './tauriEnv'

/** 文件选择唯一入口：四处 openDialog 收敛于此。 */
export async function pickFile(options: Parameters<typeof openDialog>[0]): Promise<string | null> {
  if (!isTauri) return null
  const path = await openDialog(options)
  return typeof path === 'string' ? path : null
}

export async function pickProjectYaml(): Promise<string | null> {
  return pickFile({
    multiple: false,
    filters: [{ name: 'Coflow Project', extensions: ['yaml', 'yml'] }],
  })
}

export async function pickProjectDirectory(): Promise<string | null> {
  return pickFile({
    multiple: false,
    directory: true,
  })
}

export async function pickProjectInput(kind: 'schema' | 'data', directory: boolean): Promise<string | null> {
  return pickFile({
    multiple: false,
    directory,
    ...(directory ? {} : { filters: [{ name: kind === 'schema' ? 'Coflow Schema' : 'Coflow Data', extensions: [kind === 'schema' ? 'cft' : 'cfd'] }] }),
  })
}

export async function pickFrontendPluginManifest(): Promise<string | null> {
  return pickFile({
    multiple: false,
    filters: [{ name: 'CFD Editor Plugin', extensions: ['json'] }],
  })
}
