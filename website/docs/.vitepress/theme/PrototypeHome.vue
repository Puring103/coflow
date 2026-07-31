<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { withBase } from 'vitepress'
import prototypeDocument from './home-template.html?raw'
import prototypeStyles from './home.css?raw'

const host = ref<HTMLElement | null>(null)
const bodyMatch = prototypeDocument.match(/<body>([\s\S]*?)<script src="app\.js"><\/script>\s*<\/body>/)

const prototypeBody = (bodyMatch?.[1] ?? '')
  .replaceAll('assets/coflow-mark.svg', withBase('/logo.svg'))
  .replaceAll('https://puring103.github.io/coflow/docs/guide/install.html', withBase('/docs/guide/install'))
  .replaceAll('https://puring103.github.io/coflow/docs/', withBase('/docs/'))

const cleanups: Array<() => void> = []
let revealObserver: IntersectionObserver | null = null
let styleElement: HTMLStyleElement | null = null
let previousTheme: string | undefined
let previousLang: string | null = null
const originalContent = new Map<HTMLElement, string>()

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
  previousLang = document.documentElement.getAttribute('lang')
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
  const langCurrent = root.querySelector<HTMLElement>('[data-lang-current]')
  const langOptions = [...root.querySelectorAll<HTMLButtonElement>('[data-lang-option]')]
  const themeQuery = matchMedia('(prefers-color-scheme: dark)')

  const translatable = [...root.querySelectorAll<HTMLElement>('[data-en]')]
  translatable.forEach((element) => originalContent.set(element, element.innerHTML))
  const updateLanguage = (language: 'zh' | 'en') => {
    root.dataset.lang = language
    document.documentElement.lang = language === 'en' ? 'en' : 'zh-CN'
    translatable.forEach((element) => {
      if (language === 'en') element.textContent = element.dataset.en ?? ''
      else element.innerHTML = originalContent.get(element) ?? ''
    })
    if (langCurrent) langCurrent.textContent = language === 'en' ? 'EN' : '中'
    langOptions.forEach((option) => option.classList.toggle('is-active', option.dataset.langOption === language))
  }
  const updateThemeControl = () => {
    const dark = document.documentElement.dataset.theme === 'dark'
    themeToggle?.setAttribute('aria-label', dark ? '切换到浅色模式' : '切换到深色模式')
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
    const next = option.dataset.langOption === 'en' ? 'en' : 'zh'
    localStorage.setItem('coflow-home-language', next)
    updateLanguage(next)
    closeLanguageMenu()
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

  root.querySelectorAll<HTMLElement>('[data-cardstack]').forEach((stack) => {
    const tabs = [...stack.querySelectorAll<HTMLButtonElement>('.stack-tabs button[data-view]')]
    const cards = [...stack.querySelectorAll<HTMLElement>('.stack-card')]
    const order = cards.map((card) => card.dataset.card)
    const activate = (view?: string) => {
      const front = order.indexOf(view)
      if (front < 0) return
      tabs.forEach((tab) => tab.classList.toggle('is-active', tab.dataset.view === view))
      cards.forEach((card) => {
        let offset = order.indexOf(card.dataset.card) - front
        if (offset > order.length / 2) offset -= order.length
        if (offset < -order.length / 2) offset += order.length
        card.dataset.offset = String(offset)
      })
    }
    cards.forEach((card) => listen(card, 'click', () => card.dataset.offset !== '0' && activate(card.dataset.card)))
    tabs.forEach((tab) => {
      listen(tab, 'click', () => activate(tab.dataset.view))
      listen(tab, 'focus', () => activate(tab.dataset.view))
    })
    activate((tabs.find((tab) => tab.classList.contains('is-active')) ?? tabs[0])?.dataset.view)
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
  updateLanguage(localStorage.getItem('coflow-home-language') === 'en' ? 'en' : 'zh')
  updateHeader()
})

onBeforeUnmount(() => {
  revealObserver?.disconnect()
  cleanups.splice(0).forEach((cleanup) => cleanup())
  styleElement?.remove()
  originalContent.clear()
  if (previousTheme === undefined) delete document.documentElement.dataset.theme
  else document.documentElement.dataset.theme = previousTheme
  if (previousLang === null) document.documentElement.removeAttribute('lang')
  else document.documentElement.setAttribute('lang', previousLang)
})
</script>

<template>
  <div ref="host" class="prototype-home" v-html="prototypeBody"></div>
</template>
