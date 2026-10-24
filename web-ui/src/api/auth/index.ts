import { HttpAuthApi } from "./http-auth-api"
import { MockAuthApi } from "./mock-auth-api"
import type { AuthApi } from "./types"

export * from "./types"
export { AuthApiError, toErrorMessage } from "./errors"
export { MOCK_CREDENTIALS } from "./mock-auth-api"

const apiMode = import.meta.env.VITE_API_MODE ?? "mock"
const apiBaseUrl = import.meta.env.VITE_API_BASE_URL ?? "/api/v1"

/** 当前是否为 mock 模式（登录页据此展示演示账号提示） */
export const isMockMode = apiMode !== "real"

export const authApi: AuthApi =
  isMockMode ? new MockAuthApi() : new HttpAuthApi(apiBaseUrl)
