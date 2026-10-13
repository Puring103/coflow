import { defineConfig, mergeConfig } from 'vite'
import baseConfig from './vite.config'

// 独立构建真实组件预览，不替换桌面使用的 dist。
export default mergeConfig(baseConfig, defineConfig({
  build: {
    outDir: 'dist-layout-preview',
    rollupOptions: { input: 'layout-preview.html' },
  },
}))
