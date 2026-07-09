import DefaultTheme from 'vitepress/theme'
import type { Theme } from 'vitepress'
import './custom.css'

const theme: Theme = {
  ...DefaultTheme,
  enhanceApp({ router }) {
    if (typeof window === 'undefined') return

    const install = () => {
      const doc = document as Document & { __coflowCopyBound?: boolean }
      if (doc.__coflowCopyBound) return
      doc.__coflowCopyBound = true

      document.addEventListener('click', (event) => {
        const target = event.target as HTMLElement | null
        if (!target) return
        const btn = target.closest<HTMLButtonElement>('.start__copy')
        if (!btn) return
        const text = btn.dataset.clip ?? ''
        if (!text) return
        navigator.clipboard.writeText(text).then(() => {
          const original = btn.textContent
          btn.textContent = '已复制'
          btn.classList.add('is-copied')
          window.setTimeout(() => {
            btn.textContent = original
            btn.classList.remove('is-copied')
          }, 1500)
        })
      })
    }

    if (document.readyState === 'loading') {
      document.addEventListener('DOMContentLoaded', install, { once: true })
    } else {
      install()
    }
  }
}

export default theme
