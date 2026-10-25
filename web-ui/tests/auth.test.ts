import { describe, expect, test } from "bun:test"

import { AuthApiError, toErrorMessage } from "../src/api/auth/errors"
import { MOCK_CREDENTIALS, MockAuthApi } from "../src/api/auth/mock-auth-api"
import { loginSchema } from "../src/lib/login-schema"
import { sanitizeFrom } from "../src/lib/safe-redirect"
import {
  clearSession,
  readSession,
  writeSession,
  type AuthSession,
} from "../src/auth/session-storage"

/**
 * mock 模式的演示账号。口令从 mock 实现导出的清单里取，
 * 避免在测试代码里写死口令字面量。
 */
const DEMO_PASSWORD = MOCK_CREDENTIALS[0]?.password ?? ""

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

/**
 * 从 mock 生成的验证码图片里还原答案。
 * 答案只存在于图片本身（每个字符一个 <text> 节点），
 * 这样测试可以走完整链路，而无需 mock 实现向外暴露答案。
 */
function readCaptchaAnswer(image: string): string {
  const base64 = image.slice(image.indexOf(",") + 1)
  const svg = atob(base64)
  const chars = [...svg.matchAll(/<text[^>]*>([^<]*)<\/text>/g)].map(
    (match) => match[1] ?? "",
  )
  return chars.join("")
}

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
      password: DEMO_PASSWORD,
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
      password: DEMO_PASSWORD,
      captchaCode: "8F3K",
      remember: false,
    })
    expect(result.success).toBe(false)
  })

  test("验证码为空时提示请输入验证码", () => {
    const result = loginSchema.safeParse({
      username: "admin",
      password: DEMO_PASSWORD,
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
      password: DEMO_PASSWORD,
      captchaCode: "8F3",
      remember: true,
    })
    expect(short.success).toBe(false)
    if (!short.success) {
      expect(short.error.issues[0]?.message).toBe("验证码格式不正确")
    }

    const long = loginSchema.safeParse({
      username: "admin",
      password: DEMO_PASSWORD,
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
      password: DEMO_PASSWORD,
      captchaCode: " 8F3K ",
      remember: true,
    })
    expect(result.success).toBe(true)
    if (result.success) {
      expect(result.data.captchaCode).toBe("8F3K")
    }
  })
})

describe("mock 认证接口", () => {
  const api = new MockAuthApi()

  test("正确口令返回 token 与用户信息", async () => {
    const captcha = await api.fetchCaptcha()
    const result = await api.login({
      username: "admin",
      password: DEMO_PASSWORD,
      remember: true,
      captchaId: captcha.captchaId,
      captchaCode: readCaptchaAnswer(captcha.image),
    })
    expect(result.token.startsWith("mock-token-")).toBe(true)
    expect(result.user.username).toBe("admin")
    expect(new Date(result.expiresAt).getTime()).toBeGreaterThan(Date.now())
  })

  test("口令错误抛出 INVALID_CREDENTIALS", async () => {
    const captcha = await api.fetchCaptcha()
    expect(
      api.login({
        username: "admin",
        password: "wrong",
        remember: true,
        captchaId: captcha.captchaId,
        captchaCode: readCaptchaAnswer(captcha.image),
      }),
    ).rejects.toThrow(AuthApiError)
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
  })
})

describe("mock 验证码", () => {
  const api = new MockAuthApi()

  test("fetchCaptcha 返回可用的 captchaId 与 base64 图片", async () => {
    const captcha = await api.fetchCaptcha()
    expect(captcha.captchaId.length).toBeGreaterThan(0)
    expect(captcha.image.startsWith("data:image/svg+xml;base64,")).toBe(true)
    expect(new Date(captcha.expiresAt).getTime()).toBeGreaterThan(Date.now())
    // 答案为 4 位，取自易混字符剔除后的字符集
    const answer = readCaptchaAnswer(captcha.image)
    expect(answer).toHaveLength(4)
    expect(/^[23456789ABCDEFGHJKMNPQRSTUVWXYZ]{4}$/.test(answer)).toBe(true)
  })

  test("验证码错误时抛出 INVALID_CAPTCHA", async () => {
    const captcha = await api.fetchCaptcha()
    expect(
      api.login({
        username: "admin",
        password: DEMO_PASSWORD,
        remember: true,
        captchaId: captcha.captchaId,
        captchaCode: "WRONG",
      }),
    ).rejects.toMatchObject({ code: "INVALID_CAPTCHA" })
  })

  test("缺少验证码字段时抛出 INVALID_CAPTCHA", async () => {
    expect(
      api.login({
        username: "admin",
        password: DEMO_PASSWORD,
        remember: true,
        captchaId: "",
        captchaCode: "",
      }),
    ).rejects.toMatchObject({ code: "INVALID_CAPTCHA" })
  })

  test("验证码一次性消费：同一 captchaId 第二次使用判失败", async () => {
    const captcha = await api.fetchCaptcha()
    const answer = readCaptchaAnswer(captcha.image)
    await api.login({
      username: "admin",
      password: DEMO_PASSWORD,
      remember: true,
      captchaId: captcha.captchaId,
      captchaCode: answer,
    })
    expect(
      api.login({
        username: "admin",
        password: DEMO_PASSWORD,
        remember: true,
        captchaId: captcha.captchaId,
        captchaCode: answer,
      }),
    ).rejects.toMatchObject({ code: "INVALID_CAPTCHA" })
  })

  test("校验失败同样作废验证码（一次性消费）", async () => {
    const captcha = await api.fetchCaptcha()
    const answer = readCaptchaAnswer(captcha.image)
    await expect(
      api.login({
        username: "admin",
        password: DEMO_PASSWORD,
        remember: true,
        captchaId: captcha.captchaId,
        captchaCode: "NOPE",
      }),
    ).rejects.toMatchObject({ code: "INVALID_CAPTCHA" })
    expect(
      api.login({
        username: "admin",
        password: DEMO_PASSWORD,
        remember: true,
        captchaId: captcha.captchaId,
        captchaCode: answer,
      }),
    ).rejects.toMatchObject({ code: "INVALID_CAPTCHA" })
  })

  test("验证码先于口令校验：口令错但验证码对时返回 INVALID_CREDENTIALS", async () => {
    const captcha = await api.fetchCaptcha()
    const answer = readCaptchaAnswer(captcha.image)
    expect(
      api.login({
        username: "admin",
        password: "wrong",
        remember: true,
        captchaId: captcha.captchaId,
        captchaCode: answer,
      }),
    ).rejects.toMatchObject({ code: "INVALID_CREDENTIALS" })
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
