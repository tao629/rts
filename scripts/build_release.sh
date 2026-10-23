#!/usr/bin/env bash
#
# f_matcher 发布打包脚本。
#
# 一条命令完成：编译 → 组装产物目录 → 打包 tar.gz → 生成 sha256 校验文件。
#
#   scripts/build_release.sh                # 本机架构对应的 musl 平台
#   scripts/build_release.sh --all          # 全部支持的平台
#   scripts/build_release.sh --target aarch64-unknown-linux-musl
#
# 产物布局（默认 <仓库根>/dist）：
#
#   dist/
#   ├── f_matcher-<version>-linux-<arch>/
#   │   ├── f_matcher            # musl 静态二进制
#   │   ├── sensitive_rule.txt   # 规则文件，与二进制同级
#   │   └── README.md
#   ├── f_matcher-<version>-linux-<arch>.tar.gz
#   └── f_matcher-<version>-linux-<arch>.tar.gz.sha256
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CRATE_DIR="$REPO_ROOT/toolkit/f_matcher"
CRATE_NAME="f_matcher"

# 支持的目标平台（均为 musl 静态链接，目标机无需运行库）。
SUPPORTED_TARGETS=(x86_64-unknown-linux-musl aarch64-unknown-linux-musl)

usage() {
    cat <<EOF
用法: scripts/build_release.sh [选项]

编译 ${CRATE_NAME} 并打包成可直接分发到目标机的发布包（musl 静态链接）。

选项:
  --target <triple>   指定目标平台，默认按本机架构选择对应的 musl triple
  --all               为全部支持的平台各出一份产物
  --output-dir <dir>  产物输出目录，默认 <仓库根>/dist
  -h, --help          打印本用法后退出

支持的平台:
  x86_64-unknown-linux-musl    linux-x86_64
  aarch64-unknown-linux-musl   linux-aarch64

产物:
  <output-dir>/${CRATE_NAME}-<version>-linux-<arch>/          产物目录
  <output-dir>/${CRATE_NAME}-<version>-linux-<arch>.tar.gz     压缩包
  <output-dir>/${CRATE_NAME}-<version>-linux-<arch>.tar.gz.sha256  校验和
EOF
}

die() {
    echo "错误: $*" >&2
    exit 1
}

# --- 参数解析 ---------------------------------------------------------------

target=""
build_all=0
output_dir=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --target)
            [[ $# -ge 2 ]] || die "--target 需要一个目标平台"
            target="$2"
            shift 2
            ;;
        --target=*)
            target="${1#*=}"
            shift
            ;;
        --all)
            build_all=1
            shift
            ;;
        --output-dir)
            [[ $# -ge 2 ]] || die "--output-dir 需要一个目录路径"
            output_dir="$2"
            shift 2
            ;;
        --output-dir=*)
            output_dir="${1#*=}"
            shift
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            echo "错误: 未知选项: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
done

# --- 基本信息 ---------------------------------------------------------------

version="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$CRATE_DIR/Cargo.toml" | head -n 1)"
[[ -n "$version" ]] || die "无法从 $CRATE_DIR/Cargo.toml 解析版本号"

[[ -n "$output_dir" ]] || output_dir="$REPO_ROOT/dist"
mkdir -p "$output_dir"
output_dir="$(cd "$output_dir" && pwd)"

# 清理上次运行遗留的临时组装目录。
rm -rf "$output_dir"/.staging-*

command -v cargo >/dev/null 2>&1 || die "未找到 cargo，请先安装 Rust 工具链"

# 校验和工具：优先 sha256sum，退化到 shasum。
if command -v sha256sum >/dev/null 2>&1; then
    sha256_of() { sha256sum "$1" | awk '{print $1}'; }
elif command -v shasum >/dev/null 2>&1; then
    sha256_of() { shasum -a 256 "$1" | awk '{print $1}'; }
else
    die "未找到 sha256sum 或 shasum，无法生成校验和"
fi

# --- 目标平台选择 -----------------------------------------------------------

is_supported() {
    local candidate="$1"
    local supported
    for supported in "${SUPPORTED_TARGETS[@]}"; do
        [[ "$supported" == "$candidate" ]] && return 0
    done
    return 1
}

# 本机架构归一化（aarch64 / arm64 视为同一架构）。
host_arch="$(uname -m)"
case "$host_arch" in
    aarch64 | arm64) host_arch="aarch64" ;;
esac

if [[ -n "$target" ]]; then
    is_supported "$target" || die "不支持的目标平台: $target（支持: ${SUPPORTED_TARGETS[*]}）"
    targets=("$target")
elif [[ $build_all -eq 1 ]]; then
    targets=("${SUPPORTED_TARGETS[@]}")
else
    case "$host_arch" in
        x86_64) targets=(x86_64-unknown-linux-musl) ;;
        aarch64) targets=(aarch64-unknown-linux-musl) ;;
        *) die "本机架构 $host_arch 不在支持列表，请用 --target 指定目标平台" ;;
    esac
fi

# --- 单平台打包 -------------------------------------------------------------

build_one() {
    local triple="$1"
    local arch="${triple%%-*}"
    local pkg="${CRATE_NAME}-${version}-linux-${arch}"
    local pkg_dir="$output_dir/$pkg"
    local tarball="$output_dir/$pkg.tar.gz"
    local checksum="$tarball.sha256"
    local bin_src="$REPO_ROOT/target/$triple/release/${CRATE_NAME}"

    echo "==> 构建 $triple"

    # 目标平台的 rust-std 必须已安装（不自动安装，避免隐式改动工具链状态）。
    if command -v rustup >/dev/null 2>&1; then
        if ! rustup target list --installed 2>/dev/null | grep -qx "$triple"; then
            die "目标平台 $triple 的 rust-std 未安装，请先执行: rustup target add $triple"
        fi
    fi

    # 交叉编译时用 rust-lld + 自包含链接，不依赖交叉工具链。
    if [[ "${triple%%-*}" != "$host_arch" ]]; then
        local env_suffix
        env_suffix="$(echo "$triple" | tr '[:lower:]-' '[:upper:]_')"
        export "CARGO_TARGET_${env_suffix}_LINKER=rust-lld"
        export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-C link-self-contained=yes"
        echo "    交叉编译：使用 rust-lld 自包含链接"
    fi

    cargo build \
        --release \
        -p "$CRATE_NAME" \
        --manifest-path "$REPO_ROOT/Cargo.toml" \
        --target "$triple"

    [[ -f "$bin_src" ]] || die "未找到编译产物: $bin_src"

    # 清理同名旧产物，保证产物是干净重建的。
    rm -rf "$pkg_dir" "$tarball" "$checksum"

    # 先在临时目录组装，全部成功后再整体移入正式位置：
    # 中途失败只会残留 .staging-* 临时目录（下次运行时清理），不会留下半成品产物。
    local staging="$output_dir/.staging-$pkg"
    rm -rf "$staging"
    mkdir -p "$staging"

    cp "$bin_src" "$staging/$CRATE_NAME"
    cp "$CRATE_DIR/sensitive_rule.txt" "$staging/sensitive_rule.txt"
    cp "$CRATE_DIR/README.md" "$staging/README.md"
    chmod 0755 "$staging/$CRATE_NAME"

    mv "$staging" "$pkg_dir"

    tar -czf "$tarball" -C "$output_dir" "$pkg"
    (cd "$output_dir" && sha256sum "$pkg.tar.gz" > "$pkg.tar.gz.sha256")

    local digest
    digest="$(sha256_of "$tarball")"

    echo "==> 产物"
    echo "    目录:   $pkg_dir"
    echo "    压缩包: $tarball ($(du -h "$tarball" | cut -f1))"
    echo "    校验和: $digest"
    echo
}

for triple in "${targets[@]}"; do
    build_one "$triple"
done

echo "打包完成: ${targets[*]}"
