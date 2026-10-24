import { z } from "zod"

export const loginSchema = z.object({
  username: z
    .string()
    .trim()
    .min(1, "请输入用户名")
    .max(64, "用户名长度不能超过 64 个字符"),
  password: z
    .string()
    .min(1, "请输入密码")
    .max(64, "密码长度不能超过 64 个字符"),
  remember: z.boolean(),
})

export type LoginFormValues = z.infer<typeof loginSchema>
