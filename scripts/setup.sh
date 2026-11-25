#!/usr/bin/env bash
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$root"
mkdir -p .tools/bin
source "$root/scripts/env.sh"

node_version=v24.22.3
mcp_version=v4.1.0
rust_version="$(awk -F '"' '/^channel[[:space:]]*=/ {print $2}' rust-toolchain.toml)"
case "$(uname -s)-$(uname -m)" in
    Linux-x86_64)
        node_arch=x64
        rust_target=x86_64-unknown-linux-gnu
        node_sha=061ec3e3062115705b56b51fa9cc330ab50d34b6969387d4e4b1a50c6809acc2
        mcp_sha=12c2426d5c0c041c326447fb313866c92cb246e96ab96cd86cbd4a98fca0eb48
        ;;
    Linux-aarch64)
        node_arch=arm64
        rust_target=aarch64-unknown-linux-gnu
        node_sha=261af2af250dfd7e9f25cf151d1b2713a0adf8f10a9ad116c4947f68627affe8
        mcp_sha=5826f74613e51f7c17a77bb47262bcf308bb4a8ace55abb4605fb7dcf9cfac43
        ;;
    *)
        printf 'This bootstrap supports glibc Linux x86_64 and aarch64. See README.md for tool pins.\n' >&2
        exit 1
        ;;
esac

download="$(mktemp -d "$root/.tools/download.XXXXXX")"
trap 'rm -rf -- "$download"' EXIT
fetch() { curl --fail --silent --show-error --location --retry 2 --connect-timeout 15 --max-time 180 "$1" -o "$2"; }
verify() {
    if ! printf '%s  %s\n' "$1" "$2" | sha256sum --check --status; then
        printf 'Checksum verification failed for %s\n' "$2" >&2
        exit 1
    fi
}

if [[ "$("$root/.tools/node/bin/node" --version 2>/dev/null || true)" != "$node_version" ]]; then
    printf 'Installing Node.js %s in .tools/node\n' "$node_version"
    archive="node-$node_version-linux-$node_arch.tar.gz"
    fetch "https://nodejs.org/dist/$node_version/$archive" "$download/$archive"
    verify "$node_sha" "$download/$archive"
    mkdir "$download/node"
    tar -xzf "$download/$archive" --strip-components=1 -C "$download/node"
    rm -rf -- "$root/.tools/node"
    mv -- "$download/node" "$root/.tools/node"
fi

if [[ "$("$root/.tools/bin/rust-anything" --version 2>/dev/null || true)" != "rust-anything ${mcp_version#v}" ]]; then
    printf 'Installing rust-anything %s in .tools/bin\n' "$mcp_version"
    archive="rust-anything-$mcp_version-$rust_target.tar.gz"
    fetch "https://github.com/erkamyaman/rust-anything/releases/download/$mcp_version/$archive" "$download/$archive"
    verify "$mcp_sha" "$download/$archive"
    member="$(tar -tzf "$download/$archive" | awk '/(^|\/)rust-anything$/ {print}')"
    tar -xOzf "$download/$archive" "$member" > "$download/rust-anything"
    install -m 755 "$download/rust-anything" "$root/.tools/bin/rust-anything"
fi

if [[ ! -x "$CARGO_HOME/bin/rustup" ]]; then
    printf 'Installing Rust %s in .tools/rustup\n' "$rust_version"
    fetch "https://static.rust-lang.org/rustup/dist/$rust_target/rustup-init" "$download/rustup-init"
    fetch "https://static.rust-lang.org/rustup/dist/$rust_target/rustup-init.sha256" "$download/rustup-init.sha256"
    verify "$(awk 'NR==1 {print $1}' "$download/rustup-init.sha256")" "$download/rustup-init"
    chmod +x "$download/rustup-init"
    "$download/rustup-init" -y --no-modify-path --default-toolchain none
fi
"$CARGO_HOME/bin/rustup" toolchain install "$rust_version" --profile minimal --component rustfmt --component clippy --no-self-update

"$root/scripts/setup-showdown.sh"
