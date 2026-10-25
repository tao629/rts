import { AuthApiError } from "./errors"
import type { AuthApi, AuthUser, LoginRequest, LoginResponse } from "./types"

interface MockAccount {
  user: AuthUser
  password: string
}

const MOCK_ACCOUNTS: MockAccount[] = [
  {
    user: {
      id: "u-1001",
      username: "admin",
      displayName: "系统管理员",
      roles: ["super_admin"],
    },
    password: "Admin@123",
  },
  {
    user: {
      id: "u-2001",
      username: "operator",
      displayName: "值班运维",
      roles: ["operator"],
    },
    password: "Operator@123",
  },
]

const NETWORK_DELAY_MS = 600

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

/** mock 账号清单，供登录页展示演示提示 */
export const MOCK_CREDENTIALS = MOCK_ACCOUNTS.map((account) => ({
  username: account.user.username,
  password: account.password,
}))

/**
 * 前端内置的 mock 实现：行为与 spec.md 约定的真实接口保持一致，
 * 便于在后端接口就绪前完整走通登录闭环。
 */
export class MockAuthApi implements AuthApi {
  async login(request: LoginRequest): Promise<LoginResponse> {
    await delay(NETWORK_DELAY_MS)

    const username = request.username.trim()
    if (!username || !request.password) {
      throw new AuthApiError("VALIDATION_ERROR", undefined, 400)
    }

    const account = MOCK_ACCOUNTS.find(
      (item) => item.user.username === username,
    )
    if (!account || account.password !== request.password) {
      throw new AuthApiError("INVALID_CREDENTIALS", undefined, 401)
    }

    const ttlMs = request.remember ? 24 * 60 * 60 * 1000 : 2 * 60 * 60 * 1000
    return {
      token: `mock-token-${Math.random().toString(36).slice(2, 12)}`,
      tokenType: "Bearer",
      expiresAt: new Date(Date.now() + ttlMs).toISOString(),
      user: account.user,
    }
  }

  async logout(): Promise<void> {
    await delay(200)
  }
}
