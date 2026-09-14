// wire 兼容层：按域拆分后的统一出口，存量导入路径不变。
// 新代码优先从 './wire/ids|paths|annotation|values|diagnostics|ipc|graph|routing' 直接导入。
export * from './wire/ids'
export * from './wire/paths'
export * from './wire/annotation'
export * from './wire/values'
export * from './wire/diagnostics'
export * from './wire/ipc'
export * from './wire/graph'
export * from './wire/routing'
