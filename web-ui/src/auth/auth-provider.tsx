import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react"

import { authApi, type LoginRequest } from "@/api/auth"
import { AuthContext, type AuthContextValue } from "./auth-context"
import {
  clearSession,
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
