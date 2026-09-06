import { useEffect, useRef, useState } from 'react'
import type { ProjectBootstrap } from '../bindings/ProjectBootstrap'
import type { WorkspaceTab } from '../state/workspaceTabs'
import { Icon } from './Icon'

export const GIT_DIFF_TAB_ID = '__git_diff__'

export interface PluginPageTab {
  key: string
  title: string
}

interface Props {
  fileTypes: ProjectBootstrap['file_types'] | undefined
  workspaceTabs: WorkspaceTab[]
  activeWorkspaceTabId: string | null
  pluginTabs: PluginPageTab[]
  activePluginTabKey: string | null
  gitDiffOpen: boolean
  gitDiffActive: boolean
  activeWorkspaceReadOnly: boolean
  onActivate: (id: string) => void
  onCloseWorkspace: (id: string) => void
  onClosePlugin: (key: string) => void
  onCloseGitDiff: () => void
}

export function DocumentTabs({
  fileTypes,
  workspaceTabs,
  activeWorkspaceTabId,
  pluginTabs,
  activePluginTabKey,
  gitDiffOpen,
  gitDiffActive,
  activeWorkspaceReadOnly,
  onActivate,
  onCloseWorkspace,
  onClosePlugin,
  onCloseGitDiff,
}: Props) {
  const [menuOpen, setMenuOpen] = useState(false)
  const [overflowing, setOverflowing] = useState(false)
  const scrollRef = useRef<HTMLDivElement>(null)
  const menuRef = useRef<HTMLDivElement>(null)
  const visible = workspaceTabs.length > 0 || pluginTabs.length > 0 || gitDiffOpen

  useEffect(() => {
    if (!menuOpen) return
    const close = (event: MouseEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setMenuOpen(false)
    }
    window.addEventListener('mousedown', close)
    return () => window.removeEventListener('mousedown', close)
  }, [menuOpen])

  useEffect(() => {
    const activeId = gitDiffActive ? GIT_DIFF_TAB_ID : activePluginTabKey ?? activeWorkspaceTabId
    if (!activeId) return
    scrollRef.current
      ?.querySelector<HTMLElement>(`[data-tab-id="${CSS.escape(activeId)}"]`)
      ?.scrollIntoView({ inline: 'nearest', block: 'nearest' })
  }, [activePluginTabKey, activeWorkspaceTabId, gitDiffActive])

  useEffect(() => {
    const element = scrollRef.current
    if (!element) {
      setOverflowing(false)
      return
    }
    const check = () => setOverflowing(element.scrollWidth > element.clientWidth + 1)
    check()
    const observer = new ResizeObserver(check)
    observer.observe(element)
    for (const child of Array.from(element.children)) observer.observe(child)
    return () => observer.disconnect()
  }, [gitDiffOpen, pluginTabs, workspaceTabs])

  if (!visible) return null

  const activateFromMenu = (id: string) => {
    onActivate(id)
    setMenuOpen(false)
    requestAnimationFrame(() => {
      scrollRef.current
        ?.querySelector<HTMLElement>(`[data-tab-id="${CSS.escape(id)}"]`)
        ?.scrollIntoView({ inline: 'center', block: 'nearest' })
    })
  }

  return (
    <div className="document-tabs" role="tablist" aria-label="已打开内容">
      <div
        className="tab-scroll"
        ref={scrollRef}
        onWheel={event => {
          if (event.deltaX !== 0 || Math.abs(event.deltaY) < 1) return
          event.preventDefault()
          scrollRef.current!.scrollLeft += event.deltaY
        }}
      >
        {gitDiffOpen && (
          <DocumentTab
            id={GIT_DIFF_TAB_ID}
            label="Git Diff"
            icon="git-branch"
            active={gitDiffActive}
            locked
            onActivate={onActivate}
            onClose={onCloseGitDiff}
          />
        )}
        {workspaceTabs.map(tab => {
          const fileName = tab.filePath.split('/').pop() ?? tab.filePath
          const type = fileTypes?.[tab.filePath]?.find(option => option.name === tab.typeName)
          const label = type ? `${fileName} / ${type.display_name}` : fileName
          const title = type && type.display_name !== type.name
            ? `${tab.filePath} / ${type.display_name} (${type.name})`
            : `${tab.filePath}${tab.typeName ? ` / ${tab.typeName}` : ''}`
          return (
            <DocumentTab
              key={tab.id}
              id={tab.id}
              label={label}
              title={title}
              icon="file"
              active={!gitDiffActive && !activePluginTabKey && tab.id === activeWorkspaceTabId}
              locked={activeWorkspaceReadOnly && tab.id === activeWorkspaceTabId}
              onActivate={onActivate}
              onClose={onCloseWorkspace}
            />
          )
        })}
        {pluginTabs.map(tab => (
          <DocumentTab
            key={tab.key}
            id={tab.key}
            label={tab.title}
            icon="extensions"
            active={!gitDiffActive && tab.key === activePluginTabKey}
            onActivate={onActivate}
            onClose={onClosePlugin}
          />
        ))}
      </div>
      {overflowing && (
        <div className="tab-overflow" ref={menuRef}>
          <button
            type="button"
            className="tab-overflow-btn"
            onClick={() => setMenuOpen(open => !open)}
            aria-label="所有已打开标签"
            title="所有已打开标签"
            aria-expanded={menuOpen}
          >
            <Icon name="chevron-down" size={13} />
          </button>
          {menuOpen && (
            <div className="tab-overflow-menu" role="menu">
              {gitDiffOpen && (
                <OverflowItem id={GIT_DIFF_TAB_ID} label="Git Diff" icon="git-branch" active={gitDiffActive} onActivate={activateFromMenu} />
              )}
              {workspaceTabs.map(tab => {
                const fileName = tab.filePath.split('/').pop() ?? tab.filePath
                const type = fileTypes?.[tab.filePath]?.find(option => option.name === tab.typeName)
                return (
                  <OverflowItem
                    key={tab.id}
                    id={tab.id}
                    label={type ? `${fileName} / ${type.display_name}` : fileName}
                    icon="file"
                    active={!gitDiffActive && !activePluginTabKey && tab.id === activeWorkspaceTabId}
                    onActivate={activateFromMenu}
                  />
                )
              })}
              {pluginTabs.map(tab => (
                <OverflowItem
                  key={tab.key}
                  id={tab.key}
                  label={tab.title}
                  icon="extensions"
                  active={!gitDiffActive && tab.key === activePluginTabKey}
                  onActivate={activateFromMenu}
                />
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  )
}

interface TabProps {
  id: string
  label: string
  title?: string
  icon: 'file' | 'extensions' | 'git-branch'
  active: boolean
  locked?: boolean
  onActivate: (id: string) => void
  onClose: (id: string) => void
}

function DocumentTab({ id, label, title = label, icon, active, locked = false, onActivate, onClose }: TabProps) {
  return (
    <div
      className={`document-tab${active ? ' active' : ''}`}
      role="tab"
      aria-selected={active}
      tabIndex={active ? 0 : -1}
      data-tab-id={id}
      onClick={() => onActivate(id)}
      onKeyDown={event => {
        if (event.key === 'Delete') {
          event.preventDefault()
          onClose(id)
          return
        }
        onTabListKeyDown(event, onActivate)
      }}
      title={title}
    >
      <Icon name={icon} size={12} className="document-tab-icon" aria-hidden />
      <span className="document-tab-label">{label}</span>
      {locked && <Icon name="lock" size={10} className="document-tab-lock" aria-hidden />}
      <button
        type="button"
        className="document-tab-close"
        onClick={event => {
          event.stopPropagation()
          onClose(id)
        }}
        aria-label={`关闭 ${label}`}
        title="关闭标签"
      >
        <Icon name="close" size={11} aria-hidden />
      </button>
    </div>
  )
}

function OverflowItem({ id, label, icon, active, onActivate }: Omit<TabProps, 'title' | 'locked' | 'onClose'>) {
  return (
    <button
      type="button"
      role="menuitem"
      className={`tab-overflow-item${active ? ' active' : ''}`}
      onClick={() => onActivate(id)}
    >
      <Icon name={icon} size={12} aria-hidden />
      <span className="name">{label}</span>
    </button>
  )
}

function onTabListKeyDown(event: React.KeyboardEvent, activate: (id: string) => void) {
  if (event.key === 'Enter') {
    event.preventDefault()
    const id = (event.currentTarget as HTMLElement).dataset.tabId
    if (id) activate(id)
    return
  }
  if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight' && event.key !== 'Home' && event.key !== 'End') return
  const tabs = Array.from(
    event.currentTarget.closest('[role="tablist"]')?.querySelectorAll<HTMLElement>('[role="tab"][data-tab-id]') ?? [],
  )
  const index = tabs.indexOf(event.currentTarget as HTMLElement)
  if (index < 0 || tabs.length === 0) return
  event.preventDefault()
  const nextIndex = event.key === 'Home'
    ? 0
    : event.key === 'End'
      ? tabs.length - 1
      : index + (event.key === 'ArrowRight' ? 1 : -1)
  const tab = tabs[nextIndex]
  if (!tab) return
  tab.focus()
  const id = tab.dataset.tabId
  if (id) activate(id)
}
