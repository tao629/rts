# 构建方案（Plan）: 日志目录加入 gitignore

依据：`spec.md`。状态：Finalize |

## 1. 改动文件清单

| 序 | 文件 | 变更 | 说明 |
|----|----|----|----|
| 1 | `.gitignore` | 改 | 追加 `/logs/` |
| 2 | `sdlc/20261024_日志目录加入gitignore/{intent,spec,plan}.md` | 新增 | 记录本次变更 |

## 2. 实现要点

1. 在 `.gitignore` 末尾追加 `/logs/`，保持与 `/target`、`/dist/` 一致的根目录锚定风格。
2. 不使用无前导斜杠的 `logs/`：后者会忽略任意层级的同名目录，可能误伤子目录里本应入库的业务目录。

## 3. 风险与对策

| 风险 | 影响 | 对策 |
|----|----|----|
| 误伤子目录中的 `logs/` | 该目录不再入库 | 用 `/logs/` 根目录锚定 |
| 已有被跟踪的 `logs/` 文件 | 忽略规则对已跟踪文件无效 | 仓库中不存在被跟踪的 `logs/`（`git ls-files logs/` 为空） |

## 4. 验证方式

1. 仓库根执行 `cargo run -p rts-core --example log_demo` → 生成 `logs/app.log` 与 `logs/archive/`。
2. `git status --porcelain` 为空。
3. `cargo build --workspace` 通过。

## 5. 验证结果

- 仓库根执行示例程序后，`logs/` 正常生成，`git status --porcelain` 为空（忽略规则生效）。
- `cargo build --workspace` 通过（未改代码，回归确认）。
