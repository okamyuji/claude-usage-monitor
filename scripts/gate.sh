#!/usr/bin/env bash
# 変更したファイルに品質ゲートをかける。各タスクの完了条件を、同じ基準のコマンド1つで確かめるため。
# 使い方: [GATE_BASE=<コミット>] [GATE_LOW=1] [GATE_JOBS=<並行数>] [GATE_NO_MUTANTS=1] scripts/gate.sh src/models/domain/usage.rs [...]
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
# バックグラウンドの優先度（CPUとディスクI/O）で動かし、ビルドとテストの並行をGATE_JOBS（既定は1）に絞る。
run=()
cov_args=()
mut_args=(--timeout 300)
if [ -n "${GATE_LOW:-}" ]; then
  jobs="${GATE_JOBS:-1}"
  case "$jobs" in
    ''|*[!0-9]*|0) echo "gate.sh: GATE_JOBSは1以上の整数にしてください: $jobs" >&2; exit 2 ;;
  esac
  export CARGO_BUILD_JOBS="$jobs"
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
  cov_args=("${targets[@]}" -- --test-threads="$jobs")
  # cargo-mutantsは既定でCPU数の枠のjobserverを子のcargoに渡し、CARGO_BUILD_JOBSより優先されるため、jobserverを使わない。
  mut_args=(--jobserver false --timeout 300 -- "${targets[@]}" -- --test-threads="$jobs")
fi

"${run[@]}" cargo fmt --manifest-path "$manifest" --check
"${run[@]}" cargo clippy --manifest-path "$manifest" --all-targets -- -D warnings
# 前回の実行のprofrawや古いテストバイナリが残ると、集計が失敗したり、古い関数が0回として数えられたりする。
# このクレートのビルド物ごと消す（依存クレートのビルドは残る）。
"${run[@]}" cargo llvm-cov clean --manifest-path "$manifest" --workspace
"${run[@]}" cargo llvm-cov --manifest-path "$manifest" --no-report "${cov_args[@]}"
"${run[@]}" cargo llvm-cov report --manifest-path "$manifest" --json --output-path "$root/target/llvm-cov.json"
python3 "$root/scripts/crap.py" "$root/target/llvm-cov.json" "$@"
# mutationは数十分かかるので、途中のタスクでは飛ばし、最後にまとめて検査できるようにする。
if [ -n "${GATE_NO_MUTANTS:-}" ]; then
  echo "mutation: GATE_NO_MUTANTSのため省略"
  exit 0
fi

mutant_files=()
for f in "$@"; do
  case "$f" in
    src/views/layout.rs|src/views/probe.rs|src/views/theme.rs|src/views/*/math.rs|src/views/*/format.rs|src/views/*/state.rs) mutant_files+=("$f") ;;
    src/views/*|src/main.rs) ;;
    *) mutant_files+=("$f") ;;
  esac
done
# viewsの描画とmain.rsの依存の組み立てはmutationの対象外にする（spec 12.3節）。色や並びを変える変異はe2eで見分けられないため。
# viewsの下でも、計算だけのファイル（寸法、部品の記録、配色、グラフの計算と書式と操作の状態）は壊れると表示が崩れるので対象に残す。
# 対象が残らないときは、cargo-mutantsが「対象なし」で返す終了コードに頼らず、ここで終える。
if [ "${#mutant_files[@]}" -eq 0 ]; then
  echo "mutation: 対象なし（viewsの描画とmain.rsは対象外）"
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
# mutationは一時領域に複製したソースを書き換えて回し、作業ツリーには触れない。実行中も編集できるようにするため。
# 複製はgitが追跡するファイルと未追跡の新しいファイルだけにする。依存のビルドはtarget/mutantsに残し、
# 実行のたびに4GBほどの依存を作り直さない。複製は終わったとき（中断を含む）に消す。
work="$(mktemp -d "${TMPDIR:-/tmp}/cumon-mutants.XXXXXX")"
trap 'rm -rf "$work"' EXIT
trap 'exit 130' INT TERM
# 作業ツリーで消したがgitの索引に残るファイルは、tarが読めずに止まるので渡さない。
git -C "$root" ls-files -z --cached --others --exclude-standard \
  | while IFS= read -r -d '' f; do [ -e "$root/$f" ] && printf '%s\0' "$f"; done \
  | tar -C "$root" --null -T - -cf - \
  | tar -C "$work" -xf -
CARGO_TARGET_DIR="$root/target/mutants" "${run[@]}" cargo mutants -d "$work" --in-place \
  --output "$root" --in-diff "$diff_file" "${mut_args[@]}" || status=$?
if [ "$status" -ne 0 ] && [ "$status" -ne 3 ]; then
  exit "$status"
fi
