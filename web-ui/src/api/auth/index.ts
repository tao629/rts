import { HttpAuthApi } from "./http-auth-api"
import type { AuthApi } from "./types"

export * from "./types"
export { AuthApiError, toErrorMessage } from "./errors"

/**
 * 后端基址。
 *
 * 默认走同源 `/api/v1`：开发期由 vite dev server 代理到 rts-center，
 * 部署期由 nginx 反代到 rts-center —— 前端始终是同源请求，不需要 CORS。
 */
const apiBaseUrl = import.meta.env.VITE_API_BASE_URL ?? "/api/v1"

/** 唯一的认证接口实现：真实后端 rts-center。 */
export const authApi: AuthApi = new HttpAuthApi(apiBaseUrl)
