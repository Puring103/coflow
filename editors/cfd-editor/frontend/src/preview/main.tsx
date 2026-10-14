import { StrictMode, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { QueryClientProvider } from '@tanstack/react-query'
import App from '../App'
import { queryClient } from '../queryClient'
import { MOCK_EDITOR_SETTINGS } from '../mock'
import { DEFAULT_TABLE_VIEW_ID, DEFAULT_RECORD_VIEW_ID, DEFAULT_SOURCE_VIEW_ID } from '../state/views'
import { defaultWorkspaceTab, workspaceToWire } from '../state/workspaceTabs'
import './layout-preview.css'

// 只准备真实配置格式的示例视图；渲染、编辑、历史和导航均交给正式 App。
const settings = structuredClone(MOCK_EDITOR_SETTINGS)
settings.views = {
  'data/item.cfd': {
    Item: [
      { id: 'preview-basic', name: '基础属性', kind: 'table', columns: ['name', 'quality', 'max_stack', 'stackable'],
        column_widths: {}, relations: [], fields: [], group_filter: null },
      { id: 'preview-potions', name: '药水分组', kind: 'table', columns: ['name', 'max_stack'],
        column_widths: {}, relations: [], fields: [], group_filter: 'mock-potions' },
    ],
  },
  'data/npc.cfd': {
    Npc: [
      { id: 'preview-relations', name: '掉落与奖励', kind: 'graph', columns: [], column_widths: {},
        relations: ['drops', 'reward_item'], fields: ['name', 'level', 'faction'], group_filter: null },
    ],
  },
}
settings.view_order = {
  'data/item.cfd': { Item: [DEFAULT_TABLE_VIEW_ID, 'preview-basic', 'preview-potions', DEFAULT_RECORD_VIEW_ID, DEFAULT_SOURCE_VIEW_ID] },
  'data/npc.cfd': { Npc: [DEFAULT_TABLE_VIEW_ID, 'preview-relations', DEFAULT_RECORD_VIEW_ID, DEFAULT_SOURCE_VIEW_ID] },
}
const tabs = [defaultWorkspaceTab('data/item.cfd', 'Item'), defaultWorkspaceTab('data/npc.cfd', 'Npc')]
settings.workspace = workspaceToWire(tabs, tabs[0].id)

function LayoutPreview() {
  const [compact, setCompact] = useState(true)
  const [fontSize, setFontSize] = useState('14')
  return (
    <div className="component-preview" style={{ '--preview-font-size': fontSize + 'px' } as React.CSSProperties}>
      <div className="component-preview-bar">
        <strong>布局草案 · 真实组件</strong>
        <div className="component-preview-variants" role="group" aria-label="对比布局">
          <button aria-pressed={compact} onClick={() => setCompact(true)}>草案布局</button>
          <button aria-pressed={!compact} onClick={() => setCompact(false)}>原布局</button>
        </div>
        <label>字号 <select value={fontSize} onChange={event => setFontSize(event.target.value)}>
          <option value="12">12px</option><option value="13">13px</option><option value="14">14px</option>
        </select></label>
        <span className="component-preview-note">示例项目 · 原生文件、构建与 Git 查询需桌面后端</span>
        <details className="component-preview-help">
          <summary>体验说明</summary>
          <div>
            <p>表格、记录、图谱、维度、复杂值、选择和撤销使用项目现有组件与状态逻辑。</p>
            <p>在“基础属性 / 药水分组”点击“视图选项”，可以编辑字段与顺序、分组；视图标签支持拖拽和右键菜单。</p>
            <p>切到 Npc 标签并选择“掉落与奖励”查看真实图谱；左侧维度入口可查看本地化。</p>
            <p>浏览器使用现有 Mock 数据。桌面文件操作、源码服务、构建、Git 和外部插件的后端行为须在正式集成时验证。</p>
            <p>对比布局保留当前编辑与选择；详情和问题面板可手动展开。</p>
          </div>
        </details>
      </div>
      <App layoutPreview={compact} browserSettings={settings} />
    </div>
  )
}

createRoot(document.getElementById('root')!).render(
  <StrictMode><QueryClientProvider client={queryClient}><LayoutPreview /></QueryClientProvider></StrictMode>,
)
