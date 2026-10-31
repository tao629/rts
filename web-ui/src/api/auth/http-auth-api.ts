import { AuthApiError } from "./errors"
import type {
  AuthApi,
  AuthErrorCode,
  AuthUser,
  CaptchaResponse,
  LoginRequest,
  LoginResponse,
} from "./types"

const ERROR_CODES: AuthErrorCode[] = [
  "VALIDATION_ERROR",
  "INVALID_CAPTCHA",
  "INVALID_CREDENTIALS",
  "TOKEN_INVALID",
  "ACCOUNT_DISABLED",
  "FORBIDDEN",
  "NOT_FOUND",
  "USERNAME_TAKEN",
  "ACCOUNT_LOCKED",
  "RATE_LIMITED",
  "NETWORK_ERROR",
  "SERVER_ERROR",
]

function isAuthErrorCode(value: unknown): value is AuthErrorCode {
  return typeof value === "string" && ERROR_CODES.includes(value as AuthErrorCode)
}

/** 真实后端实现：契约见 spec.md §3 */
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
        body: JSON.stringify({
          username: request.username,
          password: request.password,
          remember: request.remember,
          captchaId: request.captchaId,
          captchaCode: request.captchaCode,
        }),
      })
    } catch {
      throw new AuthApiError("NETWORK_ERROR")
    }

    if (!response.ok) {
      throw new AuthApiError(
        await this.readErrorCode(response, "INVALID_CREDENTIALS", "FORBIDDEN"),
        undefined,
        response.status,
      )
    }

    return (await response.json()) as LoginResponse
  }

  /** 获取一次性图形验证码（契约见 spec.md §3.1） */
  async fetchCaptcha(): Promise<CaptchaResponse> {
    let response: Response
    try {
      response = await fetch(`${this.baseUrl}/auth/captcha`, {
        method: "GET",
        headers: { "Cache-Control": "no-store" },
        // 验证码一次性生效，不允许任何缓存
        cache: "no-store",
      })
    } catch {
      throw new AuthApiError("NETWORK_ERROR")
    }

    if (!response.ok) {
      throw new AuthApiError("SERVER_ERROR", undefined, response.status)
    }

    return (await response.json()) as CaptchaResponse
  }

  /**
   * 用令牌换回当前用户信息。
   *
   * 令牌无效时后端返回 401 TOKEN_INVALID，账号被停用时返回 403 ACCOUNT_DISABLED，
   * 两者都表示"登录态不再成立"。
   */
  async fetchMe(token: string): Promise<AuthUser> {
    let response: Response
    try {
      response = await fetch(`${this.baseUrl}/auth/me`, {
        method: "GET",
        headers: { Authorization: `Bearer ${token}` },
        cache: "no-store",
      })
    } catch {
      throw new AuthApiError("NETWORK_ERROR")
    }

    if (!response.ok) {
      throw new AuthApiError(
        await this.readErrorCode(response, "TOKEN_INVALID", "ACCOUNT_DISABLED"),
        undefined,
        response.status,
      )
    }

    return (await response.json()) as AuthUser
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

  /**
   * 读后端的业务错误码。
   *
   * 后端正常情况下总会在响应体里带 `code`；只有在响应体不是合法 JSON 时，
   * 才按 HTTP 状态码兜底 —— 这时用调用方给的 `unauthorized` / `forbidden`，
   * 因为同一个 401 在登录接口是「口令错误」、在令牌接口是「令牌无效」；
   * 403 在令牌接口是「账号已停用」、在管理接口是「没有权限」。
   */
  private async readErrorCode(
    response: Response,
    unauthorized: AuthErrorCode,
    forbidden: AuthErrorCode,
  ): Promise<AuthErrorCode> {
    try {
      const body = (await response.json()) as { code?: unknown }
      if (isAuthErrorCode(body.code)) return body.code
    } catch {
      // 响应体不是合法 JSON 时按状态码兜底
    }
    if (response.status === 401) return unauthorized
    if (response.status === 403) return forbidden
    if (response.status === 409) return "USERNAME_TAKEN"
    if (response.status === 404) return "NOT_FOUND"
    if (response.status === 429) return "RATE_LIMITED"
    if (response.status >= 500) return "SERVER_ERROR"
    if (response.status >= 400) return "VALIDATION_ERROR"
    return "SERVER_ERROR"
  }
}
