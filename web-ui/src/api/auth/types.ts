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

export interface LoginRequest {
  username: string
  password: string
  remember: boolean
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
  | "INVALID_CREDENTIALS"
  | "ACCOUNT_DISABLED"
  | "ACCOUNT_LOCKED"
  | "RATE_LIMITED"
  | "NETWORK_ERROR"
  | "SERVER_ERROR"

export interface AuthApi {
  login(request: LoginRequest): Promise<LoginResponse>
  logout(token: string): Promise<void>
}
