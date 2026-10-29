# 开发计划（Plan）: web-ui SPA 前端 Docker 镜像

> 依据 [`spec.md`](./spec.md) 编写。开发在独立 worktree `.claude/worktrees/webui-docker/` 下进行，主仓只读。

## 1. 改动清单与顺序

| # | 文件 | 动作 | 说明 |
|---|---|---|---|
| 1 | `web-ui/.dockerignore` | 新增 | 排除 `node_modules`、`dist`、`.env.local`、`.git`、测试与文档产物 |
| 2 | `web-ui/docker/nginx.conf` | 新增 | nginx 主配置：SPA fallback、缓存、gzip、`/api` 反代、`/healthz` |
| 3 | `web-ui/docker/entrypoint.sh` | 新增 | 用 `envsubst` 渲染 `API_UPSTREAM` → `nginx.conf.template`；校验必填；切换到非 root |
| 4 | `web-ui/Dockerfile` | 新增 | 多阶段构建（bun 构建 → nginx 托管），非 root、8080 端口、HEALTHCHECK |
| 5 | `web-ui/package.json` | 修改 | 增加 `docker:build` / `docker:run` script |
| 6 | `web-ui/README.md` | 修改 | 追加「容器化」章节：构建、运行、后端依赖、常见问题 |
| 7 | `sdlc/20261029_webui_docker/verify.md` | 新增 | 验证记录（构建日志要点、冒烟结果、截图/结论） |

依赖关系：`1` → `4`（构建上下文）；`2` + `3` → `4`（运行期）。

## 2. 关键实现要点

### 2.1 Dockerfile

```dockerfile
# --- builder ---
FROM oven/bun:1.3.14-alpine AS builder
WORKDIR /app
ARG VITE_API_MODE=real
ARG VITE_API_BASE_URL=/api/v1
ENV VITE_API_MODE=$VITE_API_MODE VITE_API_BASE_URL=$VITE_API_BASE_URL
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile
COPY . .
RUN bun run build          # tsc -b && vite build → /app/dist

# --- runtime ---
FROM nginx:alpine
```

- `bun install` 与 `bun run build` 分两层：依赖层可复用源码改动带来的缓存失效。
- `COPY . .` 放在依赖安装之后，且 `.dockerignore` 排除了 `node_modules`/`dist`。
- runtime 阶段：`COPY --from=builder /app/dist /usr/share/nginx/html`，非 root 需先修 `/var/cache/nginx`、`/var/run` 属主，`nginx.conf` 里 `pid` 指到可写路径、临时目录指到 `/tmp`。
- `EXPOSE 8080`；`HEALTHCHECK` 打 `/healthz`。

### 2.2 非 root 运行 nginx

官方 `nginx:alpine` 以 root 启动并监听 80。改法：
- 配置里 `listen 8080;`、`pid /tmp/nginx.pid;`
- 临时目录 `client_body_temp_path /tmp/...` 等全部指到 `/tmp`（nginx 官方镜像默认 `/var/cache/nginx/*`，需 chown）
- 启动前 `chown -R nginx:nginx /var/cache/nginx /var/run /etc/nginx/conf.d`
- 以 `USER nginx` + `ENTRYPOINT` 脚本启动 nginx

> `nginx` 用户 uid=101、gid=101（alpine 版）。chown 需在构建期做（运行时 root 降权已无权限）。

### 2.3 上游地址可配

`nginx.conf.template` 中写 `proxy_pass ${API_UPSTREAM};`，entrypoint 用 `envsubst`（`gettext` 已在 alpine 基础镜像中）渲染成 `/etc/nginx/nginx.conf`。默认值写在 `ENV API_UPSTREAM=http://host.docker.internal:8080`。

注意：`envsubst` 会替换**所有** `$变量`，nginx 配置里的 `$uri` `$host` 等会被误替换。解决：只对 `envsubst '$API_UPSTREAM'` 做定向替换（传变量名列表），其余保持原样。

### 2.4 反代路径

前端请求 `/api/v1/auth/captcha`，后端路由前缀就是 `/api/v1`。因此：

```nginx
location /api/ {
    proxy_pass ${API_UPSTREAM};   # 不带尾斜杠 → 原样透传 /api/v1/...
}
```

`proxy_pass` **不带** URI 部分时，nginx 原样转发完整原始 URI，最省心、最不易错。

## 3. 风险与对策

| # | 风险 | 影响 | 对策 |
|---|---|---|---|
| 1 | 守护进程拉不到基础镜像 | 无法 `FROM` | 已在宿主侧拉取并 `docker load`（`nginx:alpine`、`oven/bun:1.3.14-alpine`）；Dockerfile 保持标准写法 |
| 2 | `envsubst` 吞掉 nginx 变量 | 配置语法错误、启动失败 | 用定向替换 `envsubst '$API_UPSTREAM'`；启动后用 `nginx -t` 校验 |
| 3 | 非 root 下 nginx 写不了 pid/临时目录 | 容器启动即退出 | pid 与全部 `*_temp_path` 指向 `/tmp`，构建期 chown 必要目录 |
| 4 | `bun install --frozen-lockfile` 与 `bun.lock` 版本不符 | 构建失败 | 基础镜像 bun 版本与本地一致（1.3.14）；lockfileVersion=1 受支持 |
| 5 | Vite 构建期内联 `VITE_*`，运行时改环境变量无效 | 切 mock/real 需要重新构建 | 在 README 明确；`ARG` 化并在文档给出两条构建命令 |
| 6 | 容器访问不到宿主 `127.0.0.1` | `/api` 502 | 默认上游 `host.docker.internal:8080`；后端需 `RTS_CENTER_ADDR=0.0.0.0:8080` |
| 7 | `tsc -b` 类型检查失败导致构建中断 | 构建失败 | 先在本地跑一次 `bun run build` 确认基线通过 |
| 8 | 端口 8080 被占用 | 容器起不来 | `docker run` 时用 `-p <宿主端口>:8080` 换端口 |

## 4. 验证方式

### 4.1 静态验证
- `docker build` 成功；`docker history` / `docker inspect` 确认镜像内无 `.env.local`、无口令。
- `docker run --rm rts-web-ui:latest nginx -t` 通过。

### 4.2 运行时验证
1. 启动后端（宿主）：`RTS_CENTER_ADDR=0.0.0.0:8080 RTS_AUTH_USERS=... RTS_JWT_SECRET=... cargo run -p rts-center`
2. 启动前端容器：`docker run -d --name rts-web-ui -p 8080:8080 rts-web-ui:latest`
3. 冒烟：
   - `curl -I http://127.0.0.1:8080/` → 200
   - `curl -I http://127.0.0.1:8080/login` → 200（SPA fallback）
   - `curl -I http://127.0.0.1:8080/assets/<hash>.js` → `immutable` + gzip
   - `curl http://127.0.0.1:8080/api/v1/auth/captcha` → 后端 JSON
   - `curl http://127.0.0.1:8080/healthz` → 200
4. 浏览器（Windows Chrome headless + CDP）：打开 `http://127.0.0.1:8080/`，确认登录页**无** mock 演示账号提示；完成「取码 → 输入验证码 → 登录」全流程，进入首页并截图。
5. 停掉后端再登录 → 出现网络错误提示，无白屏。

### 4.3 记录
结果写入 `sdlc/20261029_webui_docker/verify.md`。

## 5. 交付

- worktree 内提交 → 推分支 `feature/webui-docker` → 建 PR（走 GitHub REST API，本机无 `gh` CLI）→ 回填 PR 编号到 `verify.md`。
