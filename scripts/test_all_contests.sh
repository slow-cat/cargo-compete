#!/bin/bash
# Run random test on abc440–abc454, all problems a–g
set -euo pipefail

CONTESTS_DIR=/workspaces/atcoder-rust-devcontainer/src/contest
RESULTS_FILE=/workspaces/atcoder-rust-devcontainer/cargo-compete/docs/random-test-results.md

echo "# ランダムテスト実行結果 (abc440–abc454)" > "$RESULTS_FILE"
echo "" >> "$RESULTS_FILE"
echo "実行日: $(date '+%Y-%m-%d')" >> "$RESULTS_FILE"
echo "" >> "$RESULTS_FILE"

for contest in abc440 abc441 abc442 abc443 abc444 abc445 abc446 abc447 abc448 abc449 abc450 abc451 abc452 abc453 abc454; do
    contest_dir="$CONTESTS_DIR/$contest"
    if [ ! -d "$contest_dir" ]; then
        continue
    fi

    echo "## $contest" >> "$RESULTS_FILE"
    echo "" >> "$RESULTS_FILE"

    # Build all bins
    (cd "$contest_dir" && cargo build --bins 2>&1) | tail -3 || true

    bin_dir="$contest_dir/target/debug"

    for problem in a b c d e f g; do
        bin_path="$bin_dir/${contest}-${problem}"
        if [ ! -f "$bin_path" ]; then
            continue
        fi

        echo "### $problem" >> "$RESULTS_FILE"
        echo "" >> "$RESULTS_FILE"
        echo '```' >> "$RESULTS_FILE"

        output=$(cargo compete test "$problem" --random 5 --no-test \
            2>&1 <<< "" || true)
        # Actually run via cargo-compete from contest dir
        output=$(cd "$contest_dir" && cargo compete test "$problem" --random 5 --no-test 2>&1 || true)
        echo "$output" >> "$RESULTS_FILE"

        echo '```' >> "$RESULTS_FILE"
        echo "" >> "$RESULTS_FILE"
    done
done

echo "Done. Results in $RESULTS_FILE"
