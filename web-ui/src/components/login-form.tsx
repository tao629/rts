import { useCallback, useEffect, useState } from "react"
import { Controller, useForm } from "react-hook-form"
import { zodResolver } from "@hookform/resolvers/zod"
import { AlertCircle, Info, Loader2, LockKeyhole, User } from "lucide-react"
import { useNavigate, useSearchParams } from "react-router-dom"

import { authApi, isMockMode, MOCK_CREDENTIALS, toErrorMessage } from "@/api/auth"
import { useAuth } from "@/auth/auth-context"
import { Alert } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { loginSchema, type LoginFormValues } from "@/lib/login-schema"
import { sanitizeFrom } from "@/lib/safe-redirect"
import { CaptchaField } from "./captcha-field"
import { PasswordField } from "./password-field"

export function LoginForm() {
  const { login } = useAuth()
  const navigate = useNavigate()
  const [searchParams] = useSearchParams()
  const [serverError, setServerError] = useState<string | null>(null)
  const [showResetHint, setShowResetHint] = useState(false)
  const [captchaId, setCaptchaId] = useState("")
  const [captchaImage, setCaptchaImage] = useState<string | null>(null)
  // 初始为 true：首帧即处于取码中，避免短暂展示空验证码
  const [captchaLoading, setCaptchaLoading] = useState(true)
  const [captchaError, setCaptchaError] = useState<string | null>(null)

  const {
    register,
    control,
    handleSubmit,
    setValue,
    clearErrors,
    formState: { errors, isSubmitting },
  } = useForm<LoginFormValues>({
    resolver: zodResolver(loginSchema),
    mode: "onSubmit",
    defaultValues: {
      username: "",
      password: "",
      captchaCode: "",
      remember: true,
    },
  })

  /** 拉取验证码并更新界面状态 */
  const loadCaptcha = useCallback(() => {
    return authApi.fetchCaptcha().then(
      (captcha) => {
        setCaptchaId(captcha.captchaId)
        setCaptchaImage(captcha.image)
        setCaptchaError(null)
        setCaptchaLoading(false)
      },
      (error: unknown) => {
        setCaptchaId("")
        setCaptchaImage(null)
        setCaptchaError(toErrorMessage(error))
        setCaptchaLoading(false)
      },
    )
  }, [])

  /** 刷新验证码（用户点击图片 / 登录失败后自动触发） */
  const refreshCaptcha = useCallback(() => {
    setCaptchaLoading(true)
    return loadCaptcha()
  }, [loadCaptcha])

  // 页面挂载后首次自动获取验证码
  useEffect(() => {
    void loadCaptcha()
  }, [loadCaptcha])

  const onSubmit = handleSubmit(async (values) => {
    setServerError(null)
    try {
      await login({
        username: values.username,
        password: values.password,
        remember: values.remember,
        captchaId,
        captchaCode: values.captchaCode.trim(),
      })
      navigate(sanitizeFrom(searchParams.get("from")), { replace: true })
    } catch (error) {
      setServerError(toErrorMessage(error))
      setValue("password", "")
      // 验证码一次性消费，任何登录失败后都需重新取码并清空输入
      setValue("captchaCode", "")
      clearErrors("captchaCode")
      void refreshCaptcha()
    }
  })

  return (
    <div className="w-full max-w-[26rem]">
      <header className="mb-8">
        <h1 className="text-2xl font-semibold tracking-tight text-ink-900">
          欢迎回来
        </h1>
        <p className="mt-2 text-sm text-ink-500">
          登录以进入搬运工控制台
        </p>
      </header>

      {serverError && (
        <Alert className="mb-5">
          <AlertCircle className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
          <span>{serverError}</span>
        </Alert>
      )}

      <form onSubmit={onSubmit} noValidate className="space-y-5">
        <div className="space-y-2">
          <Label htmlFor="username">用户名</Label>
          <div className="relative">
            <User
              className="pointer-events-none absolute top-1/2 left-3.5 size-4 -translate-y-1/2 text-ink-400"
              aria-hidden="true"
            />
            <Input
              id="username"
              autoComplete="username"
              placeholder="请输入用户名"
              className="pl-10"
              disabled={isSubmitting}
              aria-invalid={errors.username ? true : undefined}
              aria-describedby={
                errors.username ? "username-error" : undefined
              }
              {...register("username")}
            />
          </div>
          {errors.username && (
            <p
              id="username-error"
              role="alert"
              className="text-xs text-danger-600"
            >
              {errors.username.message}
            </p>
          )}
        </div>

        <div className="space-y-2">
          <Label htmlFor="password">密码</Label>
          <PasswordField
            id="password"
            autoComplete="current-password"
            placeholder="请输入密码"
            disabled={isSubmitting}
            invalid={Boolean(errors.password)}
            aria-describedby={errors.password ? "password-error" : undefined}
            {...register("password")}
          />
          {errors.password && (
            <p
              id="password-error"
              role="alert"
              className="text-xs text-danger-600"
            >
              {errors.password.message}
            </p>
          )}
        </div>

        <div className="space-y-2">
          <Label htmlFor="captchaCode">验证码</Label>
          <CaptchaField
            image={captchaImage}
            loading={captchaLoading}
            onRefresh={() => void refreshCaptcha()}
            disabled={isSubmitting}
            aria-invalid={errors.captchaCode ? true : undefined}
            aria-describedby={
              errors.captchaCode ? "captchaCode-error" : undefined
            }
            {...register("captchaCode")}
          />
          {errors.captchaCode && (
            <p
              id="captchaCode-error"
              role="alert"
              className="text-xs text-danger-600"
            >
              {errors.captchaCode.message}
            </p>
          )}
          {captchaError && (
            <p role="alert" className="text-xs text-danger-600">
              {captchaError}
            </p>
          )}
        </div>

        <div className="flex items-center justify-between pt-1">
          <Controller
            control={control}
            name="remember"
            render={({ field }) => (
              <label
                htmlFor="remember"
                className="flex cursor-pointer items-center gap-2 text-sm text-ink-600 select-none"
              >
                <Checkbox
                  id="remember"
                  checked={field.value}
                  onCheckedChange={(checked) =>
                    field.onChange(checked === true)
                  }
                  onBlur={field.onBlur}
                  ref={field.ref}
                  disabled={isSubmitting}
                />
                记住我
              </label>
            )}
          />
          <button
            type="button"
            onClick={() => setShowResetHint((value) => !value)}
            className="text-sm text-brand-600 underline-offset-4 hover:underline"
          >
            忘记密码？
          </button>
        </div>

        {showResetHint && (
          <Alert variant="info" className="items-start">
            <Info className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
            <span>请联系系统管理员重置密码。</span>
          </Alert>
        )}

        <Button
          type="submit"
          size="lg"
          className="w-full"
          disabled={isSubmitting}
          aria-busy={isSubmitting}
        >
          {isSubmitting ? (
            <>
              <Loader2 className="size-4 animate-spin" aria-hidden="true" />
              登录中…
            </>
          ) : (
            <>
              <LockKeyhole className="size-4" aria-hidden="true" />
              登录
            </>
          )}
        </Button>
      </form>

      {isMockMode && (
        <div className="mt-8 rounded-card border border-dashed border-ink-200 bg-ink-50/70 p-4">
          <p className="text-xs font-medium text-ink-600">演示账号（mock 模式）</p>
          <ul className="mt-2 space-y-1">
            {MOCK_CREDENTIALS.map((credential) => (
              <li
                key={credential.username}
                className="font-mono text-xs text-ink-500"
              >
                {credential.username} / {credential.password}
              </li>
            ))}
          </ul>
        </div>
      )}

      <p className="mt-6 text-center text-xs text-ink-400">
        RTS 搬运工 · 内部系统，请勿外传凭证
      </p>
    </div>
  )
}
