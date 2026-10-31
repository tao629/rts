/**
 * 认证接口契约（与 spec.md §3 一致）
 *
 * 后端 rts-center 是这份契约的唯一实现：验证码、登录、登出、确认登录态
 * 全部由服务端裁决，前端不再内置任何模拟实现。
 */

export interface AuthUser {
  id: string
  username: string
  displayName: string
  roles: string[]
}

/** 图形验证码响应（契约见 spec.md §3.1） */
export interface CaptchaResponse {
  /** 验证码唯一标识，登录时原样回传 */
  captchaId: string
  /** data:image/svg+xml;base64,... */
  image: string
  /** ISO 8601 时间戳 */
  expiresAt: string
}

export interface LoginRequest {
  username: string
  password: string
  remember: boolean
  /** 验证码 ID */
  captchaId: string
  /** 验证码内容，大小写不敏感 */
  captchaCode: string
}

export interface LoginResponse {
  token: string
  tokenType: "Bearer"
  /** ISO 8601 时间戳 */
  expiresAt: string
  user: AuthUser
}

/**
 * 后端错误码全集（spec.md §3.4）。
 *
 * `NETWORK_ERROR` 是唯一的客户端错误码：请求压根没发出去时用它兜底。
 */
export type AuthErrorCode =
  | "VALIDATION_ERROR"
  | "INVALID_CAPTCHA"
  | "INVALID_CREDENTIALS"
  | "TOKEN_INVALID"
  | "ACCOUNT_DISABLED"
  | "FORBIDDEN"
  | "NOT_FOUND"
  | "USERNAME_TAKEN"
  | "ACCOUNT_LOCKED"
  | "RATE_LIMITED"
  | "NETWORK_ERROR"
  | "SERVER_ERROR"

export interface AuthApi {
  login(request: LoginRequest): Promise<LoginResponse>
  logout(token: string): Promise<void>
  /** 获取一次性图形验证码 */
  fetchCaptcha(): Promise<CaptchaResponse>
  /** 用令牌换回当前用户信息；令牌无效时抛 TOKEN_INVALID */
  fetchMe(token: string): Promise<AuthUser>
}
