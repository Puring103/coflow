import { defineConfig } from 'vitepress'
import { withMermaid } from 'vitepress-plugin-mermaid'

const pagesBase = process.env.VITEPRESS_BASE ?? (process.env.GITHUB_ACTIONS ? '/coflow/' : '/')
const projectUrl = 'https://github.com/wtlll/coflow'

export default withMermaid(defineConfig({
  title: 'Coflow',
  description: 'A typed, validated, AI-friendly game configuration workflow.',
  base: pagesBase,
  cleanUrls: true,
  mermaid: {
    theme: 'default',
    flowchart: {
      curve: 'basis'
    }
  },
  lastUpdated: true,
  themeConfig: {
    logo: '/logo.svg',
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
        {
          text: '文档',
          items: [
            { text: '概述', link: '/docs/' },
            { text: '快速开始', link: '/docs/guide/install' },
            { text: '引用', link: '/docs/reference/project-config' }
          ]
        },
        {
          text: '快速开始',
          collapsed: false,
          items: [
            { text: '安装', link: '/docs/guide/install' },
            { text: '策划视角', link: '/docs/guide/for-designers' },
            { text: '程序视角', link: '/docs/guide/for-programmers' },
            { text: '示例', link: '/docs/guide/examples' },
            { text: '最佳工作流', link: '/docs/guide/best-workflow' }
          ]
        },
        {
          text: '引用',
          collapsed: false,
          items: [
            { text: '项目配置', link: '/docs/reference/project-config' },
            { text: 'CFT 语法', link: '/docs/reference/cft' },
            { text: 'CFD 语法', link: '/docs/reference/cfd' },
            { text: 'CLI 命令', link: '/docs/reference/cli' },
            { text: '数据模型', link: '/docs/reference/data-model' },
            { text: 'Schema API', link: '/docs/reference/schema-api' },
            { text: '项目流水线', link: '/docs/reference/project-pipeline' },
            {
              text: '数据源与 Provider',
              collapsed: false,
              items: [
                { text: '概览', link: '/docs/reference/sources/' },
                { text: '单元格值语法', link: '/docs/reference/sources/cell-value' }
              ]
            },
            {
              text: '导出',
              collapsed: false,
              items: [
                { text: 'JSON', link: '/docs/reference/export/json' },
                { text: 'MessagePack', link: '/docs/reference/export/messagepack' }
              ]
            },
            {
              text: '代码生成',
              collapsed: false,
              items: [
                { text: 'C#', link: '/docs/reference/codegen/csharp' }
              ]
            },
            { text: '错误码', link: '/docs/reference/diagnostics' },
            { text: '本地化与维度', link: '/docs/reference/localization' },
            { text: '项目架构', link: '/docs/reference/architecture' }
          ]
        }
      ],
      '/examples/': [
        {
          text: '示例',
          items: [
            { text: 'RPG 示例', link: '/examples/rpg' }
          ]
        }
      ]
    }
  }
}))
