#!/usr/bin/env bash
# 変更したファイルに品質ゲートをかける。各タスクの完了条件を、同じ基準のコマンド1つで確かめるため。
# 使い方: [GATE_BASE=<コミット>] [GATE_LOW=1] scripts/gate.sh src/models/domain/usage.rs [...]
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

# GATE_LOW=1 は、並列のビルドとmutationでmacOSのWindowServerが固まったため、時間より負荷を優先する。
# ビルドとテストを1本ずつ、バックグラウンドの優先度（CPUとディスクI/O）で動かす。
# mutationは作業ツリーをその場で書き換えるので、終わるまでソースを編集しない。
run=()
cov_args=()
mut_args=(--timeout 300 --jobs 2)
if [ -n "${GATE_LOW:-}" ]; then
  export CARGO_BUILD_JOBS=1
  if command -v taskpolicy >/dev/null; then
    run=(taskpolicy -b)
  fi
  # readme_imagesはwgpuでGPUを使い、負荷が大きいので外す。
  # 結合テストでしか検出できない変異もあるので、mutationもカバレッジと同じテストで検査する。
  targets=(--lib)
  for t in "$root"/tests/*.rs; do
    name="$(basename "$t" .rs)"
    [ "$name" = readme_images ] || targets+=(--test "$name")
  done
  cov_args=("${targets[@]}" -- --test-threads=1)
  # cargo-mutantsは既定でCPU数の枠のjobserverを子のcargoに渡し、CARGO_BUILD_JOBSより優先されるため、jobserverを使わない。
  mut_args=(--in-place --jobserver false --timeout 300 -- "${targets[@]}" -- --test-threads=1)
fi

"${run[@]}" cargo fmt --manifest-path "$manifest" --check
"${run[@]}" cargo clippy --manifest-path "$manifest" --all-targets -- -D warnings
# 前回の実行のprofrawや古いテストバイナリが残ると、集計が失敗したり、古い関数が0回として数えられたりする。
# このクレートのビルド物ごと消す（依存クレートのビルドは残る）。
"${run[@]}" cargo llvm-cov clean --manifest-path "$manifest" --workspace
"${run[@]}" cargo llvm-cov --manifest-path "$manifest" --no-report "${cov_args[@]}"
"${run[@]}" cargo llvm-cov report --manifest-path "$manifest" --json --output-path "$root/target/llvm-cov.json"
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
# `--output=...`のような値をgit diffのオプションとして読ませないため、コミットとして解釈できる値だけを受け付ける。
if ! git -C "$root" rev-parse --verify --quiet "${base}^{commit}" >/dev/null; then
  echo "gate.sh: GATE_BASEがコミットではありません: $base" >&2
  exit 2
fi
diff_file="$root/target/gate.diff"
git -C "$root" diff "$base" -- "${mutant_files[@]}" > "$diff_file"
# 未追跡の新しいファイルはgit diffに出ず、検査されないまま合格になるため、全行を追加として足す。
for f in "${mutant_files[@]}"; do
  if ! git -C "$root" ls-files --error-unmatch -- "$f" >/dev/null 2>&1; then
    git -C "$root" diff --no-index -- /dev/null "$f" >> "$diff_file" || true
  fi
done
if [ ! -s "$diff_file" ]; then
  echo "mutation: $base からの変更行なし"
  exit 0
fi
# 終了コード3はタイムアウトだけが出た場合。ループの終了条件を壊す変異は無限ループになり、タイムアウトで検出されるので合格とする。
# 生存（コード2）とその他の失敗は不合格のまま返す。
status=0
# 低負荷のmutationは--in-placeで、依存を含めた4GBほどの複製と全体の再ビルドを避ける。
"${run[@]}" cargo mutants -d "$root" --in-diff "$diff_file" "${mut_args[@]}" || status=$?
if [ "$status" -ne 0 ] && [ "$status" -ne 3 ]; then
  exit "$status"
fi
