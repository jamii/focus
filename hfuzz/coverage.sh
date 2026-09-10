#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"

corpus_dir="${1:-${CORPUS_DIR:-$repo_root/hfuzz/hfuzz_workspace/fuzz_hfuzz/input}}"
report_dir="${2:-${REPORT_DIR:-$repo_root/target/coverage/hfuzz}}"

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo is required" >&2
  exit 1
fi

if ! cargo llvm-cov --version >/dev/null 2>&1; then
  echo "error: cargo-llvm-cov is required; enter nix-shell or install cargo-llvm-cov" >&2
  exit 1
fi

LLVM_COV="${LLVM_COV:-$(command -v llvm-cov || true)}"
LLVM_PROFDATA="${LLVM_PROFDATA:-$(command -v llvm-profdata || true)}"
if [[ -z "$LLVM_COV" || -z "$LLVM_PROFDATA" ]]; then
  echo "error: llvm-cov and llvm-profdata are required; enter nix-shell or install LLVM tools" >&2
  exit 1
fi
export LLVM_COV LLVM_PROFDATA

if [[ ! -d "$corpus_dir" ]]; then
  echo "error: corpus directory does not exist: $corpus_dir" >&2
  exit 1
fi

mapfile -d '' inputs < <(find "$corpus_dir" -type f -print0 | sort -z)
if [[ "${#inputs[@]}" -eq 0 ]]; then
  echo "error: corpus directory contains no files: $corpus_dir" >&2
  exit 1
fi

cd "$repo_root"
mkdir -p "$report_dir"

llvm_cov_env="$(cargo llvm-cov show-env --export-prefix)"
eval "$llvm_cov_env"
cargo llvm-cov clean --workspace
cargo build --bin replay

echo "replaying ${#inputs[@]} corpus inputs from $corpus_dir"
count=0
for input in "${inputs[@]}"; do
  count=$((count + 1))
  if ! target/debug/replay "$input" >/dev/null 2>&1; then
    echo "error: replay failed for corpus input: $input" >&2
    target/debug/replay "$input" >&2
    exit 1
  fi
  if (( count % 1000 == 0 )); then
    echo "replayed $count/${#inputs[@]} inputs"
  fi
done

cargo llvm-cov report --html --output-dir "$report_dir"
cargo llvm-cov report --summary-only

echo "coverage report: $report_dir/html/index.html"
