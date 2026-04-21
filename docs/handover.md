# cargo-compete ランダムテスト機能 引き継ぎ資料

作成日: 2026-04-19  
ブランチ: `feat/random-test`  
作業ディレクトリ: `/workspaces/atcoder-rust-devcontainer/cargo-compete`

---

## 1. プロジェクト概要

AtCoder 向け CLI ツール `cargo-compete` に「ランダムテスト」および「クロスチェック」機能を追加中。

### コマンド例

```bash
# ランダムテスト: サンプルをスキップしてランダム入力で 5 件テスト
cargo compete test a --random 5 --no-test

# ランダムテスト: サンプルテスト通過後にランダムテストも実行
cargo compete test a --random 10

# クロスチェック: a.rs と "a copy.rs" の出力を比較
cargo compete test a --cross "a copy.rs" --random 50
```

---

## 2. 変更ファイル一覧

### 新規作成

| ファイル | 役割 |
|---|---|
| `src/random_test/mod.rs` | エントリポイント。`run_random_tests()` / `run_cross_check()` を公開 |
| `src/random_test/parse.rs` | task.html の制約・入力形式をパース。`ConstraintParsed` / `InputBlock` を定義 |
| `src/random_test/generate.rs` | `ConstraintParsed` + `InputBlock` からランダム入力文字列を生成 |

### 変更済み

| ファイル | 変更内容 |
|---|---|
| `src/commands/test.rs` | `--random N` / `--cross PATH` / `--no-test` オプション追加 |
| `src/commands/submit.rs` | `--random N` オプション追加 |
| `src/testing.rs` | `Args` 拡張、サンプルテスト後のランダムテスト/クロスチェック呼び出し追加 |
| `src/lib.rs` | `mod random_test;` 追加 |
| `src/web/input_template.rs` | 複数関数を `pub(crate)` に変更（再利用のため） |
| `Cargo.toml` | `rand = "0.8"` 追加 |

---

## 3. 主要型・関数

### `parse.rs`

```rust
pub(crate) struct ConstraintParsed {
    pub bounds: HashMap<String, VarBound>,  // 変数名(小文字) → 上下限
    pub var_to_var: Vec<(String, String)>,  // lo <= hi 順序制約
    pub var_not_eq: Vec<(String, String)>,  // a != b 制約
    pub string_vars: HashMap<String, StringVarSpec>,  // 文字列変数
    pub skipped: Vec<String>,               // パース不能だった制約
}

pub(crate) enum BoundVal {
    Lit(i64),
    Var(String),           // 別変数を上限とする（例: M <= N）
    VarOffset(String, i64),
    Set(Vec<i64>),         // 離散値集合（「いずれか」制約）
}

pub(crate) fn parse_constraints(items: &[String]) -> ConstraintParsed
pub(crate) fn parse_input_blocks(lines: &[String]) -> Vec<InputBlock>
```

`InputBlock` 種別:
- `Scalars(Vec<String>)` — 1 行に複数スカラー（例: `N M`）
- `Array1D { base, len }` — 1 行配列（例: `A_1 ... A_N`）
- `NRepeat { cols, count }` — N 行繰り返し（例: `U_i V_i` × M 行）
- `Vertical { base, count }` — 縦配列（例: `w_1 / w_2 / ... / w_Q`）

### `generate.rs`

```rust
pub(crate) enum CaseStrategy {
    Random,
    AllMax, AllMin,
    SmallSize(i64),              // size 変数を小さく（1/2/3）
    ArrayMonoInc, ArrayMonoDec,
    ArrayAllMax, ArrayAllMin, ArrayAllSame,
    ArrayAltMaxMin, ArrayMountain, ArrayOneMaxRestMin,
}

pub(crate) fn make_strategy_list(blocks: &[InputBlock], count: u32) -> Vec<CaseStrategy>
pub(crate) fn generate_random_input(
    blocks: &[InputBlock],
    parsed: &ConstraintParsed,
    rng: &mut impl Rng,
    strategy: &CaseStrategy,
) -> String
```

`make_strategy_list`: 先頭に固定コーナーケース（AllMax, AllMin, SmallSize×3, 配列系×8）を入れ、残りを Random で埋める。

`generate_random_input`: 制約に違反するまでリトライするループ（`var_to_var` / `var_not_eq` チェック）。

### `mod.rs`

```rust
pub(crate) fn run_random_tests(args: RandomTestArgs<'_>) -> anyhow::Result<()>
pub(crate) fn run_cross_check(args: CrossCheckArgs<'_>) -> anyhow::Result<()>
pub(crate) fn ensure_cross_bin_registered(...) -> anyhow::Result<String>
```

---

## 4. 既知のバグと修正履歴（このセッションで解決済み）

### バグ1: 文字列制約が認識されない（今セッションで修正）

**症状**: `S は英小文字からなる長さ N の文字列` のような制約があっても、S に数値が生成される。

**原因**: `parse_string_constraints()` と `try_parse_enum_constraint()` で `" は "` (は の前後に半角スペース) で検索していたが、`"S は英小文字"` は「は」の直後に日本語が続くためスペースがなく、マッチしなかった。

**修正**:
```rust
// before
let Some(ha_pos) = item.find(" は ") else { continue };
let rest = &item[ha_pos + " は ".len()..];

// after
let Some(ha_pos) = item.find(" は") else { continue };
let rest = item[ha_pos + " は".len()..].trim_start_matches(' ');
```

対象箇所: `parse.rs` の `parse_string_constraints()` と `try_parse_enum_constraint()`

### バグ2: OOM（以前のセッションで修正済み）

**症状**: N=10^9 の配列をメモリ確保しようとして kill される。

**修正**: `MAX_ARRAY_SIZE = 200_000` を定義し、`resolve_size()` と size変数の scalar 生成両方でキャップ。

### バグ3: 無限リトライループ（以前のセッションで修正済み）

**症状**: `1<=l<=r<=N` の連鎖制約で AllMax が無限ループ。

**原因**: `bound_lit_hi("r")` が r の hi = Var("n") を再帰解決しなかった。

**修正**: `bound_lit_hi_depth(depth: u8)` で再帰的に解決（depth > 8 で打ち切り）。

---

## 5. テスト

```bash
# ユニットテスト（parse.rs 内）
cargo test --lib random_test::parse::tests

# テスト内容:
# - debug_abc454c_bounds / debug_abc454c_blocks
# - string_constraint_lower  (英小文字からなる)
# - string_constraint_explicit_charset  (A, B, C からなる)
# - enum_constraint  (1,2 のいずれか)
# - sum_constraint_t_limit  (総和制約)
```

現在 6 テスト全通過。

---

## 6. 動作確認結果

`docs/random-test-results.md` に abc440〜abc454 × a〜g の実行結果あり。  
`docs/abc440-random-test.md` 〜 `docs/abc454-random-test.md` に問題ごとの詳細分析あり。

主な傾向:
- A/B 問題はほぼ全 AC
- C〜G は RE が多い（解法未実装で入力読み取り後に panic）
- 入力生成自体は正しい（AC の場合は入力形式が一致している）

---

## 7. 残課題

### 優先度高

- [ ] **`--cross` フローの動作確認**: `--no-test` でのクロスチェック実行、Cargo.toml 自動登録の確認
- [ ] **submit コマンドでの `--random` 動作確認**

### 優先度低

- [ ] コーナーケースの追加（現在 5+8=13 種類）
  - 全要素が境界値±1 / 全要素が0 / 素数のみ など
- [ ] `--random N` の N とコーナーケース数の配分を明示（現在は先着順で固定ケースを埋める）

---

## 8. ビルド・インストール

```bash
# 開発ビルド
cargo build --manifest-path /workspaces/atcoder-rust-devcontainer/cargo-compete/Cargo.toml

# システムへのインストール（動作確認時）
cargo install --path /workspaces/atcoder-rust-devcontainer/cargo-compete --force

# テスト実行例（abc441-b）
cd /workspaces/atcoder-rust-devcontainer/src/contest/abc441
cargo compete test b --random 3 --no-test
```

---

## 9. コード上の注意点

- **変数名は全て小文字で管理**: `bounds` / `string_vars` のキーはすべて lowercase。`parse_input_blocks` も lowercase に正規化している。
- **size 変数のキャップ**: `MAX_ARRAY_SIZE = 200_000` で AllMax でもバイナリが OOM しないように制限。
- **リトライループ**: `generate_random_input` は `var_to_var` / `var_not_eq` を満たすまでループするため、矛盾する制約があると無限ループになりうる（実用上は問題ないはず）。
- **task.html の構造**: `<span class="h2">A - Title</span>` でセクション分割、`<h3>制約</h3>` 配下の `<ul><li>` を制約として取得。
- 最終的にフォーク元のリポジトリのmasterにプルリクする予定なので、無闇に既存コードを変更しない -
