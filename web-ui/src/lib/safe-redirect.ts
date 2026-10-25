/**
 * 只允许站内相对路径，防止开放重定向（外部站点路径 / 协议相对 URL）。
 */
export function sanitizeFrom(value: string | null): string {
  if (!value) return "/"
  if (!value.startsWith("/") || value.startsWith("//")) return "/"
  return value
}
