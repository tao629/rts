import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react"

import { AuthApiError, authApi, type LoginRequest } from "@/api/auth"
import { AuthContext, type AuthContextValue } from "./auth-context"
import {
  clearSession,
  isRemembered,
  readSession,
  writeSession,
  type AuthSession,
} from "./session-storage"

/** 登录成功提示的展示时长 */
const LOGIN_NOTICE_MS = 3000

export function AuthProvider({ children }: { children: ReactNode }) {
  const [session, setSession] = useState<AuthSession | null>(() =>
    readSession(),
  )
  const [loginNotice, setLoginNotice] = useState(false)
  const noticeTimer = useRef<ReturnType<typeof setTimeout> | null>(null)

  const dismissLoginNotice = useCallback(() => {
    setLoginNotice(false)
  }, [])

  const login = useCallback(async (request: LoginRequest) => {
    const response = await authApi.login(request)
    const next: AuthSession = {
      token: response.token,
      tokenType: response.tokenType,
      expiresAt: response.expiresAt,
      user: response.user,
    }
    writeSession(next, request.remember)
    setSession(next)

    setLoginNotice(true)
    if (noticeTimer.current) clearTimeout(noticeTimer.current)
    noticeTimer.current = setTimeout(() => {
      setLoginNotice(false)
    }, LOGIN_NOTICE_MS)
  }, [])

  const logout = useCallback(async () => {
    const token = session?.token
    if (token) {
      try {
        await authApi.logout(token)
      } catch {
        // 登出接口失败不阻断本地清理
      }
    }
    clearSession()
    setSession(null)
    setLoginNotice(false)
  }, [session?.token])

  /**
   * 挂载时向后端确认登录态。
   *
   * 登录态由服务端裁决：本地存着的令牌可能已经过期、被停用账号失效，
   * 所以刷新页面后要拿 `/auth/me` 换一次用户信息：
   * - 成功：用后端返回的用户信息刷新本地会话；
   * - 401 / 403：登录态不再成立，清干净按未登录处理；
   * - 网络错误：**保留**本地会话——网络抖动不该把人踢出登录态。
   */
  useEffect(() => {
    const current = readSession()
    if (!current) return

    let cancelled = false
    void (async () => {
      try {
        const user = await authApi.fetchMe(current.token)
        if (cancelled) return
        const next: AuthSession = { ...current, user }
        writeSession(next, isRemembered())
        setSession(next)
      } catch (error) {
        if (cancelled) return
        if (error instanceof AuthApiError && error.code === "NETWORK_ERROR") {
          console.warn("校验登录态失败（网络不可用），暂时保留本地登录态")
          return
        }
        clearSession()
        setSession(null)
      }
    })()

    return () => {
      cancelled = true
    }
  }, [])

  useEffect(
    () => () => {
      if (noticeTimer.current) clearTimeout(noticeTimer.current)
    },
    [],
  )

  const value = useMemo<AuthContextValue>(
    () => ({
      session,
      isAuthenticated: session !== null,
      loginNotice,
      dismissLoginNotice,
      login,
      logout,
    }),
    [session, loginNotice, dismissLoginNotice, login, logout],
  )

  return <AuthContext value={value}>{children}</AuthContext>
}
