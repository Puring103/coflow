import type { FrontendPluginBundle } from '../bindings/FrontendPluginBundle'
import type { FrontendPluginProjectState } from '../bindings/FrontendPluginProjectState'
import type { FrontendPluginState } from '../bindings/FrontendPluginState'
import { invokeCommand } from './tauriEnv'

export async function installFrontendPlugin(manifestPath: string): Promise<FrontendPluginBundle> {
  return invokeCommand<FrontendPluginBundle>('install_frontend_plugin', { manifestPath })
}

export async function listFrontendPlugins(): Promise<FrontendPluginState> {
  return invokeCommand<FrontendPluginState>('list_frontend_plugins')
}

export async function uninstallFrontendPlugin(id: string): Promise<void> {
  return invokeCommand<void>('uninstall_frontend_plugin', { id })
}

export async function installProjectFrontendPlugin(sessionId: number, manifestPath: string): Promise<FrontendPluginBundle> {
  return invokeCommand<FrontendPluginBundle>('install_project_frontend_plugin', { sessionId, manifestPath })
}

export async function listProjectFrontendPlugins(sessionId: number): Promise<FrontendPluginProjectState> {
  return invokeCommand<FrontendPluginProjectState>('list_project_frontend_plugins', { sessionId })
}

export async function uninstallProjectFrontendPlugin(sessionId: number, id: string): Promise<void> {
  return invokeCommand<void>('uninstall_project_frontend_plugin', { sessionId, id })
}

export async function setProjectFrontendPluginEnabled(sessionId: number, id: string, enabled: boolean): Promise<void> {
  return invokeCommand<void>('set_project_frontend_plugin_enabled', { sessionId, id, enabled })
}

