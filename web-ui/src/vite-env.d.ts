/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** 认证接口模式：mock（前端内置）| real（真实后端） */
  readonly VITE_API_MODE?: "mock" | "real"
  /** 真实后端基础地址，默认 /api/v1 */
  readonly VITE_API_BASE_URL?: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}
