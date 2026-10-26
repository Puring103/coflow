<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { useData, withBase } from 'vitepress'
import prototypeDocument from './home-template.html?raw'
import prototypeStyles from './home.css?raw'

const host = ref<HTMLElement | null>(null)
const { lang } = useData()
const english = computed(() => lang.value === 'en')
const bodyMatch = prototypeDocument.match(/<body>([\s\S]*?)<script src="app\.js"><\/script>\s*<\/body>/)

// 静态构建与浏览器首屏共用语言内容，避免英文页面先展示中文。
const prototypeBody = computed(() => {
  const prefix = english.value ? '/en' : ''
  let body = (bodyMatch?.[1] ?? '')
  .replaceAll('assets/coflow-mark.svg', withBase('/logo.svg'))
  .replaceAll('assets/skill-tree-light.png', withBase('/assets/skill-tree-light.png'))
  .replaceAll('assets/skill-tree-dark.png', withBase('/assets/skill-tree-dark.png'))
  .replaceAll('https://puring103.github.io/coflow/docs/guide/install.html', withBase(`${prefix}/docs/guide/install`))
  .replaceAll('https://puring103.github.io/coflow/docs/', withBase(`${prefix}/docs/`))
  if (english.value) {
    body = body.replace(/<(\w+)\b([^>]*\bdata-en="([^"]*)"[^>]*)>[\s\S]*?<\/\1>/g,
      (_, tag, attributes, text) => `<${tag}${attributes}>${text}</${tag}>`)
    body = body.replace(/<[^>]+>/g, tag => {
      for (const [, attribute, text] of tag.matchAll(/data-en-(aria-label|title|alt)="([^"]*)"/g)) {
        tag = tag.replace(new RegExp(` ${attribute}="[^"]*"`), ` ${attribute}="${text}"`)
      }
      return tag
    })
    body = body.replace('<span data-lang-current>中</span>', '<span data-lang-current>EN</span>')
  }
  return body
})

const cleanups: Array<() => void> = []
let revealObserver: IntersectionObserver | null = null
let styleElement: HTMLStyleElement | null = null
let previousTheme: string | undefined

function listen(target: EventTarget, event: string, handler: EventListener, options?: AddEventListenerOptions) {
  target.addEventListener(event, handler, options)
  cleanups.push(() => target.removeEventListener(event, handler, options))
}

onMounted(async () => {
  styleElement = document.createElement('style')
  styleElement.dataset.coflowPrototype = 'home'
  styleElement.textContent = prototypeStyles
  document.head.appendChild(styleElement)

  previousTheme = document.documentElement.dataset.theme
  const forcedTheme = new URLSearchParams(location.search).get('theme')
  const savedTheme = localStorage.getItem('coflow-theme')
  const preferredTheme = matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
  document.documentElement.dataset.theme = forcedTheme || savedTheme || preferredTheme

  await nextTick()
  const root = host.value
  if (!root) return

  const header = root.querySelector<HTMLElement>('[data-header]')
  const themeToggle = root.querySelector<HTMLButtonElement>('[data-theme-toggle]')
  const langPicker = root.querySelector<HTMLElement>('[data-lang-picker]')
  const langTrigger = root.querySelector<HTMLButtonElement>('[data-lang-trigger]')
  const langOptions = [...root.querySelectorAll<HTMLButtonElement>('[data-lang-option]')]
  const themeQuery = matchMedia('(prefers-color-scheme: dark)')

  langOptions.forEach(option => option.classList.toggle('is-active', option.dataset.langOption === (english.value ? 'en' : 'zh')))
  const updateThemeControl = () => {
    const dark = document.documentElement.dataset.theme === 'dark'
    themeToggle?.setAttribute('aria-label', english.value
      ? (dark ? 'Switch to light mode' : 'Switch to dark mode')
      : (dark ? '切换到浅色模式' : '切换到深色模式'))
    themeToggle?.setAttribute('aria-pressed', String(dark))
  }
  const toggleTheme = () => {
    const next = document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark'
    document.documentElement.dataset.theme = next
    localStorage.setItem('coflow-theme', next)
    updateThemeControl()
  }
  const followSystemTheme = (event: MediaQueryListEvent) => {
    if (localStorage.getItem('coflow-theme')) return
    document.documentElement.dataset.theme = event.matches ? 'dark' : 'light'
    updateThemeControl()
  }
  if (themeToggle) listen(themeToggle, 'click', toggleTheme)
  const closeLanguageMenu = () => {
    langPicker?.classList.remove('is-open')
    langTrigger?.setAttribute('aria-expanded', 'false')
  }
  if (langTrigger) listen(langTrigger, 'click', (() => {
    const open = !langPicker?.classList.contains('is-open')
    langPicker?.classList.toggle('is-open', open)
    langTrigger.setAttribute('aria-expanded', String(open))
  }) as EventListener)
  langOptions.forEach((option) => listen(option, 'click', (() => {
    // 使用语言路由统一首页、文档链接与页面元数据。
    location.assign(withBase(option.dataset.langOption === 'en' ? '/en/' : '/') + location.search + location.hash)
  }) as EventListener))
  listen(document, 'click', ((event: MouseEvent) => {
    if (!langPicker?.contains(event.target as Node)) closeLanguageMenu()
  }) as EventListener)
  listen(document, 'keydown', ((event: KeyboardEvent) => {
    if (event.key === 'Escape') closeLanguageMenu()
  }) as EventListener)
  listen(themeQuery, 'change', followSystemTheme as EventListener)

  const updateHeader = () => header?.classList.toggle('is-scrolled', scrollY > 12)
  listen(window, 'scroll', updateHeader, { passive: true })

  root.querySelectorAll<HTMLElement>('[data-cardstack]').forEach((stack, stackIndex) => {
    const tabs = [...stack.querySelectorAll<HTMLButtonElement>('.stack-tabs button[data-view]')]
    const cards = [...stack.querySelectorAll<HTMLElement>('.stack-card')]
    const order = cards.map((card) => card.dataset.card)
    const activate = (view?: string) => {
      const front = order.indexOf(view)
      if (front < 0) return
      // 视觉叠放保留相邻卡片，无障碍阅读只进入当前视图。
      tabs.forEach((tab) => {
        const selected = tab.dataset.view === view
        tab.classList.toggle('is-active', selected)
        tab.setAttribute('aria-selected', String(selected))
        tab.tabIndex = selected ? 0 : -1
      })
      cards.forEach((card) => {
        let offset = order.indexOf(card.dataset.card) - front
        if (offset > order.length / 2) offset -= order.length
        if (offset < -order.length / 2) offset += order.length
        card.dataset.offset = String(offset)
        card.setAttribute('aria-hidden', String(offset !== 0))
      })
    }
    cards.forEach((card) => listen(card, 'click', () => card.dataset.offset !== '0' && activate(card.dataset.card)))
    tabs.forEach((tab) => {
      const panel = cards.find((card) => card.dataset.card === tab.dataset.view)
      tab.id = `view-tab-${stackIndex}-${tab.dataset.view}`
      tab.setAttribute('role', 'tab')
      if (panel) {
        panel.id = `view-panel-${stackIndex}-${tab.dataset.view}`
        panel.setAttribute('role', 'tabpanel')
        panel.setAttribute('aria-labelledby', tab.id)
        tab.setAttribute('aria-controls', panel.id)
      }
      listen(tab, 'click', () => activate(tab.dataset.view))
      listen(tab, 'focus', () => activate(tab.dataset.view))
      listen(tab, 'keydown', ((event: KeyboardEvent) => {
        const index = tabs.indexOf(tab)
        const next = event.key === 'ArrowRight' ? (index + 1) % tabs.length
          : event.key === 'ArrowLeft' ? (index - 1 + tabs.length) % tabs.length
          : event.key === 'Home' ? 0 : event.key === 'End' ? tabs.length - 1 : -1
        if (next < 0) return
        event.preventDefault()
        tabs[next].focus()
      }) as EventListener)
    })
    activate((tabs.find((tab) => tab.classList.contains('is-active')) ?? tabs[0])?.dataset.view)
  })

  // 官网使用可安装示例插件的同一挂载函数，输入变化走插件 update 生命周期。
  const formulaInput = root.querySelector<HTMLInputElement>('[data-chemical-input]')!
  const formulaOutput = root.querySelector<HTMLElement>('[data-chemical-output]')!
  const formulaController = new AbortController()
  cleanups.push(() => formulaController.abort())
  // public 插件使用完整 URL 交给浏览器加载，避免开发服务器将其纳入源码转换。
  const formulaModuleUrl = new URL(withBase('/examples/chemistry/plugin.js'), window.location.origin).href
  const formulaModule = await import(/* @vite-ignore */ formulaModuleUrl)
  if (formulaController.signal.aborted) return
  const formulaContext = () => ({ value: { kind: 'object', value: { actual_type: 'Formula', fields: {
    expression: { kind: 'string', value: formulaInput.value },
  } } } })
  const formulaView = formulaModule.mountFormula(formulaContext(), {
    element: formulaOutput, signal: formulaController.signal,
    replace: (content: Node) => formulaOutput.replaceChildren(content),
  })
  listen(formulaInput, 'input', () => formulaView.update(formulaContext()))
  root.querySelectorAll<HTMLButtonElement>('[data-formula]').forEach(button => {
    listen(button, 'click', () => {
      formulaInput.value = button.dataset.formula!
      formulaView.update(formulaContext())
    })
  })

  const stageList = root.querySelector<HTMLElement>('.stage-list')
  const stages = [...root.querySelectorAll<HTMLElement>('.stage')]
  let stageFocusFrame = 0
  const updateStageFocus = () => {
    stageFocusFrame = 0
    const visible = stages.filter((stage) => {
      const rect = stage.getBoundingClientRect()
      return rect.bottom > 80 && rect.top < innerHeight
    })
    if (!visible.length) {
      stageList?.classList.remove('has-focus')
      stages.forEach((stage) => stage.classList.remove('is-focused'))
      return
    }
    const viewportCenter = innerHeight * 0.52
    const focused = visible.reduce((nearest, stage) => {
      const rect = stage.getBoundingClientRect()
      const distance = Math.abs(rect.top + rect.height / 2 - viewportCenter)
      const nearestRect = nearest.getBoundingClientRect()
      const nearestDistance = Math.abs(nearestRect.top + nearestRect.height / 2 - viewportCenter)
      return distance < nearestDistance ? stage : nearest
    })
    stageList?.classList.add('has-focus')
    stages.forEach((stage) => stage.classList.toggle('is-focused', stage === focused))
  }
  const scheduleStageFocus = () => {
    if (stageFocusFrame) return
    stageFocusFrame = requestAnimationFrame(updateStageFocus)
  }
  listen(window, 'scroll', scheduleStageFocus, { passive: true })
  listen(window, 'resize', scheduleStageFocus)
  cleanups.push(() => stageFocusFrame && cancelAnimationFrame(stageFocusFrame))
  updateStageFocus()

  const reveals = root.querySelectorAll<HTMLElement>('[data-reveal]')
  const reduceMotion = matchMedia('(prefers-reduced-motion: reduce)').matches
  if (reduceMotion || location.search.includes('reveal=off') || !('IntersectionObserver' in window)) {
    reveals.forEach((element) => element.classList.add('is-visible'))
  } else {
    revealObserver = new IntersectionObserver((entries, observer) => {
      entries.forEach((entry, index) => {
        if (!entry.isIntersecting) return
        const element = entry.target as HTMLElement
        const delay = Number(element.dataset.revealDelay ?? index * 60)
        window.setTimeout(() => element.classList.add('is-visible'), delay)
        observer.unobserve(element)
      })
    }, { rootMargin: '0px 0px -12% 0px', threshold: 0.12 })
    reveals.forEach((element) => revealObserver?.observe(element))
  }

  updateThemeControl()
  updateHeader()
})

onBeforeUnmount(() => {
  revealObserver?.disconnect()
  cleanups.splice(0).forEach((cleanup) => cleanup())
  styleElement?.remove()
  if (previousTheme === undefined) delete document.documentElement.dataset.theme
  else document.documentElement.dataset.theme = previousTheme
})
</script>

<template>
  <div ref="host" class="prototype-home" :data-lang="english ? 'en' : 'zh'" v-html="prototypeBody"></div>
</template>
