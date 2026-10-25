import { createContext, useContext } from "react"

import type { LoginRequest } from "@/api/auth"
import type { AuthSession } from "./session-storage"

export interface AuthContextValue {
  session: AuthSession | null
  isAuthenticated: boolean
  /** 登录成功后短暂为 true，用于展示「登录成功」提示 */
  loginNotice: boolean
  dismissLoginNotice: () => void
  login: (request: LoginRequest) => Promise<void>
  logout: () => Promise<void>
}

export const AuthContext = createContext<AuthContextValue | null>(null)

export function useAuth(): AuthContextValue {
  const context = useContext(AuthContext)
  if (!context) {
    throw new Error("useAuth 必须在 <AuthProvider> 内部使用")
  }
  return context
}
