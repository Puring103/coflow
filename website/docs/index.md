---
layout: page
title: Coflow
---

<div class="home">

<section class="hero">
<div class="hero__inner">
<p class="hero__eyebrow">Coflow</p>

<h1 class="hero__title">
AI 时代的<br />
<span class="hero__accent">数据配置工作流</span>
</h1>

<p class="hero__tagline">灵活 · 可信 · 高效</p>

<p class="hero__lead">
从类型建模、多源采集、业务校验，到数据交付，Coflow 把游戏配置整理成一套可验证、可定位、可写回、AI 可维护的流水线。
</p>
</div>

<aside class="start" aria-label="快速开始">
<div class="start__section">
<div class="start__label"><span class="start__step">01</span>安装 CLI</div>
<div class="start__snippet">
<pre v-pre><code>cargo install --git https://github.com/Puring103/coflow.git coflow
coflow --help</code></pre>
<button type="button" class="start__copy" data-clip="cargo install --git https://github.com/Puring103/coflow.git coflow&#10;coflow --help" aria-label="复制安装命令">复制</button>
</div>
</div>

<div class="start__section">
<div class="start__label"><span class="start__step">02</span>安装 AI Agent Skills</div>
<div class="start__snippet">
<pre v-pre><code>npx skills add Puring103/coflow -g --skill "*" --copy -y</code></pre>
<button type="button" class="start__copy" data-clip='npx skills add Puring103/coflow -g --skill "*" --copy -y' aria-label="复制 skills 安装命令">复制</button>
</div>
</div>

<div class="start__cta">
<a class="btn btn--primary" href="/docs/guide/install"><span>阅读文档</span><span aria-hidden="true">→</span></a>
<a class="btn" href="https://github.com/Puring103/coflow/releases/latest" target="_blank" rel="noopener"><span>下载最新 Release</span><span aria-hidden="true">↗</span></a>
</div>
</aside>
</section>


<section class="journey">
<header class="journey__head">
<p class="kicker">一次完整的配置旅程</p>
<h2>从一个字段的想法，走到运行时可用的产物</h2>
<p class="dek">每一幕都是同一个 Skill 配置的一个阶段。Coflow 把这些阶段串成同一个数据模型，让程序、策划和 AI 在同一条流水线上工作。</p>
</header>

<ol class="chapters">

<li class="chapter">
<div class="chapter__index">
<span class="chapter__num">01</span>
<span class="chapter__tag">Model</span>
</div>
<div class="chapter__text">
<h3>用 CFT 描述配置的语义</h3>
<p>类型、枚举、默认值、引用、多态、数组和字典都是一等公民。<code>check</code> 让业务规则和字段建模住在同一处。</p>
</div>
<div class="chapter__demo">
<pre v-pre class="code"><code><span class="tok-kw">enum</span> <span class="tok-type">Element</span> &#123; Fire, Ice, Lightning &#125;
<span class="tok-kw">abstract type</span> <span class="tok-type">Effect</span> &#123;&#125;
<span class="tok-kw">type</span> <span class="tok-type">Damage</span> : <span class="tok-type">Effect</span> &#123;
  element: <span class="tok-type">Element</span>;
  value:   <span class="tok-type">int</span>;
&#125;
<span class="tok-kw">type</span> <span class="tok-type">Skill</span> &#123;
  <span class="tok-anno">@localized</span> name: <span class="tok-type">string</span>;
  cost:    <span class="tok-type">int</span> = <span class="tok-num">0</span>;
  effects: [<span class="tok-type">Effect</span>];
  <span class="tok-kw">check</span> &#123;
    cost &gt;= <span class="tok-num">0</span>;
    effects.len() &gt; <span class="tok-num">0</span>;
  &#125;
&#125;</code></pre>
</div>
</li>

<li class="chapter chapter--reverse">
<div class="chapter__index">
<span class="chapter__num">02</span>
<span class="chapter__tag">Ingest</span>
</div>
<div class="chapter__text">
<h3>让每一份数据留在它该在的地方</h3>
<p>Excel 装数值、CFD 装嵌套结构、飞书协作、CSV 版本友好。Coflow 从任何一种来源都能读到同一个 runtime model。</p>
<p class="chapter__note">当前支持 Excel、CFD、CSV、飞书表格，后续会持续扩展更多数据源。</p>
</div>
<div class="chapter__demo">
<div class="sources">
<div class="source">
<header><b>Excel</b><span>items.xlsx · Item</span></header>
<pre v-pre class="table"><code><span class="tok-mute">A     B          C    D</span>
id    name       cost hp
<span class="tok-brand">apple</span> 苹果       10   +20
<span class="tok-brand">sword</span> 长剑       80   —
<span class="tok-brand">robe</span>  法袍       50   +5</code></pre>
</div>
<div class="source">
<header><b>CFD</b><span>skills.cfd</span></header>
<pre v-pre class="code"><code><span class="tok-type">Skill</span> fireball &#123;
  name = <span class="tok-str">"火球术"</span>
  cost = <span class="tok-num">35</span>
  effects = [
    <span class="tok-type">Damage</span> &#123;
      element = <span class="tok-type">Fire</span>
      value   = <span class="tok-num">120</span>
    &#125;
  ]
&#125;</code></pre>
</div>
<div class="source">
<header><b>Lark</b><span>飞书表格 · Buff</span></header>
<pre v-pre class="table"><code><span class="tok-mute">id      duration  stack</span>
<span class="tok-brand">burn</span>    3.0       1
<span class="tok-brand">frozen</span>  2.0       1
<span class="tok-brand">shield</span>  5.0       3</code></pre>
</div>
<div class="source">
<header><b>CSV</b><span>loot.csv</span></header>
<pre v-pre class="table"><code><span class="tok-mute">boss,drop,weight</span>
dragon,fireball,50
dragon,shield,30
lich,frozen,60</code></pre>
</div>
</div>
</div>
</li>

<li class="chapter">
<div class="chapter__index">
<span class="chapter__num">03</span>
<span class="chapter__tag">Validate</span>
</div>
<div class="chapter__text">
<h3>诊断精确到单元格和字段路径</h3>
<p>错误带上 code、stage、文件、sheet、cell、record、字段路径。人可读，AI 也能拿去逐条修复。</p>
</div>
<div class="chapter__demo">
<article class="diag diag--err">
<header>
<span class="diag__code">CELL-TypeMismatch</span>
<span class="diag__stage">CELL</span>
</header>
<dl>
<dt>file</dt><dd>data/items.xlsx</dd>
<dt>sheet</dt><dd>Item</dd>
<dt>cell</dt><dd>D2</dd>
<dt>message</dt><dd>expected <code>int</code>, got <code>"+20"</code></dd>
</dl>
</article>
<article class="diag diag--warn">
<header>
<span class="diag__code">CFD-CHECK-015</span>
<span class="diag__stage">CHECK</span>
</header>
<dl>
<dt>record</dt><dd>Skill.fireball</dd>
<dt>path</dt><dd>Skill.effects</dd>
<dt>message</dt><dd>effect list must not be empty</dd>
</dl>
</article>
</div>
</li>

<li class="chapter chapter--reverse">
<div class="chapter__index">
<span class="chapter__num">04</span>
<span class="chapter__tag">Deliver</span>
</div>
<div class="chapter__text">
<h3>运行时产物，一次构建</h3>
<p>JSON 便于调试和检索，MessagePack 用于运行时加载，C# 代码消除 DTO 与 schema 的漂移。</p>
<p class="chapter__note">当前支持 JSON、MessagePack 和 C# 代码生成，后续会持续扩展更多导出格式与代码生成目标。</p>
</div>
<div class="chapter__demo">
<div class="artifacts">
<div class="artifact">
<header><b>JSON</b><span>data.json</span></header>
<pre v-pre class="code"><code>&#123;
  <span class="tok-str">"id"</span>: <span class="tok-str">"fireball"</span>,
  <span class="tok-str">"name"</span>: <span class="tok-str">"火球术"</span>,
  <span class="tok-str">"cost"</span>: <span class="tok-num">35</span>,
  <span class="tok-str">"effects"</span>: [
    &#123; <span class="tok-str">"$type"</span>: <span class="tok-str">"Damage"</span>,
      <span class="tok-str">"element"</span>: <span class="tok-str">"Fire"</span>,
      <span class="tok-str">"value"</span>: <span class="tok-num">120</span> &#125;
  ]
&#125;</code></pre>
</div>
<div class="artifact">
<header><b>C#</b><span>Config.cs</span></header>
<pre v-pre class="code"><code><span class="tok-kw">public sealed record</span> <span class="tok-type">Skill</span>(
  <span class="tok-type">string</span> Id,
  <span class="tok-type">string</span> Name,
  <span class="tok-type">int</span>    Cost,
  <span class="tok-type">IReadOnlyList</span>&lt;<span class="tok-type">Effect</span>&gt; Effects
);
<span class="tok-kw">public abstract record</span> <span class="tok-type">Effect</span>;
<span class="tok-kw">public sealed record</span> <span class="tok-type">Damage</span>(
  <span class="tok-type">Element</span> Element,
  <span class="tok-type">int</span>     Value
) : <span class="tok-type">Effect</span>;</code></pre>
</div>
</div>
</div>
</li>

<li class="chapter">
<div class="chapter__index">
<span class="chapter__num">05</span>
<span class="chapter__tag">Iterate</span>
</div>
<div class="chapter__text">
<h3>把这条流水线交给 AI</h3>
<p><code>coflow</code> CLI 是稳定的 agent 入口：读 schema、查记录、提交 patch、根据 diagnostics 继续修，直到 check 通过。</p>
</div>
<div class="chapter__demo">
<div class="agent">
<div class="agent__step">
<span class="agent__prompt">$</span>
<code>coflow schema inspect Skill</code>
<span class="agent__note">读懂字段和 check</span>
</div>
<div class="agent__step">
<span class="agent__prompt">$</span>
<code>coflow data get Skill.fireball</code>
<span class="agent__note">取到当前记录</span>
</div>
<div class="agent__step">
<span class="agent__prompt">$</span>
<code>coflow data patch --patch patch.json</code>
<span class="agent__note">结构化写回</span>
</div>
<div class="agent__step">
<span class="agent__prompt">$</span>
<code>coflow check examples/rpg</code>
<span class="agent__note">拿诊断继续迭代</span>
</div>
</div>
</div>
</li>

</ol>
</section>

<section class="pillars">
<header class="pillars__head">
<p class="kicker">Coflow 是什么</p>
<h2>不是导表脚本，而是一套配置工程基础设施</h2>
</header>
<div class="pillars__grid">
<article class="pillar">
<span class="pillar__num">01</span>
<h3>强类型建模</h3>
<p>CFT schema 是所有配置的权威定义。类型、默认值、引用、多态和 check 规则写在源头，让非法数据在进入运行时之前就被挡下。</p>
</article>
<article class="pillar">
<span class="pillar__num">02</span>
<h3>统一数据模型</h3>
<p>不同来源的数据加载后进入同一个 runtime model，引用解析、check 与导出共用一套语义，不用为每种数据源写一套逻辑。</p>
</article>
<article class="pillar">
<span class="pillar__num">03</span>
<h3>结构化诊断</h3>
<p>诊断携带 code、stage、文件、单元格、record、字段路径，既能被人快速定位，也能被 AI agent 逐条消费和修复。</p>
</article>
<article class="pillar">
<span class="pillar__num">04</span>
<h3>安全写回</h3>
<p>改动可以原样写回 Excel、CFD、飞书等原始数据源。所有 patch 都经过 schema 校验，改了哪一条、哪个字段都可复盘。</p>
</article>
<article class="pillar">
<span class="pillar__num">05</span>
<h3>运行时交付</h3>
<p>同一次构建同时产出运行时数据和加载代码。当前支持 JSON、MessagePack 和 C# 代码生成，后续会持续扩展。</p>
</article>
<article class="pillar">
<span class="pillar__num">06</span>
<h3>维度与变体</h3>
<p>用 dimension 描述任意维度的变体 —— 语言、平台、渠道、区服，都能在同一份 schema 下生成对应的源文件和运行时数据。</p>
</article>
</div>
</section>

<section class="paths">
<header class="paths__head">
<p class="kicker">从这里开始</p>
<h2>程序、策划、AI 用同一套配置语义</h2>
</header>
<div class="paths__grid">
<a class="path" href="/docs/guide/for-programmers">
<span class="path__tag">程序</span>
<h3>配置 CI 与运行时交付</h3>
<p>从项目结构、导出格式到 C# 代码生成，理解 Coflow 的 artifact 安全边界。</p>
<span class="path__arrow">→</span>
</a>
<a class="path" href="/docs/guide/for-designers">
<span class="path__tag">策划</span>
<h3>继续用你熟悉的表格</h3>
<p>用 Excel/飞书维护数值，用 CFD 表达嵌套结构，用诊断定位问题。</p>
<span class="path__arrow">→</span>
</a>
<a class="path" href="/docs/guide/ai-agent">
<span class="path__tag">AI Agent</span>
<h3>让 agent 用工具修配置</h3>
<p>通过 CLI 读 schema、查记录、提交 patch，根据 diagnostics 迭代到通过。</p>
<span class="path__arrow">→</span>
</a>
</div>
</section>

</div>
