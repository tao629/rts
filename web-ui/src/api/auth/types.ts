/**
 * 认证接口契约（与 spec.md 第 4 节一致）
 * 后端 rts-center 实现该契约后，前端仅需把 VITE_API_MODE 切到 real 即可对接。
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

export type AuthErrorCode =
  | "VALIDATION_ERROR"
  | "INVALID_CAPTCHA"
  | "INVALID_CREDENTIALS"
  | "ACCOUNT_DISABLED"
  | "ACCOUNT_LOCKED"
  | "RATE_LIMITED"
  | "NETWORK_ERROR"
  | "SERVER_ERROR"

export interface AuthApi {
  login(request: LoginRequest): Promise<LoginResponse>
  logout(token: string): Promise<void>
  /** 获取一次性图形验证码 */
  fetchCaptcha(): Promise<CaptchaResponse>
}
