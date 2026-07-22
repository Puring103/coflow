import { defineConfig } from 'vitepress'
import { withMermaid } from 'vitepress-plugin-mermaid'

const pagesBase = process.env.VITEPRESS_BASE ?? (process.env.GITHUB_ACTIONS ? '/coflow/' : '/')
const projectUrl = 'https://github.com/Puring103/coflow'

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
            { text: '设计理念', link: '/docs/' },
            { text: '快速开始', link: '/docs/guide/install' },
            { text: '引用', link: '/docs/reference/01-project-config' }
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
            { text: 'AI Agent Skills', link: '/docs/guide/ai-agent' },
            { text: '最佳工作流', link: '/docs/guide/best-workflow' }
          ]
        },
        {
          text: '引用',
          collapsed: false,
          items: [
            { text: '项目配置', link: '/docs/reference/01-project-config' },
            { text: '项目流水线', link: '/docs/reference/02-project-pipeline' },
            {
              text: '语言与语法',
              collapsed: false,
              items: [
                { text: 'CFT Schema', link: '/docs/reference/03-language/01-cft' },
                { text: 'Check 校验', link: '/docs/reference/03-language/04-check' },
                { text: 'CFD 文本数据', link: '/docs/reference/03-language/02-cfd' },
                { text: '表格单元格值', link: '/docs/reference/03-language/03-cell-value' }
              ]
            },
            {
              text: '数据源与 Provider',
              collapsed: false,
              items: [
                { text: '概览', link: '/docs/reference/04-sources/01-overview' },
                { text: '表格 Source', link: '/docs/reference/04-sources/02-table' },
                { text: 'Excel', link: '/docs/reference/04-sources/03-excel' },
                { text: 'CSV', link: '/docs/reference/04-sources/04-csv' },
                { text: 'Provider API', link: '/docs/reference/04-sources/06-provider-api' }
              ]
            },
            { text: '数据模型', link: '/docs/reference/05-data-model' },
            {
              text: '导出',
              collapsed: false,
              items: [
                { text: 'JSON', link: '/docs/reference/06-export/01-json' },
                { text: 'MessagePack', link: '/docs/reference/06-export/02-messagepack' }
              ]
            },
            {
              text: '代码生成',
              collapsed: false,
              items: [
                { text: 'C#', link: '/docs/reference/07-codegen/01-csharp' }
              ]
            },
            { text: 'CLI 命令', link: '/docs/reference/08-cli' },
            {
              text: '诊断',
              collapsed: false,
              items: [
                { text: '诊断模型', link: '/docs/reference/09-diagnostics/01-diagnostics' },
                { text: '错误码索引', link: '/docs/reference/09-diagnostics/02-codes' }
              ]
            },
            { text: '本地化与维度', link: '/docs/reference/10-localization' },
            { text: 'Schema API', link: '/docs/reference/11-schema-api' },
            { text: '项目架构', link: '/docs/reference/12-architecture' }
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
