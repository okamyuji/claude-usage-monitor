#!/usr/bin/env python3
"""対象ファイルの行カバレッジと、関数ごとのCRAP値を検査する。

Rustには定番のCRAP計測ツールがないため、cargo-llvm-covの関数別リージョンカバレッジと
lizardの循環的複雑度を、ファイルと開始行で突き合わせて算出する。
CRAP = comp^2 * (1 - cov)^3 + comp

使い方: crap.py <llvm-cov.json> [対象ファイル ...]
対象ファイルを省略すると src 配下の全関数を検査する。
"""
import csv
import io
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CRAP_LIMIT = 15.0
LINE_COVERAGE_MIN = 80.0
CODE_REGION = 0


def load(cov_json):
    return json.loads(Path(cov_json).read_text())["data"][0]


def line_coverage(data):
    return {str(Path(f["filename"]).resolve()): f["summary"]["lines"]["percent"] for f in data["files"]}


def region_coverage_by_function(data):
    # 同じ関数がコンパイル単位（libのテスト、cpsバイナリ、各結合テスト）ごとに別の記録として出る。
    # 足し合わせると実行されないコピーが薄めるため、同じ位置の領域は1つにまとめ、どれかで実行されたら通過とする。
    regions = {}
    for fn in data["functions"]:
        key = str(Path(fn["filenames"][0]).resolve())
        # 要素は [開始行, 開始列, 終了行, 終了列, 実行回数, ファイルID, 展開ファイルID, 種類]
        for r in fn["regions"]:
            if r[5] == 0 and r[7] == CODE_REGION:
                span = (key, r[0], r[1], r[2], r[3])
                regions[span] = regions.get(span, False) or r[4] > 0
    out = {}
    for (key, start, *_), covered in regions.items():
        out.setdefault(key, []).append((start, int(covered), 1))
    return out


def complexity(src):
    res = subprocess.run([sys.executable, "-m", "lizard", "-l", "rust", "--csv", str(src)], capture_output=True, text=True, check=True)
    for row in csv.reader(io.StringIO(res.stdout)):
        # NLOC, CCN, token, PARAM, length, location, file, function, long_name, start, end
        if len(row) < 11 or not row[1].isdigit():
            continue
        yield str(Path(row[6]).resolve()), row[7], int(row[1]), int(row[9]), int(row[10])


def check_line_coverage(data, targets):
    """対象ファイルごとに行カバレッジを検査し、不合格の件数を返す。"""
    lines = line_coverage(data)
    failures = 0
    for t in sorted(targets):
        # llvm-covは実行する行のないファイル（mod宣言だけのmod.rsなど）を出力に含めない。
        if t not in lines:
            print(f"--  行カバレッジ 対象外（実行する行なし） {t}")
            continue
        pct = lines[t]
        ok = pct >= LINE_COVERAGE_MIN
        print(f"{'OK ' if ok else 'NG '} 行カバレッジ {pct:.1f}% {t}")
        failures += 0 if ok else 1
    return failures


def crap_of(cov, file, start, end, ccn):
    hits = [c for c in cov.get(file, []) if start <= c[0] <= end]
    ratio = (sum(c[1] for c in hits) / sum(c[2] for c in hits)) if hits else 0.0
    return ccn**2 * (1 - ratio) ** 3 + ccn, ratio


def check_crap(data, targets):
    """対象ファイル（空なら全ファイル）の関数ごとにCRAP値を検査し、不合格の件数を返す。"""
    cov = region_coverage_by_function(data)
    failures = 0
    for file, name, ccn, start, end in complexity(ROOT / "src"):
        if targets and file not in targets:
            continue
        crap, ratio = crap_of(cov, file, start, end, ccn)
        if crap >= CRAP_LIMIT:
            print(f"NG  CRAP {crap:.1f} ccn={ccn} cov={ratio:.0%} {file}:{start} {name}")
            failures += 1
    return failures


def main():
    data = load(sys.argv[1] if len(sys.argv) > 1 else ROOT / "target/llvm-cov.json")
    targets = {str((ROOT / p).resolve()) for p in sys.argv[2:]}
    failures = check_line_coverage(data, targets) + check_crap(data, targets)
    print(f"不合格: {failures}件")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
