import { AuthApiError } from "./errors"
import type {
  AuthApi,
  AuthUser,
  CaptchaResponse,
  LoginRequest,
  LoginResponse,
} from "./types"

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
const CAPTCHA_DELAY_MS = 150

/** 验证码字符集：剔除 0/O/1/I/L 等易混字符 */
const CAPTCHA_CHARSET = "23456789ABCDEFGHJKMNPQRSTUVWXYZ"
const CAPTCHA_LENGTH = 4
const CAPTCHA_TTL_MS = 5 * 60 * 1000
const CAPTCHA_WIDTH = 260
const CAPTCHA_HEIGHT = 60

/** 验证码答案只保存在 mock 实现内部（模块级存储），前端代码读不到 */
interface MockCaptcha {
  /** 验证码答案（大写） */
  answer: string
  /** 过期时刻（毫秒时间戳） */
  expiresAt: number
}

const captchaStore = new Map<string, MockCaptcha>()

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

/** 生成 [0, max) 的随机整数，优先使用密码学随机源 */
function randomInt(max: number): number {
  const cryptoObj = globalThis.crypto
  if (cryptoObj?.getRandomValues) {
    const buffer = new Uint32Array(1)
    cryptoObj.getRandomValues(buffer)
    return buffer[0] % max
  }
  return Math.floor(Math.random() * max)
}

function randomChar(): string {
  return CAPTCHA_CHARSET[randomInt(CAPTCHA_CHARSET.length)]
}

/** 生成验证码 ID（随机十六进制串） */
function randomCaptchaId(): string {
  let id = ""
  for (let i = 0; i < 16; i += 1) {
    id += randomInt(16).toString(16)
  }
  return id
}

/** 字符颜色池：深色系，避免与浅底冲突 */
const CAPTCHA_COLORS = [
  "#1e293b",
  "#334155",
  "#0369a1",
  "#0f766e",
  "#7c2d12",
  "#9d174d",
  "#4c1d95",
]

/**
 * 渲染 SVG 验证码：字符 + 噪点 + 干扰线 + 字符扭曲。
 * 每个字符单独放在 <text> 元素里，便于自动化测试从图片中还原答案。
 */
function renderCaptchaSvg(code: string): string {
  const parts: string[] = []
  parts.push(
    `<svg xmlns="http://www.w3.org/2000/svg" width="${CAPTCHA_WIDTH}" height="${CAPTCHA_HEIGHT}" viewBox="0 0 ${CAPTCHA_WIDTH} ${CAPTCHA_HEIGHT}">`,
  )
  parts.push(
    `<rect width="${CAPTCHA_WIDTH}" height="${CAPTCHA_HEIGHT}" rx="6" fill="#f1f5f9"/>`,
  )

  // 干扰线
  for (let i = 0; i < 4; i += 1) {
    const x1 = randomInt(CAPTCHA_WIDTH)
    const y1 = randomInt(CAPTCHA_HEIGHT)
    const x2 = randomInt(CAPTCHA_WIDTH)
    const y2 = randomInt(CAPTCHA_HEIGHT)
    const color = CAPTCHA_COLORS[randomInt(CAPTCHA_COLORS.length)]
    parts.push(
      `<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="${color}" stroke-width="1" opacity="0.35"/>`,
    )
  }

  // 噪点
  for (let i = 0; i < 40; i += 1) {
    const cx = randomInt(CAPTCHA_WIDTH)
    const cy = randomInt(CAPTCHA_HEIGHT)
    const r = 1 + randomInt(2)
    const color = CAPTCHA_COLORS[randomInt(CAPTCHA_COLORS.length)]
    parts.push(`<circle cx="${cx}" cy="${cy}" r="${r}" fill="${color}" opacity="0.4"/>`)
  }

  // 字符：随机旋转 / 斜切 / 字号 / 颜色 / 纵向偏移
  const step = CAPTCHA_WIDTH / (CAPTCHA_LENGTH + 1)
  for (let i = 0; i < code.length; i += 1) {
    const char = code[i] ?? ""
    const cx = Math.round(step * (i + 1))
    const cy = 38 + randomInt(8) - 4
    const rotate = randomInt(45) - 22
    const skew = randomInt(20) - 10
    const size = 28 + randomInt(8)
    const color = CAPTCHA_COLORS[randomInt(CAPTCHA_COLORS.length)]
    parts.push(
      `<text x="${cx}" y="${cy}" font-family="monospace" font-size="${size}" font-weight="600" fill="${color}" text-anchor="middle" transform="rotate(${rotate} ${cx} ${cy}) skewX(${skew})">${char}</text>`,
    )
  }

  parts.push("</svg>")
  return parts.join("")
}

/** SVG 转 base64 data URI（内容均为 ASCII，可直接用 btoa 编码） */
function toDataUri(svg: string): string {
  return `data:image/svg+xml;base64,${btoa(svg)}`
}

/** 清理过期验证码 */
function purgeExpiredCaptcha(): void {
  const now = Date.now()
  for (const [id, item] of captchaStore) {
    if (item.expiresAt <= now) captchaStore.delete(id)
  }
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

    // ① 先校验验证码（一次性消费），再校验账号口令
    this.consumeCaptcha(request.captchaId, request.captchaCode)

    // ② 校验账号口令
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

  /** 浏览器端生成同规格的图形验证码 */
  async fetchCaptcha(): Promise<CaptchaResponse> {
    await delay(CAPTCHA_DELAY_MS)
    purgeExpiredCaptcha()

    let answer = ""
    for (let i = 0; i < CAPTCHA_LENGTH; i += 1) {
      answer += randomChar()
    }

    const captchaId = randomCaptchaId()
    const expiresAt = Date.now() + CAPTCHA_TTL_MS
    captchaStore.set(captchaId, { answer, expiresAt })

    return {
      captchaId,
      image: toDataUri(renderCaptchaSvg(answer)),
      expiresAt: new Date(expiresAt).toISOString(),
    }
  }

  /**
   * 校验并消费验证码：无论校验成功与否，一次尝试后该验证码立即失效。
   * 缺失 / 不匹配 / 过期 / 已使用一律判失败。
   */
  private consumeCaptcha(captchaId: string, captchaCode: string): void {
    const id = (captchaId ?? "").trim()
    const code = (captchaCode ?? "").trim().toUpperCase()

    if (!id) {
      throw new AuthApiError("INVALID_CAPTCHA", undefined, 400)
    }

    const item = captchaStore.get(id)
    if (!item) {
      // 未知或已被消费的验证码
      throw new AuthApiError("INVALID_CAPTCHA", undefined, 400)
    }

    // 一次性消费：取出即失效（校验成功或失败都会作废该验证码）
    captchaStore.delete(id)

    if (!code || item.expiresAt <= Date.now() || item.answer !== code) {
      throw new AuthApiError("INVALID_CAPTCHA", undefined, 400)
    }
  }
}
