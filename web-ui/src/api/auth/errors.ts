import type { AuthErrorCode } from "./types"

/** 认证接口错误：携带业务错误码，供页面映射为中文提示 */
export class AuthApiError extends Error {
  readonly code: AuthErrorCode
  readonly status: number

  constructor(code: AuthErrorCode, message?: string, status = 0) {
    super(message ?? code)
    this.name = "AuthApiError"
    this.code = code
    this.status = status
  }
}

const ERROR_MESSAGES: Record<AuthErrorCode, string> = {
  VALIDATION_ERROR: "提交信息有误，请检查后重试",
  INVALID_CREDENTIALS: "用户名或密码错误",
  ACCOUNT_DISABLED: "该账号已被停用，请联系管理员",
  ACCOUNT_LOCKED: "账号已锁定，请稍后再试或联系管理员",
  RATE_LIMITED: "尝试过于频繁，请稍后再试",
  NETWORK_ERROR: "无法连接到服务器，请检查网络后重试",
  SERVER_ERROR: "服务器开小差了，请稍后再试",
}

/** 将任意异常归一化为页面可直接展示的中文文案 */
export function toErrorMessage(error: unknown): string {
  if (error instanceof AuthApiError) {
    return ERROR_MESSAGES[error.code] ?? ERROR_MESSAGES.SERVER_ERROR
  }
  if (error instanceof Error && error.message) {
    return error.message
  }
  return ERROR_MESSAGES.SERVER_ERROR
}
