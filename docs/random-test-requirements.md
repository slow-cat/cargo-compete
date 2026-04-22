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

# クロスチェック（サンプル通過後に別実装と比較、省略時はデフォルト100件）
cargo compete test a --cross "a_brute.rs"
cargo compete test a --cross "a_brute.rs" 50

# サンプルテストをスキップしてランダムテスト/クロスチェックのみ実行
cargo compete test a --random --no-test
cargo compete test a --cross "a_brute.rs" --no-test
```

### `cargo compete submit`

```bash
# サブミット前にランダムテストを実行（失敗したらサブミットしない）
cargo compete submit a --random 50
# tb補足 件数省略はtestと同様(実装済)

# サブミット前にクロスチェックを実行
cargo compete submit a --cross "a_brute.rs"
# tb補足 件数省略はtestと同様(実装済)
```

**制約:**
- `--cross` と `--no-test` は submit では同時使用不可（`cargo compete test --cross --no-test` を使うこと）

---

## 機能1: ランダムテスト（`--random`）

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
| `T \leq \sum N_i`（sum constraint） | ✅ 実装済み（`apply_sum_constraints`）|

パースできなかった制約はスキップし、末尾に警告として表示する。
# tb補足 abcの最近の問題を横断的に見て行ってよくある制約についてはなるべく対応する（継続課題）

### コーナーケース生成戦略

件数配分: 完全ランダムを先頭に1件（count<10）または2件（count≥10）割り当て、残りはシャッフルしたコーナー全種類を順に割り当てる。全種類カバー後はコーナー30%・ランダム70%の混合。✅ 実装済み

**戦略は1テストケースにつき1つ選ばれ、入力全体に適用される。** ただし効果は変数・入力の種類によって異なる。

| 戦略 | スカラー整数 | 整数配列・行列 | 文字列（可変長） | 文字グリッド（固定幅） | 検出する問題パターン |
|------|-----------|------------|--------------|-------------------|-------------------|
| AllMax | 上限値 | 全要素=上限値 | 最大長・末尾文字のみ（例: `zzzzz`） | 全行=末尾文字のみ（例: `#####`） | 大入力でのTLE・オーバーフロー |
| AllMin | 下限値 | 全要素=下限値 | 最小長・先頭文字のみ（例: `a`） | 全行=先頭文字のみ（例: `.....`） | 0・1要素の境界処理 |
| SmallSize(k) | サイズ変数=min(k,上限)、lo違反はスキップ、他ランダム | サイズ=min(k,上限)の配列、要素ランダム | 長さ=k.clamp(lo,hi)・ランダム文字 | min(k,上限)行・ランダム文字 | 小さい配列での動作 |
| ZeroCorner | lo<0<hi の変数→0、他ランダム | ランダム | ランダム | ランダム | 符号変化・ゼロ除算・0境界 |
| SumMaxSingle | T=1・inner_var=sum上限・他サイズ変数=最大・非サイズはランダム | ランダム要素 | 最大長・ランダム文字 | 最大行数×1回・ランダム文字 | sum制約下での最大単一ケース |
| ArrayMonoInc | ランダム | 単調増加列（行列は行ごとに増加） | charset 内で増加（先頭→末尾文字）（複数文字列は行ごとに変化） | 行ごとに charset 内で増加（例: `aaa`→`mmm`→`zzz`） | ソート済み入力・二分探索の境界 |
| ArrayMonoDec | ランダム | 単調減少列（行列は行ごとに減少） | charset 内で減少（末尾→先頭文字） | 行ごとに charset 内で減少（例: `zzz`→`mmm`→`aaa`） | 逆順ソート済み入力 |
| ArrayAllSame | ランダム | 全要素=同一ランダム値・全行同一 | 全文字列=同一ランダム1文字を繰り返し | 全行=同一ランダム1文字を繰り返し | 全同値・重複処理 |
| ArrayAltMaxMin | ランダム | 上限・下限を交互 | charset の末尾文字・先頭文字を交互 | 行ごとに末尾文字・先頭文字を交互（例: `###`→`...`→`###`） | 交互パターン・奇偶インデックス処理 |
| ArrayMountain | ランダム | 増加→減少（山型） | charset 内で増加→減少（山型） | 行ごとに charset 内で増加→減少（山型） | 単峰性を仮定したアルゴリズム |
| ArrayOneMaxRestMin | ランダム | 中央1要素=上限、残り=下限 | 中央1文字列=末尾文字のみ、残り=先頭文字のみ | 中央1行=末尾文字のみ、残り=先頭文字のみ | 外れ値・孤立した最大値 |
| Random | ランダム | ランダム | ランダム長・ランダム文字 | ランダム文字 | 一般ケース |

**未実装のコーナーケース:**
- 境界値周辺（max-1、min+1）❌

### 出力フォーマット（ランダムテスト）

```

══════════════════════════════════════════
               random tests
══════════════════════════════════════════
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

-- snowchains の `judge()` はバイナリ自身を実行するため、`run_with_input` の結果を直接渡すことはできない。
-- `stdin:` / `actual:` ヘッダーを bold magenta に、verdict 行フォーマットを print_pretty に揃えることで視覚的に同等とした。✅ 実装済み
-- 数値の cyan ハイライトは snowchains のトークンパーサ再実装が必要なためスキップ。
-- 😡 出力フォーマットの一番上を変更しました。サンプルテストの後に1行空行を作り、random testsを宣言します。✅ 実装済み

**注記:**
- `{name}` は `corner1`, `corner2`, ..., `random1`, `random2`, ... の形式
- tb補足　ここはrandom1,random2のままで
- `{verdict}` は `Accepted` / `Runtime Error (exit status: N)` / `Time Limit Exceeded`
- 出力が display_limit を超えた場合: `{先頭N文字}...(truncated, M bytes total)`
- Accepted はクラッシュ・TLEなしを意味し、出力の正しさは検証しない
- stdin の表示上限は 200 bytes（`...(truncated, M bytes total)` 形式で省略）✅ 実装済み
- actual の表示上限は display_limit（デフォルト 4KiB）
- スキップした制約の警告は **テストループ前と後の2回** 出力する ✅ 実装済み
  - ループ前: プロセスが Killed されても必ず目に入るようにするため
  - ループ後: 全出力がスクロールした後でも末尾に残るようにするため

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
- Acceptedについては「{case_idx}/{total} ({name}) {verdict} ({ms} ms)」しか出さない(stdin以降を出さない)ようにします ✅ 実装済み

---

## 制約パースの詳細動作

- 制約文はLaTeX形式で記述されており、`normalize_constraint()` でASCII化してからパース
- 変数名は小文字に統一して管理
- スキップする制約:
  - `\dfrac`、`\frac`、`\sqrt` を含むもの
  - 日本語文字 (`は`) や `整数` を含むもの（日本語説明文として除外）
  - 不等号が見つからないもの

---

## 未解決事項

1. **sum constraint の残課題**: `1 \leq T \leq \sum_{i=1}^{N} N_i` のような上限がない変数の扱い
2. **\min/\max 式**: `M \leq \min(N, 10^5)` のような関数を含む制約のパース（スキップで問題なし）
3. **文字列変数の生成**: `S` が英小文字からなる文字列などの入力生成（コーナーケース表の未実装項目）
