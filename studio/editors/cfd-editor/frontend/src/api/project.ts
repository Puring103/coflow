import type { ProjectBootstrap } from '../bindings/ProjectBootstrap'
import type { ProjectDiff } from '../bindings/ProjectDiff'
import { invokeCommand } from './tauriEnv'

export async function loadProject(yamlPath: string): Promise<ProjectBootstrap> {
  return invokeCommand<ProjectBootstrap>('load_project', { yamlPath })
}

export async function addProjectInput(sessionId: number, kind: 'schema' | 'data', path: string): Promise<ProjectBootstrap> {
  return invokeCommand<ProjectBootstrap>('add_project_input', { sessionId, kind, path })
}

export async function createProjectFile(sessionId: number, kind: 'schema' | 'data', parentPath: string, fileName: string): Promise<ProjectBootstrap> {
  return invokeCommand<ProjectBootstrap>('create_project_file', { sessionId, kind, parentPath, fileName })
}

export async function deleteProjectEntry(sessionId: number, path: string): Promise<ProjectBootstrap> {
  return invokeCommand<ProjectBootstrap>('delete_project_entry', { sessionId, path })
}

export async function initProject(dir: string): Promise<ProjectBootstrap> {
  return invokeCommand<ProjectBootstrap>('init_project', { dir })
}

export async function reloadSession(sessionId: number): Promise<ProjectBootstrap> {
  return invokeCommand<ProjectBootstrap>('reload_session', { sessionId })
}

export async function closeSession(sessionId: number): Promise<void> {
  return invokeCommand('close_session', { sessionId })
}

export async function checkProject(sessionId: number): Promise<string> {
  return invokeCommand<string>('check_project', { sessionId })
}

export async function generateProjectCode(sessionId: number): Promise<string> {
  return invokeCommand<string>('generate_project_code', { sessionId })
}

export async function codegenProjectStatus(sessionId: number): Promise<boolean> {
  return invokeCommand<boolean>('codegen_project_status', { sessionId })
}

export async function getProjectDiff(sessionId: number): Promise<ProjectDiff> {
  return invokeCommand<ProjectDiff>('get_project_diff', { sessionId })
}

export async function openSourceFile(sessionId: number, filePath: string): Promise<void> {
  return invokeCommand('open_source_file', { sessionId, filePath })
}

