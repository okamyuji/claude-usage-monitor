#!/usr/bin/env bash
# 変更したファイルに品質ゲートをかける。各タスクの完了条件を、同じ基準のコマンド1つで確かめるため。
# 使い方: scripts/gate.sh src/models/domain/usage.rs [...]
set -euo pipefail

root="$(dirname "$(dirname "$(realpath "$0")")")"
manifest="$root/Cargo.toml"
if [ "$#" -eq 0 ]; then
  echo "usage: scripts/gate.sh <src/...rs> [...]" >&2
  exit 2
fi

# testcontainersをColimaで動かすための既定値。利用者が設定済みならそれを使う。
export DOCKER_HOST="${DOCKER_HOST:-unix://$HOME/.colima/default/docker.sock}"
export TESTCONTAINERS_DOCKER_SOCKET_OVERRIDE="${TESTCONTAINERS_DOCKER_SOCKET_OVERRIDE:-/var/run/docker.sock}"

cargo fmt --manifest-path "$manifest" --check
cargo clippy --manifest-path "$manifest" --all-targets -- -D warnings
cargo llvm-cov --manifest-path "$manifest" --no-report
cargo llvm-cov report --manifest-path "$manifest" --json --output-path "$root/target/llvm-cov.json"
python3 "$root/scripts/crap.py" "$root/target/llvm-cov.json" "$@"

mutant_args=()
for f in "$@"; do
  mutant_args+=(--file "$f")
done
cargo mutants -d "$root" "${mutant_args[@]}" --timeout 180 --jobs 4
