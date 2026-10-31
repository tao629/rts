/// <reference types="vite/client" />

interface ImportMetaEnv {
  /**
   * 前端访问的 API 基址，默认 `/api/v1`（同源）。
   *
   * 开发期由 vite 代理转发，部署期由 nginx 反代，前端始终同源请求。
   */
  readonly VITE_API_BASE_URL?: string
  /**
   * 仅开发期生效：vite dev server 把 `/api` 代理到的后端地址，
   * 默认 `http://127.0.0.1:8080`（rts-center 默认监听地址）。
   */
  readonly VITE_API_TARGET?: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}
