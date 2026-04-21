#!/usr/bin/env bash
# Fork slow-cat/cargo-compete into your GitHub account and push the current branch.
# Requires: gh (apt install gh), one-time: gh auth login
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

if ! command -v gh >/dev/null 2>&1; then
  echo "Install GitHub CLI: sudo apt-get update && sudo apt-get install -y gh"
  exit 1
fi

if ! gh auth status >/dev/null 2>&1; then
  PAT="${GH_TOKEN:-${GITHUB_TOKEN:-}}"
  if [[ -n "$PAT" ]]; then
    echo "$PAT" | gh auth login --hostname github.com --with-token
  fi
fi

if ! gh auth status >/dev/null 2>&1; then
  echo "GitHub にログインできていません。次のいずれかを実行してください:"
  echo "  gh auth login    # GitHub.com / HTTPS / ブラウザまたはトークン"
  echo "  export GH_TOKEN=ghp_.... && $0   # PAT を環境変数で渡す"
  exit 1
fi

BRANCH="$(git branch --show-current)"
if [[ -z "$BRANCH" ]]; then
  echo "No current branch."
  exit 1
fi

# gh が既存の origin を upstream にリネームし、あなたの fork を origin として追加する
echo "Creating fork (if needed) and updating remotes..."
gh repo fork slow-cat/cargo-compete --remote

echo "Pushing branch: $BRANCH"
git push -u origin "$BRANCH"

echo
echo "Done. Open your fork on GitHub and open a Pull Request against slow-cat/cargo-compete (or qryxip/cargo-compete)."
