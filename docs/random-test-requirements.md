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
cargo compete test a --cross src/bin/a_brute.rs
cargo compete test a --cross "a copy.rs" 50

# サンプルテストをスキップしてランダムテスト/クロスチェックのみ実行
cargo compete test a --random --no-test
cargo compete test a --cross "a_brute.rs" --no-test
```

### `cargo compete submit`

```bash
# サブミット前にランダムテストを実行（失敗したらサブミットしない）
cargo compete submit a --random 50

# サンプルチェックをスキップしてランダムテストのみ実行、全ACで提出(件数省略時は5件)
cargo compete submit a --random --no-test

# サブミット前にクロスチェックを実行(全て一致したら提出)
cargo compete submit a --cross "a_brute.rs"
# 件数省略はtestと同様
```

**制約:**
- `--cross` と `--no-test` は submit では同時使用不可（`cargo compete test --cross --no-test` を使うこと）

---

## 機能1: ランダムテスト（`--random`）

### 実行フロー

1. サンプルテストを実行（`--no-test` なら省略）
2. サンプルが全て通過したらランダムテストを実行
3. 1件でも RE/TLE が出たら非ゼロ終了（サブミットに進まない）

#### snowchains を使ったランダムテスト実行フロー詳細

1. 全テストケースの入力を生成（`generate_random_input`）
2. 各テストケースを `BatchTestCase { out: None, ... }` で構築
   - `out: None` → `DeterministicExpectedOutput::Pass` → 正常終了なら常に Accepted、RE/TLE のみ失敗
3. `judge()` に全ケースを**一括**投入（進捗バー表示あり）
4. `print_pretty` で全ケース表示（`expected:` 行は出ない、`stderr:` は空でない時のみ出る）

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
| `\dfrac`、`\frac`、`\sqrt` を含む式 | ✅ スキップする |
| `\min(N, 10^5)` などの関数 | ✅ スキップする|
| `T \leq \sum N_i`（sum constraint） | ✅ 実装済み |

パースできなかった制約はスキップし、末尾に警告として表示する。
> **方針:** abcでよくある制約についてなるべく対応する

### コーナーケース生成戦略

件数配分: Random戦略を1件（count<10）または2件（count≥10）割り当て、残りはシャッフルしたコーナーケース戦略全種類を**ランダム(重複なし)に**割り当てる。全種類カバー後はコーナー30%・ランダム70%の混合(適当)。ただし全種類カバー後の30%コーナーは**ランダム要素を持つ戦略のみ**から選択する（ランダム要素のない戦略は重複しても出力が同一になるため）。
※サイズが小さい入力は実用上確認しやすいので、敢えてSmallSize(k)のみ3件に対応するようにしている

**戦略は1テストケースにつき1つ選ばれ、入力全体に適用される。** ただし効果は変数・入力の種類によって異なる。

| 戦略 | スカラー整数 | 整数配列・行列 | 文字列（可変長） | 文字グリッド（固定幅） | デフォルト | 配列あり | 入力内テストケースあり | i64あり | ランダム要素 | 検出する問題パターン |
|------|-----------|------------|--------------|-------------------|---------|---------|-----------------|------------|------------|-------------------|
| AllMax | 上限値 | 全要素=上限値 | 最大長・末尾文字のみ（例: `zzzzz`） | 全行=末尾文字のみ（例: `#####`） | ⭕️ |   |   | | — | 大入力でのTLE・オーバーフロー |
| AllMin | 下限値 | 全要素=下限値 | 最小長・先頭文字のみ（例: `a`） | 全行=先頭文字のみ（例: `.....`） | ⭕️ |   |   | | — | 0・1要素の境界処理 |
| SmallSize(k) ※k=1,2,3の3件 | サイズ変数(テストケース数含む)=k.clamp(lo,hi)（loがVar未解決時はhi以下にクランプのみ）、他ランダム | サイズ=k.clamp(lo,hi)の配列、要素ランダム | 長さ=k.clamp(lo,hi)・ランダム文字 | k.clamp(lo,hi)行・ランダム文字 | ⭕️ |   |   | | ⭕️ | 小さい配列での動作 |
| ZeroCorner | lo<0<hi → 0、他ランダム | 全要素 lo<0<hi → 0、他ランダム | ランダム | ランダム | — |   |   | ⭕️ | ⭕️ | 符号変化・ゼロ除算・0境界 |
| SumMaxSingle 😡MaxSizeに改名する| T=1・各inner_var=min(sum上限, 変数上限)・他サイズ変数=最大・非サイズはランダム | ランダム要素 | 最大長・ランダム文字 | 最大行数×1回・ランダム文字 | — | 😡⭕️にする  | ⭕️😡この列は不要になる | | ⭕️ | sum制約下で全サイズ変数が最大となる単一ケース |
| ArrayMonoInc | ランダム | 単調増加列（行列は行ごとに増加） | charset 内で増加（先頭→末尾文字）（複数文字列は行ごとに変化） | 行ごとに charset 内で増加（例: `aaa`→`mmm`→`zzz`） | — | ⭕️ |   | | ⭕️ | ソート済み入力・二分探索の境界 |
| ArrayMonoDec | ランダム | 単調減少列（行列は行ごとに減少） | charset 内で減少（末尾→先頭文字） | 行ごとに charset 内で減少（例: `zzz`→`mmm`→`aaa`） | — | ⭕️ |   | | ⭕️ | 逆順ソート済み入力 |
| ArrayAllSame | ランダム | 全要素=同一ランダム値・全行同一 | 全文字列=同一ランダム1文字を繰り返し | 各行同一ランダム文字 | — | ⭕️ |   | | ⭕️ | 全同値・重複処理 |
| ArrayAltMaxMin | ランダム | 上限・下限を交互（最初の値はランダム） | charset の末尾文字・先頭文字を交互（最初の値はランダム） | 市松模様（末尾文字・先頭文字を行列インデックスで交互、初期値はランダム）（例: `#.#`/`.#.`/`#.#`） | — | ⭕️ |   | | ⭕️ | 交互パターン・奇偶インデックス処理 |
| ArrayMountain | ランダム | 増加→減少（山型） | charset 内で増加→減少（山型） | 行ごとに charset 内で増加→減少（山型）（例: `aaa`→`mmm`→`zzz`→`mmm`→`aaa`、各行は1種類の文字） | — | ⭕️ |   | | ⭕️ | 単峰性を仮定したアルゴリズム |
| ArrayOneMaxRestMin | ランダム | 中央1要素=上限、残り=下限 | 中央1文字列=末尾文字のみ、残り=先頭文字のみ | 中央1行=末尾文字のみ、残り=先頭文字のみ | — | ⭕️ |   | | ⭕️ | 外れ値・孤立した最大値 |
| ArrayNarrowRange | ランダム | 連続する2値（ランダム位置）のみを各要素に使用 | ランダム長・連続する2文字のみを各文字に使用 | 各行、連続する2文字のみを使用（行ごとにランダム） | — | ⭕️ |   | | ⭕️ | 値域が狭い場合のバグ・境界付近の挙動 |
| ArrayPeriodic ※1件 | ランダム | 2〜5要素（ランダム）の周期パターンを繰り返す | ランダム長・2〜5文字（ランダム）の周期パターンを繰り返す | 2〜5文字（ランダム）の周期パターンを各行に繰り返す | — | ⭕️ |   | | ⭕️ | 周期性を仮定・無視したアルゴリズム |
| Random | ランダム | ランダム | ランダム長・ランダム文字 | ランダム文字 | ⭕️ |   |   | | ⭕️ | 一般ケース |


### 出力フォーマット（ランダムテスト）

#### AC
```
(サンプルチェックの最終行)

══════════════════════════════════════════
               random tests
══════════════════════════════════════════
1/5 ("corner1") Accepted (12 ms)                           ←progress bar
2/5 ("corner2") Runtime Error (exit status: 1) (3 ms)

1/5 ("corner1") Accepted (12 ms)                           ←print_pretty
stdin:
{input}
actual:
{output}

2/5 ("corner2") Runtime Error (exit status: 1) (3 ms)
stdin:
{input}
actual:
EMPTY
stderr:

note: Accepted means no crash or TLE; output correctness is not verified ← ACがある場合のみ
warning: skipped N unsupported constraint(s): {制約内容}  ← スキップがある場合のみ
error: {失敗件数}/{総件数} tests failed  ← 失敗がある場合のみ

```

**注記:**
- `{name}` は コーナーケースなら`corner1`, `corner2`, ...,ランダムケースなら `random1`, `random2`, ... の形式
- Accepted はクラッシュ・TLEなしを意味し、出力の正しさは検証しない
- 各テストケースについてprint_prettyの出力をそのまま使う (上記フォーマットはそこを指定するものではない)
- スキップした制約の警告は**末尾のみ**出力する（`judge()` + `out:None` ベースに変更後）
-- snowchainsのpretty_printを素直に使うとこうならない(max,warning,errorの出力を入れ替えた方がよい)等あれば調整する → 自前で出しているから自由なはず
-- warning, error, noteは一貫した色をつけること
-- 最初に空行が必要なことなど、空行の有無に気を使うこと (クロスチェック側も同様)

---

## 機能2: クロスチェック（`--cross`）

### 実行フロー

1. メインバイナリのサンプルテスト（`--no-test` なら省略）✅ 実装済み
2. クロスバイナリを `Cargo.toml` に自動登録（未登録の場合）
3. クロスバイナリをビルド
4. クロスバイナリのサンプルテスト（`--no-test` なら省略）✅ 実装済み
   - 愚直解は低速なことが多いため、**制限時間なし**で実行する
5. ランダム入力をクロスバイナリに流して期待出力を収集（RE/TLE のケースはスキップ）
6. 期待出力に対してメインバイナリを判定
7. 1件でも WA/RE/TLE が出たら非ゼロ終了

#### snowchains を使ったクロスチェック実行フロー詳細

1. 全テストケースの入力を生成（`generate_random_input`）
2. クロスバイナリに `run_with_input` で実行 → `Ok(output)` のみ採用（RE/TLE はスキップ）
3. 採用ケースを `BatchTestCase { out: Some(brute_output), ... }` で構築　※brute_output = クロスバイナリの出力
4. メインバイナリに対して judge() を呼び出し(期待値をクロスバイナリの出力とする) **progress_barあり**
5. `JudgeOutcome { verdicts: outcome.verdicts.into_iter().filter(非AC).collect() }` でフィルタリングし `print_pretty` — 通番は 1/N 形式にリセットされる（フィールドがpublicなため手動構築可）

### Cargo.toml 自動登録

- `[[bin]]` エントリと `[package.metadata.cargo-compete.bin]` エントリを同時に追加
- bin name: `{contest}-{ファイル名stemのkebab変換}` 例: `abc440-a-brute`
- alias: ファイル名stemのkebab変換 例: `a-brute`
- 既に登録済みならスキップ

### 比較方法

- `a.yml`（テストスイート）の `match:` フィールドを使用（`Exact` / `Lines` / `Float` など）

### 出力フォーマット（クロスチェック）

クロスバイナリのサンプルテスト後、ランダムケースの判定結果を以下の形式で表示する。

#### AC
```
(サンプルチェックの最終行)

══════════════════════════════════════════
      cross-check binary sample tests
══════════════════════════════════════════
1/3 ("sample1") Accepted (0 ms)           ←progress bar
2/3 ("sample2") Accepted (0 ms)
3/3 ("sample3") Accepted (0 ms)

1/3 ("sample1") Accepted (0 ms)           ←print_pretty
stdin:
3 5
expected:
3
actual:
3

2/3 ("sample2") Accepted (0 ms)
stdin:
1 7
expected:
7
actual:
7

3/3 ("sample3") Accepted (0 ms)
stdin:
14 79
expected:
66
actual:
66

══════════════════════════════════════════
            cross-check tests
══════════════════════════════════════════
1/3 ("corner1") Accepted (5 ms)                         ←progress bar
2/3 ("corner2") Wrong Answer (8 ms)
3/3 ("corner3") Runtime Error (exit status: 1) (2 ms)           

1/2 ("corner2") Wrong Answer (8 ms)
stdin:
{input}
expected:
{brute-force output}
actual:
{main binary output}

2/2 ("corner3") Runtime Error (exit status: 1) (2 ms)
stdin:
{input}
actual:
EMPTY

expected: a-copy ←AC以外がある場合のみ
actual: a        ←AC以外がある場合のみ

warning: skipped N unsupported constraint(s): {制約内容}  ← スキップがある場合のみ
error: {失敗件数}/{総件数} tests failed  ← 失敗がある場合のみ
```

**注記:**
- `{name}` は コーナーケースなら`corner1`, `corner2`, ...,ランダムケースなら `random1`, `random2`, ... の形式
- 各テストケースについてprint_prettyの出力をそのまま使う (上記フォーマットはそこを指定するものではない)
- スキップした制約の警告は**末尾のみ**出力する（`judge()` + `out:None` ベースに変更後）
-- snowchainsのpretty_printを素直に使うとこうならない(max,warning,errorの出力を入れ替えた方がよい)等あれば調整する

#### 末尾
```
warning: skipped N unsupported constraint(s): {制約内容}  ← スキップがある場合のみ
error: {失敗件数}/{総件数} tests failed  ← 失敗がある場合のみ
```

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

1. **sum constraint の残課題**: テストケース内の変数に連鎖制約があったときに、下限と上限だけからランダム生成して連鎖制約を満たさなければ棄却する、とするべきなのにそうなってないと聞いた
2. abc431-c 全然上限守れてない
