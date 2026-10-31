import { afterEach, describe, expect, test } from "bun:test"

import { AuthApiError, toErrorMessage } from "../src/api/auth/errors"
import { HttpAuthApi } from "../src/api/auth/http-auth-api"
import { loginSchema } from "../src/lib/login-schema"
import { sanitizeFrom } from "../src/lib/safe-redirect"
import {
  clearSession,
  isRemembered,
  readSession,
  writeSession,
  type AuthSession,
} from "../src/auth/session-storage"

/**
 * 测试用的口令：运行时随机生成，仓库里不出现口令字面量。
 */
const DUMMY_PASSWORD = crypto.randomUUID()

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

interface StubResult {
  status?: number
  body?: unknown
}

/** 被记录下来的请求。 */
interface RecordedCall {
  url: string
  init?: RequestInit
}

/**
 * 用打桩替换全局 fetch。
 *
 * 前端不再有 mock 认证实现，接口行为由后端裁决；
 * 这里只桩住 HTTP 边界，验证前端对后端契约的处理。
 */
function stubFetch(
  handler: (url: string, init?: RequestInit) => StubResult | Promise<StubResult>,
): RecordedCall[] {
  const calls: RecordedCall[] = []
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = typeof input === "string" ? input : String(input)
    calls.push({ url, init })
    const result = await handler(url, init)
    const status = result.status ?? 200
    return {
      ok: status >= 200 && status < 300,
      status,
      json: async () => result.body,
    } as Response
  }) as typeof fetch
  return calls
}

const originalFetch = globalThis.fetch

afterEach(() => {
  globalThis.fetch = originalFetch
})

describe("登录表单校验", () => {
  test("空用户名与空密码分别给出提示", () => {
    const result = loginSchema.safeParse({
      username: "",
      password: "",
      captchaCode: "8F3K",
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
      password: DUMMY_PASSWORD,
      captchaCode: "8F3K",
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
      password: DUMMY_PASSWORD,
      captchaCode: "8F3K",
      remember: false,
    })
    expect(result.success).toBe(false)
  })

  test("验证码为空时提示请输入验证码", () => {
    const result = loginSchema.safeParse({
      username: "admin",
      password: DUMMY_PASSWORD,
      captchaCode: "",
      remember: true,
    })
    expect(result.success).toBe(false)
    if (result.success) return
    expect(result.error.issues[0]?.message).toBe("请输入验证码")
  })

  test("验证码长度不在 4~6 之间时提示格式不正确", () => {
    const short = loginSchema.safeParse({
      username: "admin",
      password: DUMMY_PASSWORD,
      captchaCode: "8F3",
      remember: true,
    })
    expect(short.success).toBe(false)
    if (!short.success) {
      expect(short.error.issues[0]?.message).toBe("验证码格式不正确")
    }

    const long = loginSchema.safeParse({
      username: "admin",
      password: DUMMY_PASSWORD,
      captchaCode: "8F3K9XQ",
      remember: true,
    })
    expect(long.success).toBe(false)
    if (!long.success) {
      expect(long.error.issues[0]?.message).toBe("验证码格式不正确")
    }
  })

  test("4~6 位验证码视为有效", () => {
    const result = loginSchema.safeParse({
      username: "admin",
      password: DUMMY_PASSWORD,
      captchaCode: " 8F3K ",
      remember: true,
    })
    expect(result.success).toBe(true)
    if (result.success) {
      expect(result.data.captchaCode).toBe("8F3K")
    }
  })
})

describe("认证接口（fetch 打桩）", () => {
  test("登录成功返回令牌与用户信息", async () => {
    const calls = stubFetch(() => ({
      body: {
        token: "jwt-token",
        tokenType: "Bearer",
        expiresAt: new Date(Date.now() + 7200_000).toISOString(),
        user: {
          id: "1",
          username: "admin",
          displayName: "系统管理员",
          roles: ["super_admin"],
        },
      },
    }))

    const api = new HttpAuthApi("/api/v1")
    const result = await api.login({
      username: "admin",
      password: DUMMY_PASSWORD,
      remember: true,
      captchaId: "captcha-1",
      captchaCode: "AB3D",
    })

    expect(result.token).toBe("jwt-token")
    expect(result.tokenType).toBe("Bearer")
    expect(result.user.username).toBe("admin")
    expect(new Date(result.expiresAt).getTime()).toBeGreaterThan(Date.now())

    // 请求体里带上验证码 ID 与内容，且字段名与后端契约一致。
    expect(calls[0]?.url).toBe("/api/v1/auth/login")
    const payload = JSON.parse(String(calls[0]?.init?.body))
    expect(payload.captchaId).toBe("captcha-1")
    expect(payload.captchaCode).toBe("AB3D")
    expect(payload.username).toBe("admin")
  })

  test("后端返回 401 时抛出 INVALID_CREDENTIALS", async () => {
    stubFetch(() => ({
      status: 401,
      body: { code: "INVALID_CREDENTIALS", message: "用户名或密码错误" },
    }))

    const api = new HttpAuthApi("/api/v1")
    expect(
      api.login({
        username: "admin",
        password: DUMMY_PASSWORD,
        remember: true,
        captchaId: "captcha-1",
        captchaCode: "AB3D",
      }),
    ).rejects.toMatchObject({ code: "INVALID_CREDENTIALS" })
  })

  test("后端返回 400 时抛出 INVALID_CAPTCHA", async () => {
    stubFetch(() => ({
      status: 400,
      body: { code: "INVALID_CAPTCHA", message: "验证码错误或已失效" },
    }))

    const api = new HttpAuthApi("/api/v1")
    expect(
      api.login({
        username: "admin",
        password: DUMMY_PASSWORD,
        remember: true,
        captchaId: "captcha-1",
        captchaCode: "WRONG",
      }),
    ).rejects.toMatchObject({ code: "INVALID_CAPTCHA" })
  })

  test("账号被停用时抛出 ACCOUNT_DISABLED", async () => {
    stubFetch(() => ({
      status: 403,
      body: { code: "ACCOUNT_DISABLED", message: "账号已停用" },
    }))

    const api = new HttpAuthApi("/api/v1")
    expect(
      api.login({
        username: "admin",
        password: DUMMY_PASSWORD,
        remember: true,
        captchaId: "captcha-1",
        captchaCode: "AB3D",
      }),
    ).rejects.toMatchObject({ code: "ACCOUNT_DISABLED" })
  })

  test("请求发不出去时抛出 NETWORK_ERROR", async () => {
    globalThis.fetch = (async () => {
      throw new TypeError("Failed to fetch")
    }) as typeof fetch

    const api = new HttpAuthApi("/api/v1")
    expect(
      api.login({
        username: "admin",
        password: DUMMY_PASSWORD,
        remember: true,
        captchaId: "captcha-1",
        captchaCode: "AB3D",
      }),
    ).rejects.toMatchObject({ code: "NETWORK_ERROR" })
  })

  test("取验证码带上 no-store，且返回三项字段", async () => {
    const calls = stubFetch(() => ({
      body: {
        captchaId: "captcha-1",
        image: "data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=",
        expiresAt: new Date(Date.now() + 300_000).toISOString(),
      },
    }))

    const api = new HttpAuthApi("/api/v1")
    const captcha = await api.fetchCaptcha()

    expect(captcha.captchaId).toBe("captcha-1")
    expect(captcha.image.startsWith("data:image/svg+xml;base64,")).toBe(true)
    expect(new Date(captcha.expiresAt).getTime()).toBeGreaterThan(Date.now())
    expect(calls[0]?.url).toBe("/api/v1/auth/captcha")
    expect(calls[0]?.init?.cache).toBe("no-store")
  })

  test("fetchMe 用 Bearer 令牌换回用户信息", async () => {
    const calls = stubFetch(() => ({
      body: {
        id: "1",
        username: "admin",
        displayName: "系统管理员",
        roles: ["super_admin"],
      },
    }))

    const api = new HttpAuthApi("/api/v1")
    const user = await api.fetchMe("jwt-token")

    expect(user.username).toBe("admin")
    expect(calls[0]?.url).toBe("/api/v1/auth/me")
    const headers = calls[0]?.init?.headers as Record<string, string> | undefined
    expect(headers?.Authorization).toBe("Bearer jwt-token")
  })

  test("fetchMe 遇到 401 时抛出 TOKEN_INVALID", async () => {
    stubFetch(() => ({
      status: 401,
      body: { code: "TOKEN_INVALID", message: "令牌无效或已过期" },
    }))

    const api = new HttpAuthApi("/api/v1")
    expect(api.fetchMe("stale-token")).rejects.toMatchObject({
      code: "TOKEN_INVALID",
    })
  })

  test("fetchMe 遇到 403 时抛出 ACCOUNT_DISABLED", async () => {
    stubFetch(() => ({
      status: 403,
      body: { code: "ACCOUNT_DISABLED", message: "账号已停用" },
    }))

    const api = new HttpAuthApi("/api/v1")
    expect(api.fetchMe("jwt-token")).rejects.toMatchObject({
      code: "ACCOUNT_DISABLED",
    })
  })

  test("登出失败不抛错", async () => {
    globalThis.fetch = (async () => {
      throw new TypeError("Failed to fetch")
    }) as typeof fetch

    const api = new HttpAuthApi("/api/v1")
    await api.logout("jwt-token")
  })

  test("错误码映射为中文提示", () => {
    expect(toErrorMessage(new AuthApiError("INVALID_CREDENTIALS"))).toBe(
      "用户名或密码错误",
    )
    expect(toErrorMessage(new AuthApiError("INVALID_CAPTCHA"))).toBe(
      "验证码错误或已失效，请重新输入",
    )
    expect(toErrorMessage(new AuthApiError("NETWORK_ERROR"))).toBe(
      "无法连接到服务器，请检查网络后重试",
    )
    expect(toErrorMessage(new AuthApiError("TOKEN_INVALID"))).toBe(
      "登录状态已失效，请重新登录",
    )
    expect(toErrorMessage(new AuthApiError("ACCOUNT_DISABLED"))).toBe(
      "该账号已被停用，请联系管理员",
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
    expect(isRemembered()).toBe(true)
  })

  test("remember=false 落在 sessionStorage 并清理 localStorage", () => {
    const { local, session } = fakeWindow()
    writeSession(sessionFixture, false)
    expect(local.getItem("rts.auth.session")).toBeNull()
    expect(session.getItem("rts.auth.session")).not.toBeNull()
    expect(readSession()?.token).toBe("token-1")
    expect(isRemembered()).toBe(false)
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
