#!/usr/bin/env bash
set -euo pipefail

case "$(dpkg --print-architecture)" in
    amd64)
        mirrors=(https://mirrors.edge.kernel.org/ubuntu https://mirrors.aliyun.com/ubuntu)
        ;;
    arm64)
        mirrors=(https://mirrors.tuna.tsinghua.edu.cn/ubuntu-ports https://mirrors.aliyun.com/ubuntu-ports)
        ;;
    *)
        echo "Unsupported Ubuntu CI architecture" >&2
        exit 1
        ;;
esac

sources=$(mktemp "${RUNNER_TEMP:-/tmp}/xrack-ubuntu.XXXXXX.sources")
trap 'rm -f "$sources"' EXIT
chmod 644 "$sources"
apt=(sudo apt-get
    -o "Dir::Etc::sourcelist=$sources"
    -o Dir::Etc::sourceparts=-
    -o Acquire::Retries=1
    -o Acquire::https::Timeout=15)

for mirror in "${mirrors[@]}"; do
    echo "Installing Ubuntu desktop build dependencies from $mirror"
    cat > "$sources" <<EOF
Types: deb
URIs: $mirror
Suites: noble noble-updates noble-security
Components: main
Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg
EOF
    if "${apt[@]}" -o APT::Update::Error-Mode=any update &&
        "${apt[@]}" install -y --no-install-recommends pkg-config libdbus-1-dev libwayland-dev libxkbcommon-dev; then
        exit 0
    fi
done

echo "Could not install desktop build dependencies from either Ubuntu mirror" >&2
exit 1
