import { Navigate, Route, Routes } from "react-router-dom"

import { ProtectedRoute, PublicOnlyRoute } from "@/auth/protected-route"
import DashboardPage from "@/pages/dashboard-page"
import LoginPage from "@/pages/login-page"

export function AppRoutes() {
  return (
    <Routes>
      <Route
        path="/login"
        element={
          <PublicOnlyRoute>
            <LoginPage />
          </PublicOnlyRoute>
        }
      />
      <Route
        path="/"
        element={
          <ProtectedRoute>
            <DashboardPage />
          </ProtectedRoute>
        }
      />
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  )
}
