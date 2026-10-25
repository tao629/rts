import { AuthApiError } from "./errors"
import type {
  AuthApi,
  AuthErrorCode,
  LoginRequest,
  LoginResponse,
} from "./types"

const ERROR_CODES: AuthErrorCode[] = [
  "VALIDATION_ERROR",
  "INVALID_CREDENTIALS",
  "ACCOUNT_DISABLED",
  "ACCOUNT_LOCKED",
  "RATE_LIMITED",
  "NETWORK_ERROR",
  "SERVER_ERROR",
]

function isAuthErrorCode(value: unknown): value is AuthErrorCode {
  return typeof value === "string" && ERROR_CODES.includes(value as AuthErrorCode)
}

/** 真实后端实现：契约见 spec.md 第 4 节 */
export class HttpAuthApi implements AuthApi {
  private readonly baseUrl: string

  constructor(baseUrl: string) {
    this.baseUrl = baseUrl.replace(/\/$/, "")
  }

  async login(request: LoginRequest): Promise<LoginResponse> {
    let response: Response
    try {
      response = await fetch(`${this.baseUrl}/auth/login`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(request),
      })
    } catch {
      throw new AuthApiError("NETWORK_ERROR")
    }

    if (!response.ok) {
      throw new AuthApiError(await this.readErrorCode(response), undefined, response.status)
    }

    return (await response.json()) as LoginResponse
  }

  async logout(token: string): Promise<void> {
    try {
      await fetch(`${this.baseUrl}/auth/logout`, {
        method: "POST",
        headers: { Authorization: `Bearer ${token}` },
      })
    } catch {
      // 登出失败不阻断本地清理
    }
  }

  private async readErrorCode(response: Response): Promise<AuthErrorCode> {
    try {
      const body = (await response.json()) as { code?: unknown }
      if (isAuthErrorCode(body.code)) return body.code
    } catch {
      // 响应体不是合法 JSON 时按状态码兜底
    }
    if (response.status === 401) return "INVALID_CREDENTIALS"
    if (response.status === 429) return "RATE_LIMITED"
    if (response.status >= 500) return "SERVER_ERROR"
    if (response.status >= 400) return "VALIDATION_ERROR"
    return "SERVER_ERROR"
  }
}
