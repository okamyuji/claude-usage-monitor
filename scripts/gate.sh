#!/usr/bin/env bash
# 変更したファイルに品質ゲートをかける。各タスクの完了条件を、同じ基準のコマンド1つで確かめるため。
# 使い方: [GATE_BASE=<コミット>] scripts/gate.sh src/models/domain/usage.rs [...]
# mutationは、GATE_BASE（既定はHEAD）からの差分で変わった行だけを検査する。
# 全ファイルを毎回検査すると1,000件を超えて約7時間かかり、結果が出る前にコミットが進むため。
set -euo pipefail

root="$(dirname "$(dirname "$(realpath "$0")")")"
manifest="$root/Cargo.toml"
if [ "$#" -eq 0 ]; then
  echo "usage: scripts/gate.sh <src/...rs> [...]" >&2
  exit 2
fi

# 存在しないファイルが渡されると、どの検査も対象なしのまま合格してしまうため、先に止める。
for f in "$@"; do
  if [ ! -f "$root/$f" ]; then
    echo "gate.sh: ファイルがありません: $f" >&2
    exit 2
  fi
done

# testcontainersをColimaで動かすための既定値。利用者が設定済みならそれを使う。
export DOCKER_HOST="${DOCKER_HOST:-unix://$HOME/.colima/default/docker.sock}"
export TESTCONTAINERS_DOCKER_SOCKET_OVERRIDE="${TESTCONTAINERS_DOCKER_SOCKET_OVERRIDE:-/var/run/docker.sock}"

cargo fmt --manifest-path "$manifest" --check
cargo clippy --manifest-path "$manifest" --all-targets -- -D warnings
cargo llvm-cov --manifest-path "$manifest" --no-report
cargo llvm-cov report --manifest-path "$manifest" --json --output-path "$root/target/llvm-cov.json"
python3 "$root/scripts/crap.py" "$root/target/llvm-cov.json" "$@"

mutant_files=()
for f in "$@"; do
  case "$f" in
    src/views/*|src/main.rs) ;;
    *) mutant_files+=("$f") ;;
  esac
done
# viewsは描画、main.rsは依存の組み立てだけなのでmutationの対象外にする（spec 12.3節）。
# 対象が残らないときは、cargo-mutantsが「対象なし」で返す終了コードに頼らず、ここで終える。
if [ "${#mutant_files[@]}" -eq 0 ]; then
  echo "mutation: 対象なし（viewsとmain.rsは対象外）"
  exit 0
fi
base="${GATE_BASE:-HEAD}"
diff_file="$root/target/gate.diff"
git -C "$root" diff "$base" -- "${mutant_files[@]}" > "$diff_file"
if [ ! -s "$diff_file" ]; then
  echo "mutation: $base からの変更行なし"
  exit 0
fi
# 終了コード3はタイムアウトだけが出た場合。ループの終了条件を壊す変異は無限ループになり、タイムアウトで検出されるので合格とする。
# 生存（コード2）とその他の失敗は不合格のまま返す。
status=0
cargo mutants -d "$root" --in-diff "$diff_file" --timeout 300 --jobs 2 || status=$?
if [ "$status" -ne 0 ] && [ "$status" -ne 3 ]; then
  exit "$status"
fi
