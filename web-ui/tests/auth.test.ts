import { describe, expect, test } from "bun:test"

import { AuthApiError, toErrorMessage } from "../src/api/auth/errors"
import { MockAuthApi } from "../src/api/auth/mock-auth-api"
import { loginSchema } from "../src/lib/login-schema"
import { sanitizeFrom } from "../src/lib/safe-redirect"
import {
  clearSession,
  readSession,
  writeSession,
  type AuthSession,
} from "../src/auth/session-storage"

function createStorage(): Storage {
  const map = new Map<string, string>()
  return {
    get length() {
      return map.size
    },
    clear: () => map.clear(),
    getItem: (key: string) => map.get(key) ?? null,
    key: (index: number) => [...map.keys()][index] ?? null,
    removeItem: (key: string) => void map.delete(key),
    setItem: (key: string, value: string) => void map.set(key, value),
  }
}

function fakeWindow() {
  const local = createStorage()
  const session = createStorage()
  // @ts-expect-error —— 仅测试用的最小 window 替身
  globalThis.window = { localStorage: local, sessionStorage: session }
  return { local, session }
}

const sessionFixture: AuthSession = {
  token: "token-1",
  tokenType: "Bearer",
  expiresAt: new Date(Date.now() + 60_000).toISOString(),
  user: {
    id: "u-1",
    username: "admin",
    displayName: "系统管理员",
    roles: ["super_admin"],
  },
}

describe("登录表单校验", () => {
  test("空用户名与空密码分别给出提示", () => {
    const result = loginSchema.safeParse({
      username: "",
      password: "",
      remember: true,
    })
    expect(result.success).toBe(false)
    if (result.success) return
    expect(result.error.issues[0]?.message).toBe("请输入用户名")
    expect(result.error.issues[1]?.message).toBe("请输入密码")
  })

  test("用户名首尾空格被裁剪后仍视为有效", () => {
    const result = loginSchema.safeParse({
      username: "  admin  ",
      password: "Admin@123",
      remember: true,
    })
    expect(result.success).toBe(true)
    if (result.success) {
      expect(result.data.username).toBe("admin")
    }
  })

  test("纯空格的用户名判定为缺失", () => {
    const result = loginSchema.safeParse({
      username: "   ",
      password: "Admin@123",
      remember: false,
    })
    expect(result.success).toBe(false)
  })
})

describe("mock 认证接口", () => {
  const api = new MockAuthApi()

  test("正确口令返回 token 与用户信息", async () => {
    const result = await api.login({
      username: "admin",
      password: "Admin@123",
      remember: true,
    })
    expect(result.token.startsWith("mock-token-")).toBe(true)
    expect(result.user.username).toBe("admin")
    expect(new Date(result.expiresAt).getTime()).toBeGreaterThan(Date.now())
  })

  test("口令错误抛出 INVALID_CREDENTIALS", async () => {
    expect(
      api.login({
        username: "admin",
        password: "wrong",
        remember: true,
      }),
    ).rejects.toThrow(AuthApiError)
  })

  test("错误码映射为中文提示", () => {
    expect(toErrorMessage(new AuthApiError("INVALID_CREDENTIALS"))).toBe(
      "用户名或密码错误",
    )
    expect(toErrorMessage(new AuthApiError("NETWORK_ERROR"))).toBe(
      "无法连接到服务器，请检查网络后重试",
    )
  })
})

describe("登录态存储", () => {
  test("remember=true 落在 localStorage 并清理 sessionStorage", () => {
    const { local, session } = fakeWindow()
    writeSession(sessionFixture, true)
    expect(local.getItem("rts.auth.session")).not.toBeNull()
    expect(session.getItem("rts.auth.session")).toBeNull()
    expect(readSession()?.token).toBe("token-1")
  })

  test("remember=false 落在 sessionStorage 并清理 localStorage", () => {
    const { local, session } = fakeWindow()
    writeSession(sessionFixture, false)
    expect(local.getItem("rts.auth.session")).toBeNull()
    expect(session.getItem("rts.auth.session")).not.toBeNull()
    expect(readSession()?.token).toBe("token-1")
  })

  test("已过期的登录态被判定为无效并清除", () => {
    const { local } = fakeWindow()
    local.setItem(
      "rts.auth.session",
      JSON.stringify({
        ...sessionFixture,
        expiresAt: new Date(Date.now() - 1000).toISOString(),
      }),
    )
    expect(readSession()).toBeNull()
    clearSession()
    expect(local.getItem("rts.auth.session")).toBeNull()
  })

  test("损坏的存储内容不会导致崩溃", () => {
    const { local } = fakeWindow()
    local.setItem("rts.auth.session", "{not-json")
    expect(readSession()).toBeNull()
  })
})

describe("登录后跳转目标", () => {
  test("站内相对路径被放行", () => {
    expect(sanitizeFrom("/tasks/42")).toBe("/tasks/42")
  })

  test("外部地址与协议相对地址回退到首页", () => {
    expect(sanitizeFrom("https://evil.example.com")).toBe("/")
    expect(sanitizeFrom("//evil.example.com")).toBe("/")
    expect(sanitizeFrom(null)).toBe("/")
  })
})
