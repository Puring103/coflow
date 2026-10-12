// Worker 适配兼容层：单例 + 请求表 + 超时已拆至 graph/ 下，新代码直接引用 graph/*。
export { runGraphLayoutInWorker } from '../graph/layoutWorkerClient'
