# 意图（Intent）: 日志目录加入 gitignore

Author: zhtao. 状态：Finalize |

## 问题（Problem）

`rts-core` 的日志模块（PR #4 合并）默认把日志写到**当前工作目录下的 `logs/`**（活动文件 `logs/app.log`，归档在 `logs/archive/`）。

只要从仓库根目录跑一次用到该日志模块的程序（例如 `cargo run -p rts-core --example log_demo`），仓库根就会多出一个未跟踪的 `logs/` 目录，`git status` 立刻变脏：

```text
?? logs/
```

这与仓库既有的 `/target`、`/dist/` 处理方式不一致——那两个构建产物目录都已加入 `.gitignore`。

## 预期结果（Proposed outcome）

1. `.gitignore` 增加 `/logs/`，与 `/target`、`/dist/` 保持同样的根目录锚定风格。
2. 在仓库根运行产生 `logs/` 的程序后，`git status` 保持干净。
3. 不改动任何代码逻辑——本次只改 `.gitignore` 一个文件。

**验证方式：** 在仓库根执行 `cargo run -p rts-core --example log_demo`，随后 `git status --porcelain` 为空。

## 受影响的模块（Affected modules）

| 模块 | 路径 | 变更性质 |
|----|----|----|
| 忽略规则 | `.gitignore` | 改：新增 `/logs/` |

不涉及代码、数据库、其它模块。

## 约束条件（Constraints）

- 遵循仓库规约：独立 worktree（`.claude/worktrees/chore-gitignore-logs/`）+ 分支 + PR；主仓只读。
- 只改 `.gitignore` 一个文件，不夹带其它改动。
- 用 `/logs/`（根目录锚定）而非 `logs/`，避免误伤子目录里可能存在的同名业务目录。

## 已解决的问题（Resolved questions）

（无）
