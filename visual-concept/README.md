# Coflow 视觉语言与首页示例

一套独立视觉方案。直接打开 `index.html`，或运行本地静态服务查看。

## 视觉概念

主题为“汇流光谱”：程序、策划和 AI 在入口处使用不同颜色，进入 Coflow 后收束为统一青绿。青绿贯穿导航、按钮、工作流、阶段时间线、状态和交付结果，岗位色只用于说明来源与责任。

整体采用克制的开发者工具风格：暖调中性底色、渐变主标题、低对比网格光晕背景，以及进入视口时的轻量揭示动效。支持浅色与深色两套主题。

| 角色 / 状态 | 色彩 | 用途 |
| --- | --- | --- |
| 程序 | 清水青 `#2F93A6` | CFT、check 与工程入口 |
| 策划 | 柔和珊瑚 `#E2776B` | Excel、CSV、CFD 与内容入口 |
| AI | 草木黄绿 `#94B73E` | CLI、Skills 与自动化入口 |
| Coflow | 汇流青绿 `#24837C` | 统一模型、主操作与工作流 |
| Diagnostic | 诊断玫红 `#D24F6F` | 错误与阻塞 |

## Logo

Logo 使用三条宽流带表达不同角色。岗位色在交汇前逐步过渡为青绿，交汇区域只保留统一颜色。

- `assets/coflow-logo.svg`：浅色背景横版 Logo。
- `assets/coflow-logo-dark.svg`：深色背景横版 Logo。
- `assets/coflow-mark.svg`：彩色图形标。
- `assets/coflow-mark-mono.svg`：单色图形标。

## 页面结构

1. **Hero**：Slogan、主行动按钮、关键指标，以及三色汇入 Coflow 再交付 Runtime 的汇流图。
2. **Stages**：七个工作流阶段沿一条贯穿的时间线排列，每个阶段为“说明 + 真实产品界面演示”双栏（输入、check、诊断、多视图编辑、实时迭代、构建交付、插件扩展）。
3. **Roles**：程序、策划、AI 三张角色卡，说明各自入口与职责。
4. **CTA + Footer**：安装引导与链接。

桌面端时间线阶段为双栏，移动端折叠为单栏并收窄时间线；汇流图在窄屏改为垂直堆叠，仍保留三路输入的辨识度。

## 动效与主题

- 汇流图信号使用首尾渐隐的短光段，循环无可见跳回。
- 内容进入视口时轻量上移淡入（`IntersectionObserver`）。
- 浅色 / 深色主题默认跟随系统，手动选择保存在浏览器本地；支持 `?theme=light|dark` 强制指定。
- 完整支持 `prefers-reduced-motion`：关闭动画并直接显示全部内容。
- 调试用：`?reveal=off` 可跳过揭示动效，直接呈现所有内容（用于静态截图）。

## 文件结构

```text
visual-concept/
├── assets/
│   ├── coflow-logo.svg
│   ├── coflow-logo-dark.svg
│   ├── coflow-mark.svg
│   └── coflow-mark-mono.svg
├── index.html
├── styles.css
├── app.js
├── preview-desktop.png
├── preview-mobile.png
├── design-notes.md
└── README.md
```
