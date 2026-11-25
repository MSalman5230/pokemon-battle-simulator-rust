#!/usr/bin/env bash
# Source this file to put the project's tools on PATH for the current shell.
sim_tools_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.tools" && pwd -P)" || return 1
export CARGO_HOME="$sim_tools_dir/cargo"
export RUSTUP_HOME="$sim_tools_dir/rustup"
export npm_config_cache="$sim_tools_dir/npm-cache"
export PATH="$sim_tools_dir/bin:$sim_tools_dir/node/bin:$CARGO_HOME/bin:$PATH"
unset sim_tools_dir
