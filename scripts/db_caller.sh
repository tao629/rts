#!/usr/bin/env bash
#
# db_caller.sh —— 从环境文件读取 DATABASE_URL，连接数据库并执行传入的 SQL 文件。
#
# 用法：
#   scripts/db_caller.sh <sql文件> [更多 sql 文件 ...]
#
# 选项：
#   --env-file <path>  指定环境文件，默认 <仓库根>/.env
#   --url <url>        直接指定 DATABASE_URL（优先级最高）
#   --dry-run          只解析并打印将要执行的命令（口令脱敏），不连库
#   -y, --yes          跳过执行前的确认（供非交互 / CI 场景使用）
#   -h, --help         打印本用法
#
# 设计取舍：
#   * 只从环境文件里「文本解析」DATABASE_URL，不 source 整个文件，
#     避免环境文件中的内容被当作 shell 代码执行；
#   * 口令从连接串中剥离后放进 PGPASSWORD / MYSQL_PWD，
#     不出现在进程命令行里（ps 可见的部分不含凭据）；
#   * 任何输出都不打印口令，只打印脱敏后的目标库信息；
#   * 执行前默认要求人工确认，防止误跑破坏性 SQL。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

ENV_FILE="${REPO_ROOT}/.env"
DB_URL=""
DB_URL_SOURCE=""
DRY_RUN=0
ASSUME_YES=0
SQL_FILES=()

# --- 输出helpers ------------------------------------------------------------

die() {
    printf '错误: %s\n' "$*" >&2
    exit 1
}

warn() {
    printf '警告: %s\n' "$*" >&2
}

info() {
    printf '%s\n' "$*"
}

usage() {
    cat <<EOF
用法: ${0##*/} [选项] <sql文件> [更多 sql 文件 ...]

从环境文件读取 DATABASE_URL，连接数据库并按给定顺序执行 SQL 文件。

选项:
  --env-file <path>  指定环境文件，默认 ${REPO_ROOT}/.env
  --url <url>        直接指定 DATABASE_URL（优先级最高，覆盖环境变量与环境文件）
  --dry-run          只解析并打印将要执行的命令（口令脱敏），不连库
  -y, --yes          跳过执行前的确认
  -h, --help         打印本用法后退出

说明:
  * sql 文件写成 "-" 表示从标准输入读取 SQL；
  * 支持 postgresql/postgres、mysql/mariadb、sqlite 三类连接串；
  * DATABASE_URL 来源优先级：--url > 进程环境变量 > 环境文件；
  * 执行前默认要求确认并输入 y，非交互环境（无 TTY）自动放行但会打印提示。
EOF
}

# --- 参数解析 ---------------------------------------------------------------

while [[ $# -gt 0 ]]; do
    case "$1" in
        --env-file)
            [[ $# -ge 2 ]] || die "--env-file 缺少参数"
            ENV_FILE=$2
            shift 2
            ;;
        --env-file=*)
            ENV_FILE=${1#*=}
            shift
            ;;
        --url)
            [[ $# -ge 2 ]] || die "--url 缺少参数"
            DB_URL=$2
            shift 2
            ;;
        --url=*)
            DB_URL=${1#*=}
            shift
            ;;
        --dry-run)
            DRY_RUN=1
            shift
            ;;
        -y | --yes)
            ASSUME_YES=1
            shift
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        -)
            SQL_FILES+=("-")
            shift
            ;;
        --)
            shift
            SQL_FILES+=("$@")
            break
            ;;
        -*)
            die "未知选项: $1（用 --help 查看用法）"
            ;;
        *)
            SQL_FILES+=("$1")
            shift
            ;;
    esac
done

if [[ ${#SQL_FILES[@]} -eq 0 ]]; then
    usage >&2
    die "缺少 SQL 文件参数"
fi

for f in "${SQL_FILES[@]}"; do
    [[ $f == "-" ]] && continue
    [[ -f $f ]] || die "SQL 文件不存在: $f"
    [[ -r $f ]] || die "SQL 文件不可读: $f"
done

# --- 读取 DATABASE_URL ------------------------------------------------------

# 从环境文件里取值：只做文本解析，不 source 文件（避免执行文件中的任意命令）。
# 返回 0 表示取到了值，值写在标准输出。
read_env_value() {
    local file=$1 key=$2 line value
    [[ -f $file ]] || return 1
    line=$(grep -E "^[[:space:]]*(export[[:space:]]+)?${key}[[:space:]]*=" "$file" | tail -n 1) || return 1
    [[ -n $line ]] || return 1
    value=${line#*=}
    # 去掉首尾空白
    value=${value#"${value%%[![:space:]]*}"}
    value=${value%"${value##*[![:space:]]}"}
    # 去掉成对引号
    if [[ ${#value} -ge 2 && $value == \"*\" ]]; then
        value=${value:1:${#value}-2}
    elif [[ ${#value} -ge 2 && $value == \'*\' ]]; then
        value=${value:1:${#value}-2}
    fi
    [[ -n $value ]] || return 1
    printf '%s' "$value"
}

if [[ -n $DB_URL ]]; then
    DB_URL_SOURCE="--url 参数"
elif [[ -n ${DATABASE_URL:-} ]]; then
    DB_URL=$DATABASE_URL
    DB_URL_SOURCE="进程环境变量 DATABASE_URL"
else
    if [[ ! -f $ENV_FILE ]]; then
        die "找不到环境文件 ${ENV_FILE}，请用 --env-file 指定，或用 --url 直接给出连接串"
    fi
    if ! DB_URL=$(read_env_value "$ENV_FILE" DATABASE_URL); then
        die "环境文件 ${ENV_FILE} 中没有 DATABASE_URL"
    fi
    DB_URL_SOURCE="环境文件 ${ENV_FILE}"
fi

# --- 解析连接串 -------------------------------------------------------------

# %XX 解码。刻意不用 printf %b：它会把原文里的反斜杠也当作转义序列解释。
urldecode() {
    local s=$1 out="" hex
    while [[ -n $s ]]; do
        if [[ $s == \%?* ]]; then
            hex=${s:1:2}
            if [[ $hex =~ ^[0-9A-Fa-f]{2}$ ]]; then
                printf -v hex "\\x${hex}"
                out+=$hex
                s=${s:3}
                continue
            fi
        fi
        out+=${s:0:1}
        s=${s:1}
    done
    printf '%s' "$out"
}

SCHEME=""
DB_USER=""
DB_PASS=""
DB_HOST=""
DB_PORT=""
DB_NAME=""
DB_QUERY=""

parse_url() {
    local url=$1 rest remainder authority hostport path

    if [[ $url =~ ^([A-Za-z][A-Za-z0-9+.-]*)://(.*)$ ]]; then
        SCHEME=${BASH_REMATCH[1]}
        rest=${BASH_REMATCH[2]}
    else
        die "无法解析 DATABASE_URL：缺少 scheme（如 postgresql://...）"
    fi

    case "${SCHEME,,}" in
        sqlite | sqlite3)
            # sqlite://<路径>，路径原样保留（不做 %XX 解码，文件名可能是字面量）
            DB_NAME=$rest
            return
            ;;
        postgres | postgresql | mysql | mariadb) ;;
        *)
            die "暂不支持的数据库类型: ${SCHEME}（支持 postgresql/postgres、mysql/mariadb、sqlite）"
            ;;
    esac

    remainder=$rest
    if [[ $remainder == *\?* ]]; then
        DB_QUERY=${remainder#*\?}
        remainder=${remainder%%\?*}
    fi

    authority=""
    if [[ $remainder == *@* ]]; then
        # 按最后一个 '@' 切分：口令里出现未编码的 '@' 时也能正确解析
        authority=${remainder%@*}
        remainder=${remainder##*@}
    fi

    hostport=$remainder
    path=""
    if [[ $remainder == */* ]]; then
        hostport=${remainder%%/*}
        path=${remainder#*/}
    fi

    if [[ -n $authority ]]; then
        if [[ $authority == *:* ]]; then
            DB_USER=$(urldecode "${authority%%:*}")
            DB_PASS=$(urldecode "${authority#*:}")
        else
            DB_USER=$(urldecode "$authority")
        fi
    fi

    if [[ $hostport == \[*\]:* ]]; then
        DB_HOST="[${hostport%%]*}]"
        DB_PORT=${hostport##*:}
    elif [[ $hostport == *:* ]]; then
        DB_HOST=${hostport%%:*}
        DB_PORT=${hostport##*:}
    else
        DB_HOST=$hostport
    fi

    DB_NAME=$(urldecode "$path")
}

parse_url "$DB_URL"

case "${SCHEME,,}" in
    postgres | postgresql | mysql | mariadb)
        [[ -n $DB_NAME ]] || die "连接串里缺少数据库名: ${DB_URL%%\?*}"
        ;;
esac

# --- 目标库描述（脱敏） -----------------------------------------------------

describe_target() {
    if [[ ${SCHEME,,} == sqlite* ]]; then
        printf 'sqlite:%s' "$DB_NAME"
        return
    fi
    local desc="${SCHEME}://"
    [[ -n $DB_USER ]] && desc+="${DB_USER}@"
    desc+="${DB_HOST}"
    [[ -n $DB_PORT ]] && desc+=":${DB_PORT}"
    desc+="/${DB_NAME}"
    [[ -n $DB_QUERY ]] && desc+="?${DB_QUERY}"
    printf '%s' "$desc"
}

# --- 组装客户端命令 ---------------------------------------------------------

CMD=()
CMD_STDIN=0 # 1 表示 SQL 从标准输入喂给客户端

build_cmd() {
    local file=$1
    CMD=()
    CMD_STDIN=0
    case "${SCHEME,,}" in
        postgres | postgresql)
            command -v psql >/dev/null 2>&1 || die "未找到 psql 客户端，请先安装 postgresql-client"
            [[ -n $DB_PASS ]] && export PGPASSWORD="$DB_PASS"
            local uri="${SCHEME}://"
            if [[ -n $DB_HOST ]]; then
                [[ -n $DB_USER ]] && uri+="${DB_USER}@"
                uri+="${DB_HOST}"
                [[ -n $DB_PORT ]] && uri+=":${DB_PORT}"
            else
                # 未给主机：走本地 socket，用户名交给 PGUSER 传递
                [[ -n $DB_USER ]] && export PGUSER="$DB_USER"
            fi
            uri+="/${DB_NAME}"
            [[ -n $DB_QUERY ]] && uri+="?${DB_QUERY}"
            if [[ $file == "-" ]]; then
                CMD=(psql --no-psqlrc -v ON_ERROR_STOP=1 "$uri")
                CMD_STDIN=1
            else
                CMD=(psql --no-psqlrc -v ON_ERROR_STOP=1 -f "$file" "$uri")
            fi
            ;;
        mysql | mariadb)
            command -v mysql >/dev/null 2>&1 || die "未找到 mysql 客户端，请先安装 mysql-client"
            [[ -n $DB_PASS ]] && export MYSQL_PWD="$DB_PASS"
            CMD=(mysql --default-character-set=utf8mb4 --show-warnings)
            [[ -n $DB_HOST ]] && CMD+=(--host="$DB_HOST")
            [[ -n $DB_PORT ]] && CMD+=(--port="$DB_PORT")
            [[ -n $DB_USER ]] && CMD+=(--user="$DB_USER")
            CMD+=(--database="$DB_NAME")
            CMD_STDIN=1
            ;;
        sqlite | sqlite3)
            command -v sqlite3 >/dev/null 2>&1 || die "未找到 sqlite3 客户端，请先安装 sqlite3"
            CMD=(sqlite3 -bail "$DB_NAME")
            CMD_STDIN=1
            ;;
    esac
}

# --- 确认 -------------------------------------------------------------------

confirm() {
    local target=$1 answer=""
    if (( ASSUME_YES )); then
        return 0
    fi
    if [[ ! -t 0 || ! -t 1 ]]; then
        warn "非交互环境，跳过确认直接执行"
        return 0
    fi
    printf '目标库: %s\n' "$target"
    printf '来源:   %s\n' "$DB_URL_SOURCE"
    printf 'SQL 文件:\n'
    for f in "${SQL_FILES[@]}"; do
        printf '  - %s\n' "$f"
    done
    read -r -p "确认执行以上 SQL？[y/N] " answer </dev/tty
    [[ $answer == "y" || $answer == "Y" ]]
}

# --- 执行 -------------------------------------------------------------------

TARGET=$(describe_target)

if (( ! DRY_RUN )); then
    confirm "$TARGET" || die "已取消"
fi

for file in "${SQL_FILES[@]}"; do
    build_cmd "$file"
    if (( DRY_RUN )); then
        printf '[dry-run] %s' "${CMD[*]}"
        if (( CMD_STDIN )) && [[ $file != "-" ]]; then
            printf ' < %s' "$file"
        fi
        printf '\n'
        continue
    fi

    info "==> 执行 ${file} @ ${TARGET}"
    if (( CMD_STDIN )); then
        if [[ $file == "-" ]]; then
            "${CMD[@]}"
        else
            "${CMD[@]}" <"$file"
        fi
    else
        "${CMD[@]}"
    fi
done

if (( ! DRY_RUN )); then
    info "全部执行完成: ${TARGET}"
fi
