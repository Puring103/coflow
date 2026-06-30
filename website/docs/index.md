---
layout: home

hero:
  name: Coflow
  text: 游戏配置生产工作流
  tagline: 面向游戏项目的强类型、强校验、可编辑、可渐进本地化、AI 友好的现代化配置工具链。
  actions:
    - theme: brand
      text: 快速开始
      link: /docs/guide/install
    - theme: alt
      text: 文档
      link: /docs/
    - theme: alt
      text: GitHub 项目
      link: https://github.com/wtlll/coflow

features:
  - title: 强类型配置建模
    details: 用 CFT 描述类型、默认值、引用、多态、注解和业务校验规则。
  - title: 多数据源统一加载
    details: 从 Excel、CSV、CFD 文本配置和飞书/Lark 表格加载数据。
  - title: 面向运行时交付
    details: 经过检查后导出 JSON、MessagePack，并生成 C# 运行时代码。
  - title: 编辑器与语言服务
    details: 为配置文件、记录、关系图、诊断面板和 VS Code/LSP 集成预留统一入口。
  - title: AI 友好维护
    details: 提供结构化 schema/data 命令，让 agent 能读、查、改、校验并继续修复。
  - title: 精准诊断定位
    details: 将错误定位到文件、sheet、行列、record 和字段路径，方便人工与自动化处理。
---

## 项目介绍

Coflow 面向游戏行业配表工具割裂、各项目重复自研、AI 难以深入策划工作流的现状，提供一套统一的配置工程链路。它覆盖从配置建模、数据读取、业务校验到运行时交付的完整流程，减少每个项目重复维护导表脚本和配套工具的成本。

项目以 CFT schema 作为配置语义的事实来源，用明确的数据结构约束配置内容，降低隐式约定、字段漂移和跨表理解成本。Excel、CSV、CFD 文本配置和飞书/Lark 表格会被收敛到同一套 data model 中，再统一执行引用解析、类型检查和业务规则校验。

Coflow 同时面向人工维护和自动化维护。策划可以继续使用表格维护批量数据，也可以用 CFD 表达复杂嵌套结构；程序可以接入 JSON、MessagePack 和 C# 运行时代码；AI agent 可以通过结构化 CLI 命令读取 schema、定位记录、写入数据并根据诊断继续修复。

## 选择你的路径

<div class="feature-grid">
  <a class="feature-card" href="/docs/guide/for-programmers">
    <h3>我是程序</h3>
    <p>了解如何接入项目、配置 CI、导出运行时数据、生成 C# 代码和理解构建安全边界。</p>
  </a>
  <a class="feature-card" href="/docs/guide/for-designers">
    <h3>我是策划</h3>
    <p>了解如何继续使用 Excel/飞书表格，何时使用 CFD，以及如何借助诊断、编辑器和 AI 维护配置。</p>
  </a>
</div>

## 工作流概览

<div class="flow">
  <div class="flow-step"><strong>CFT Schema</strong> 定义类型、字段、默认值、引用和 check 规则。</div>
  <div class="flow-arrow">↓</div>
  <div class="flow-step"><strong>Excel / CSV / CFD / Lark Sheet</strong> 作为不同团队习惯下的数据输入。</div>
  <div class="flow-arrow">↓</div>
  <div class="flow-step"><strong>coflow-engine</strong> 编译 schema、resolve/load source、构建 data model、运行 check 并收集诊断。</div>
  <div class="flow-arrow">↓</div>
  <div class="flow-step"><strong>Artifacts / Tools</strong> 输出 JSON、MessagePack、C# 代码，并服务 CLI、编辑器、LSP 和 AI agent。</div>
</div>

## 能力入口

- [项目配置](/docs/reference/01-project-config)：从 `coflow.yaml` 开始理解项目定义。
- [快速开始](/docs/guide/install)：安装 Coflow 并运行示例项目。
- [RPG 示例](/examples/rpg)：从真实示例理解项目目录、schema、数据源和输出。
- [项目架构](/docs/reference/12-architecture)：作为技术参考说明 crate 边界和 engine 数据流。
