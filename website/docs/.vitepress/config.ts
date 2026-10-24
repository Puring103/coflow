import { defineConfig } from 'vitepress'
import { withMermaid } from 'vitepress-plugin-mermaid'
import cfdGrammar from '../../../editors/vscode-coflow/syntaxes/cfd.tmLanguage.json'
import cftGrammar from '../../../editors/vscode-coflow/syntaxes/cft.tmLanguage.json'

const pagesBase = process.env.VITEPRESS_BASE ?? (process.env.GITHUB_ACTIONS ? '/coflow/' : '/')
const projectUrl = 'https://github.com/Puring103/coflow'

export default withMermaid(defineConfig({
  title: 'Coflow',
  description: 'AI 原生的数据配置工作流。人与AI共同配置；同一份数据，多种视图，自由切换。',
  base: pagesBase,
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
      provider: 'local'
    },
    socialLinks: [
      { icon: 'github', link: projectUrl }
    ],
    footer: {
      message: 'Released under the Apache-2.0 License.',
      copyright: 'Copyright © 2026 Coflow contributors'
    },
    nav: [
      { text: '首页', link: '/' },
      { text: '文档', link: '/docs/' }
    ],
    sidebar: {
      '/docs/': [
        { text: '总览', link: '/docs/' },
        { text: '快速开始', link: '/docs/guide/install' },
        {
          text: '特性',
          items: [
            { text: '项目配置', link: '/docs/reference/01-project-config' },
            { text: 'CFT', link: '/docs/reference/03-language/01-cft' },
            { text: 'CFD', link: '/docs/reference/03-language/02-cfd' },
            {
              text: '代码生成',
              items: [
                { text: 'C#', link: '/docs/reference/07-codegen/01-csharp' }
              ]
            },
            { text: 'CLI 命令', link: '/docs/reference/08-cli' },
            { text: '编辑器插件', link: '/docs/reference/13-plugins' },
            { text: '本地化与维度', link: '/docs/reference/10-localization' },
            {
              text: '诊断',
              items: [
                { text: '诊断与问题定位', link: '/docs/reference/09-diagnostics/01-diagnostics' },
                { text: '错误码索引', link: '/docs/reference/09-diagnostics/02-codes' }
              ]
            }
          ]
        }
      ]
    }
  }
}))
