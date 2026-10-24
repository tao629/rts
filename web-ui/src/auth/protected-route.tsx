import type { ReactNode } from "react"
import { Navigate, useLocation } from "react-router-dom"

import { useAuth } from "./auth-context"

/** 受保护页面：无有效登录态时跳转到登录页，并带上原始路径 */
export function ProtectedRoute({ children }: { children: ReactNode }) {
  const { session } = useAuth()
  const location = useLocation()

  if (!session) {
    const from = encodeURIComponent(location.pathname + location.search)
    return <Navigate to={`/login?from=${from}`} replace />
  }

  return <>{children}</>
}

/** 登录页：已登录时直接进入首页 */
export function PublicOnlyRoute({ children }: { children: ReactNode }) {
  const { session } = useAuth()

  if (session) {
    return <Navigate to="/" replace />
  }

  return <>{children}</>
}
