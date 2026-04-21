# ランダムテスト・クロスチェック機能 要件定義

## 概要

`cargo compete test` / `cargo compete submit` コマンドに、ランダムテストおよびクロスチェック機能を追加する。

---

## コマンドインタフェース

### `cargo compete test`

```bash
# ランダムテスト（サンプル通過後にN件実行、省略時はデフォルト5件）
cargo compete test a --random
cargo compete test a --random 50
# tb補足　やっぱり変更します random→rand ❌ 未実装

# クロスチェック（サンプル通過後に別実装と比較、省略時はデフォルト100件）
cargo compete test a --cross "a_brute.rs"
cargo compete test a --cross "a_brute.rs" 50

# サンプルテストをスキップしてランダムテスト/クロスチェックのみ実行
cargo compete test a --random --no-test
# tb補足　やっぱり変更します random→rand ❌ 未実装
cargo compete test a --cross "a_brute.rs" --no-test
```

### `cargo compete submit`

```bash
# サブミット前にランダムテストを実行（失敗したらサブミットしない）
cargo compete submit a --rand 50
# tb補足 件数省略はtestと同様(実装済)

# サブミット前にクロスチェックを実行
cargo compete submit a --cross "a_brute.rs"
# tb補足 件数省略はtestと同様(実装済)
```

**制約:**
- `--cross` と `--no-test` は submit では同時使用不可（`cargo compete test --cross --no-test` を使うこと）

---

## 機能1: ランダムテスト（`--rand`）

### 実行フロー

1. サンプルテストを実行（`--no-test` なら省略）
2. サンプルが全て通過したらランダムテストを実行
3. 1件でも RE/TLE が出たら非ゼロ終了（サブミットに進まない）

### 制約情報の取得

- `task.html` をランダムテスト実行時に都度パース（キャッシュなし）
- AtCoder 専用（他プラットフォームはスキップ）

### 制約パース対応範囲

| 記法 | 対応状況 |
|------|---------|
| `1 \leq N \leq 10^5` | ✅ 実装済み |
| `3\times 10^5` などの式 | ✅ 実装済み |
| `1 \leq A,B \leq N`（複数変数） | ✅ 実装済み |
| `1 \leq M \leq N \leq 10`（チェーン） | ✅ 実装済み |
| `A_i \leq N`（変数依存上限） | ✅ 実装済み |
| `N-1`、`N+1` などのオフセット | ✅ 実装済み |
| `\dfrac`、`\frac`、`\sqrt` を含む式 | ⚠️ スキップ |
| `\min(N, 10^5)` などの関数 | ❌ 未実装 → ⚠️ スキップで問題なし(tb補足)|
| `T \leq \sum N_i`（sum constraint） | ❌ 未実装 → 実装する(tb補足)|

パースできなかった制約はスキップし、末尾に警告として表示する。
# tb補足 abcの最近の問題を横断的に見て行ってよくある制約についてはなるべく対応する ❌ 未実装

### コーナーケース生成戦略

`--rand N` のN件を以下の順番で生成し、余った枠をランダムで埋める。
# tb補足: 変更予定。完全ランダムに10件未満なら1件、10件以上なら2件まず割り当てる。残りはコーナーケースにランダムに割り当てて、コーナーケースが全種類出たら残りはコーナーケース30%、完全ランダム70%の割合とする ❌ 未実装

| 戦略 | 説明 | 実装状況 |
|------|------|---------|
| AllMax | 全変数を最大値 | ✅ |
| AllMin | 全変数を最小値 | ✅ |
| SmallSize(1) | サイズ変数を 1 にして他はランダム | ✅ |
| SmallSize(2) | サイズ変数を 2 にして他はランダム | ✅ |
| SmallSize(3) | サイズ変数を 3 にして他はランダム | ✅ |
| ArrayMonoInc | 配列要素を単調増加 | ✅ |
| ArrayMonoDec | 配列要素を単調減少 | ✅ |
| ArrayAllMax | 配列要素を全て最大値 | ✅ |
| ArrayAllMin | 配列要素を全て最小値 | ✅ |
| ArrayAllSame | 配列要素を全て同じランダム値 | ✅ |
| ArrayAltMaxMin | 配列要素を max, min, max, min... と交互 | ✅ |
| ArrayMountain | 配列要素を増加→減少（山型） | ✅ |
| ArrayOneMaxRestMin | 1要素のみ最大値、残りは最小値 | ✅ |
| Random | 完全ランダム | ✅ |

**未実装のコーナーケース（要検討）:**
- 文字列入力の生成（`'a'〜'z'` の文字列など）
- グリッドの全パターン（`'#'` のみ、`'.'` のみ、など）
- 境界値周辺（max-1、min+1）
- 負の数もある変数については0を特別視したケースを入れたい ❌ 未実装

### 出力フォーマット（ランダムテスト）

```
{case_idx}/{total} ({name}) {verdict} ({ms} ms)
stdin:
{input}
actual:
{output}

...（全ケース繰り返し）

max: {最大処理時間} ms
warning: skipped N unsupported constraint(s): {制約内容}  ← スキップがある場合のみ
note: Accepted means the program exited without runtime error or TLE (output is not verified)
error: {失敗件数}/{総件数} tests failed  ← 失敗がある場合のみ
```

**注記:**
- `{name}` は `corner1`, `corner2`, ..., `random1`, `random2`, ... の形式
- tb補足　ここはrandom1,random2のままで
- `{verdict}` は `Accepted` / `Runtime Error (exit status: N)` / `Time Limit Exceeded`
- 出力が display_limit を超えた場合: `{先頭N文字}...(truncated, M bytes total)`
- Accepted はクラッシュ・TLEなしを意味し、出力の正しさは検証しない
- 😡 今はテスト用に緩めてますが、最終的には1テストケースごとの入出力の表示の制限は厳しめにして見やすくしますか

**未実装:**
- 最長処理時間ケースの詳細出力（ケース名・入力）を最後に表示する機能 ← tb補足: スキップ⚠️  で

---

## 機能2: クロスチェック（`--cross`）

### 実行フロー

1. メインバイナリのサンプルテスト（`--no-test` なら省略）✅ 実装済み
2. クロスバイナリを `Cargo.toml` に自動登録（未登録の場合）
3. クロスバイナリをビルド
4. クロスバイナリのサンプルテスト（`--no-test` なら省略）✅ 実装済み
5. ランダム入力をクロスバイナリに流して期待出力を収集（RE/TLE のケースはスキップ）
6. 期待出力に対してメインバイナリを判定
7. 1件でも WA/RE/TLE が出たら非ゼロ終了

### Cargo.toml 自動登録

- `[[bin]]` エントリと `[package.metadata.cargo-compete.bin]` エントリを同時に追加
- bin name: `{contest}-{ファイル名stemのkebab変換}` 例: `abc440-a-brute`
- alias: ファイル名stemのkebab変換 例: `a-brute`
- 既に登録済みならスキップ

### 比較方法

- `a.yml`（テストスイート）の `match:` フィールドを使用（`Exact` / `Lines` / `Float` など）

### 出力フォーマット（クロスチェック）

- クロスバイナリのサンプルテスト結果は `snowchains_core` の `print_pretty` で表示
- ランダムケースの判定結果も `snowchains_core` の `print_pretty` で表示
- WA が出た場合: `expected: {クロスバイナリ名}` / `actual: {メインバイナリ名}` を追記
- tb補足: Acceptedについては「{case_idx}/{total} ({name}) {verdict} ({ms} ms)」しか出さない(stdin以降を出さない)ようにします ❌ 未実装

---

## 制約パースの詳細動作

- 制約文はLaTeX形式で記述されており、`normalize_constraint()` でASCII化してからパース
- 変数名は小文字に統一して管理
- スキップする制約:
  - `\dfrac`、`\frac`、`\sqrt` を含むもの
  - 日本語文字 (`は`) や `整数` を含むもの（日本語説明文として除外）
  - 不等号が見つからないもの

---

## 未解決事項（今後の議論）

1. **sum constraint**: `1 \leq T \leq \sum_{i=1}^{N} N_i` のような上限がない変数の扱い
2. **\min/\max 式**: `M \leq \min(N, 10^5)` のような関数を含む制約のパース
3. **文字列変数の生成**: `S` が英小文字からなる文字列などの入力生成
4. **コーナーケース件数配分**: 具体的な戦略の件数配分（現在は固定順）
5. **最長処理時間ケースの詳細出力**: 全ケース終了後に最も遅かったケースの入力を表示
6. ✅ 対応済み。`S_1, S_2, \vdots, S_N` 形式で `S_2` を見て即 `break` していたため `[Chars; 2]` と誤生成するバグが `input_template.rs` の `parse_grid_lines` にあった。`parse_vertical_scalars` と同様に `continue` で最後のマッチまで探索するよう修正済み（`parse.rs` 側は最初から正しく実装）。
