import { defineConfig } from 'vitepress'
import { withMermaid } from 'vitepress-plugin-mermaid'
import cfdGrammar from '../../../editors/vscode-coflow/syntaxes/cfd.tmLanguage.json'
import cftGrammar from '../../../editors/vscode-coflow/syntaxes/cft.tmLanguage.json'

const pagesBase = process.env.VITEPRESS_BASE ?? (process.env.GITHUB_ACTIONS ? '/coflow/' : '/')
const projectUrl = 'https://github.com/Puring103/coflow'

// 中英文使用相同页面路径，语言切换可直接定位当前文档的对应版本。
function navigation(english: boolean) {
  const prefix = english ? '/en' : ''
  const text = (zh: string, en: string) => english ? en : zh
  const page = (label: string, path = '') => ({ text: label, link: `${prefix}/docs/${path}` })
  return {
    nav: [
      { text: text('首页', 'Home'), link: `${prefix}/` },
      page(text('文档', 'Documentation'))
    ],
    sidebar: {
      [`${prefix}/docs/`]: [
        page(text('总览', 'Overview')),
        page(text('快速开始', 'Getting started'), 'guide/install'),
        page(text('编辑器上手', 'Editor guide'), 'guide/editors'),
        {
          text: text('特性', 'Features'),
          items: [
            page(text('项目配置', 'Project configuration'), 'reference/01-project-config'),
            page('CFT', 'reference/03-language/01-cft'),
            page('CFD', 'reference/03-language/02-cfd'),
            {
              text: text('代码生成', 'Code generation'),
              items: [page('C#', 'reference/07-codegen/01-csharp')]
            },
            page(text('CLI 命令', 'CLI commands'), 'reference/08-cli'),
            page(text('编辑器插件', 'Editor plugins'), 'reference/13-plugins'),
            page(text('本地化与维度', 'Localization and dimensions'), 'reference/10-localization'),
            {
              text: text('诊断', 'Diagnostics'),
              items: [
                page(text('诊断与问题定位', 'Diagnostics and troubleshooting'), 'reference/09-diagnostics/01-diagnostics'),
                page(text('错误码索引', 'Error code index'), 'reference/09-diagnostics/02-codes')
              ]
            }
          ]
        }
      ]
    }
  }
}

export default withMermaid(defineConfig({
  title: 'Coflow',
  description: 'AI 原生的数据配置工作流。人与AI共同配置；同一份数据，多种视图，自由切换。',
  base: pagesBase,
  locales: {
    root: {
      label: '简体中文',
      lang: 'zh-CN',
      themeConfig: {
        ...navigation(false),
        langMenuLabel: '切换语言',
        outline: { label: '本页目录' },
        docFooter: { prev: '上一页', next: '下一页' },
        lastUpdated: { text: '最后更新' },
        returnToTopLabel: '返回顶部',
        sidebarMenuLabel: '菜单',
        darkModeSwitchLabel: '主题',
        lightModeSwitchTitle: '切换到浅色模式',
        darkModeSwitchTitle: '切换到深色模式'
      }
    },
    en: {
      label: 'English',
      lang: 'en',
      description: 'An AI-native data configuration workflow. People and AI configure together. One source. Many views.',
      themeConfig: navigation(true)
    }
  },
  head: [
    ['link', { rel: 'icon', type: 'image/svg+xml', href: `${pagesBase}logo.svg` }]
  ],
  cleanUrls: true,
  markdown: {
    languages: [
      { ...cftGrammar, name: 'cft', aliases: ['CFT'] },
      { ...cfdGrammar, name: 'cfd', aliases: ['CFD'] }
    ]
  },
  mermaid: {
    theme: 'default',
    flowchart: {
      curve: 'basis'
    }
  },
  lastUpdated: true,
  themeConfig: {
    logo: { light: '/logo.svg', dark: '/logo-dark.svg', alt: 'Coflow' },
    search: {
      provider: 'local',
      options: {
        locales: {
          root: {
            translations: {
              button: { buttonText: '搜索文档', buttonAriaLabel: '搜索文档' },
              modal: {
                noResultsText: '没有找到相关结果',
                resetButtonTitle: '清除搜索',
                footer: { selectText: '选择', navigateText: '切换', closeText: '关闭' }
              }
            }
          }
        }
      }
    },
    socialLinks: [
      { icon: 'github', link: projectUrl }
    ],
    footer: {
      message: 'Released under the Apache-2.0 License.',
      copyright: 'Copyright © 2026 Coflow contributors'
    }
  }
}))
