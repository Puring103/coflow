export default function activate(host) {
  const mounted = new Set()
  const unregister = host.register.view({
    id: 'item-ids',
    title: '物品 ID 列表',
    types: ['Item'],
    readOnly: true,
    mount(context, outlet) {
      const element = document.createElement('pre')
      let disposed = false
      const dispose = () => {
        if (disposed) return
        disposed = true
        outlet.signal.removeEventListener('abort', dispose)
        element.remove()
        mounted.delete(dispose)
      }
      const update = next => {
        // 宿主传入新项目上下文时重绘，只输出文本，避免将 ID 当作 HTML。
        if (disposed || outlet.signal.aborted) return
        element.textContent = next.records.map(record => record.coordinate.id).join('\n')
      }
      if (outlet.signal.aborted) return { update, dispose }
      update(context)
      outlet.replace(element)
      mounted.add(dispose)
      outlet.signal.addEventListener('abort', dispose, { once: true })
      return { update, dispose }
    }
  })
  return {
    dispose() {
      // 插件卸载时清理所有实例并注销贡献，不保留 DOM 或事件监听。
      for (const dispose of [...mounted]) dispose()
      unregister()
    }
  }
}
