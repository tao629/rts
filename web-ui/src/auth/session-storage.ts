import type { AuthUser } from "@/api/auth"

export interface AuthSession {
  token: string
  tokenType: string
  /** ISO 8601 时间戳 */
  expiresAt: string
  user: AuthUser
}

const STORAGE_KEY = "rts.auth.session"

function isAuthSession(value: unknown): value is AuthSession {
  if (typeof value !== "object" || value === null) return false
  const session = value as Partial<AuthSession>
  return (
    typeof session.token === "string" &&
    session.token.length > 0 &&
    typeof session.expiresAt === "string" &&
    typeof session.user === "object" &&
    session.user !== null
  )
}

function parse(raw: string | null): AuthSession | null {
  if (!raw) return null
  try {
    const parsed: unknown = JSON.parse(raw)
    if (!isAuthSession(parsed)) return null
    if (new Date(parsed.expiresAt).getTime() <= Date.now()) return null
    return parsed
  } catch {
    return null
  }
}

function readFrom(storage: Storage): AuthSession | null {
  try {
    return parse(storage.getItem(STORAGE_KEY))
  } catch {
    return null
  }
}

/** 读取当前登录态：先 localStorage（记住我），后 sessionStorage */
export function readSession(): AuthSession | null {
  if (typeof window === "undefined") return null
  return readFrom(window.localStorage) ?? readFrom(window.sessionStorage)
}

/** 写入登录态；remember=false 时只落在 sessionStorage，关闭标签页即失效 */
export function writeSession(session: AuthSession, remember: boolean): void {
  if (typeof window === "undefined") return
  const payload = JSON.stringify(session)
  try {
    if (remember) {
      window.localStorage.setItem(STORAGE_KEY, payload)
      window.sessionStorage.removeItem(STORAGE_KEY)
    } else {
      window.sessionStorage.setItem(STORAGE_KEY, payload)
      window.localStorage.removeItem(STORAGE_KEY)
    }
  } catch {
    // 存储不可用（如隐私模式）时降级为内存态
  }
}

/** 清除两处存储中的凭证 */
export function clearSession(): void {
  if (typeof window === "undefined") return
  try {
    window.localStorage.removeItem(STORAGE_KEY)
    window.sessionStorage.removeItem(STORAGE_KEY)
  } catch {
    // ignore
  }
}
