# parse.rs 制約読み取り詳細設計書

## 1. 概要

`src/random_test/parse.rs` は AtCoder の問題ページから取得した制約テキストと入力フォーマットを解析し、ランダムテスト用の入力を生成するためのデータ構造に変換するモジュール。

主な責務:
1. 制約項目（`<ul>` の `<li>` テキスト）を解析し、各変数の上下限・順序・文字列仕様を抽出
2. 入力フォーマット行（`<pre>` ブロック）を解析し、`InputBlock` 列に変換
3. 生成時のリトライ条件（`var_to_var`, `var_not_eq`）を構築

---

## 1.1 input_template.rs との共有アーキテクチャ

`src/web/input_template.rs` は `proconio::input!` 宣言を生成するヒューリスティックエンジンであり、**型認識・配列認識・要素数認識** のロジックが高精度に実装されている。`parse.rs` はこれらを独自実装せず、共通関数として利用する。

### 責務分担

| 責務 | 担当モジュール |
|---|---|
| 型認識（Chars / i64 / usize） | `input_template.rs` |
| 配列パターン認識（横・縦・NRepeat） | `input_template.rs` |
| 配列要素数（SizeRef）の解析 | `input_template.rs` |
| 変数の上界・下界（lo / hi） | `parse.rs` |
| 順序制約（var_to_var） | `parse.rs` |
| 不等制約（var_not_eq） | `parse.rs` |
| 文字セット（LowerAlpha 等）と文字列長 | `parse.rs`（`からなる`制約） |
| Sum 制約 | `parse.rs` |

### input_template.rs が pub(crate) で公開する関数

| 関数 | 用途 |
|---|---|
| `parse_1d_array_line` | 横並び配列の認識 |
| `parse_n_repeat` | 複数列 N 行繰り返しの認識 |
| `parse_vertical_scalars` | 数値変数の縦並びの認識 |
| `parse_grid_lines` | 文字列変数（S_1…S_H）の縦並びの認識 |
| `parse_grid_row` | グリッド行（S_{1,1}…S_{1,W}）の認識と幅抽出 |
| `is_string_symbol` | 命名規則による文字列変数判定（S/T/U/X など）← **新規追加** |
| `num_ty_for_base` | 変数名から usize/i64 を推定 ← **新規追加** |
| `base_var` / `base_vars` | トークンから変数名を抽出（`\mathrm` 等 LaTeX マクロも除去）← **新規追加** |
| `signed_bases` | 制約テキストから i64 になる変数セットを返す ← **新規追加** |

### extract_var_names との関係

`parse.rs` の `extract_var_names` と `input_template.rs` の `base_var` / `base_vars` は同じ役割を持つ重複実装。`base_var` のほうが `\mathrm`, `\text`, `|...|` 等のLaTeXマクロ除去も行うため汎用性が高い。将来的には `extract_var_names` を廃止し `base_vars` に統一することを検討する。

同様に `parse.rs` で制約 lo < 0 を各変数ごとに検出しているロジックは、`signed_bases` と重複している。`signed_bases` を共有することで i64 判定を一元化できる。

### 型認識の優先順位

1. **`parse_string_constraints`**（`からなる` 制約）: 文字セット（LowerAlpha / UpperAlpha / Explicit）および文字列長を取得。この方法で登録された変数は確定的に文字列。
2. **`is_string_symbol`**（命名規則）: 制約に `からなる` がない場合のフォールバック。S/T/U/X 等の命名で文字列と推定。生成時はデフォルト charset（LowerAlpha）と length（1〜100）を使用。
3. **符号（i64 / usize）**: `num_ty_for_base` を参照。lo が負の `Lit(n)` の場合は i64 として生成値を許容。ランダム生成では型名は不要で、`bounds.lo` が負の場合は負数を生成する。

---

## 2. 主要な型

### BoundVal

変数の上限・下限を表す。

| バリアント | 意味 | 例 |
|---|---|---|
| `Lit(i64)` | 数値リテラル | `300000` |
| `Var(String)` | 別変数への参照 | `N`（`A_i <= N` のとき） |
| `VarOffset(String, i64)` | 変数 + 定数オフセット | `N-1` |
| `Set(Vec<i64>)` | 離散集合 | `{1, 2}`（いずれか制約） |

デフォルト: `lo = Lit(1)`, `hi = Lit(1_000_000_000)`

### VarBound

```rust
struct VarBound { lo: BoundVal, hi: BoundVal }
```

### ConstraintParsed

```
bounds       : HashMap<varname, VarBound>   // 変数ごとの上下限
var_to_var   : Vec<(lo_var, hi_var)>        // lo_var <= hi_var の順序対
var_not_eq   : Vec<(a, b)>                  // a != b の不等対
string_vars  : HashMap<varname, StringVarSpec>
skipped      : Vec<String>                  // 解析できなかった制約（ユーザーへ警告）
```

### InputBlock

| バリアント | 意味 | 例 |
|---|---|---|
| `Scalars(Vec<String>)` | 同一行のスカラー | `N M` |
| `Array1D { base, len }` | 横並び1次元配列 | `A_1 A_2 ... A_N` |
| `NRepeat { cols, count }` | 複数列 N 行繰り返し | `x_i y_i` × N 行 |
| `Vertical { base, count, width }` | 縦並び（数値 or 文字列）| `B_1 ⋮ B_N`、グリッド行 |
| `OuterRepeat { count, inner }` | 外側テストケースループ | T テストケース形式 |
| `TypedRepeat { count, branches }` | クエリ種別分岐繰り返し | `1 x` / `2 l r` 形式 |
| `Unsupported(())` | 解析不能 | — |

### TypedBranch

```rust
struct TypedBranch {
    type_val: String,      // クエリ種別トークン ("1", "2" 等)
    inner: Vec<InputBlock>, // 種別トークン以降の変数ブロック
}
```

`TypedRepeat { count, branches }` は Q 回クエリを生成し、各クエリでランダムに branch を選んで `"{type_val} {inner...}"` の形式で出力する。

---

## 3. parse_constraints() の処理フロー

```
入力: &[String]  ← 制約 <li> テキストのリスト
出力: ConstraintParsed
```

各制約項目に対して **順番に** 以下の判定を行う:

### 3.1 Enum 制約チェック（最優先）

`try_parse_enum_constraint(item)` を呼ぶ。

**対応パターン:**
- `B_i は 1,2 のいずれか` (日本語 "のいずれか" / "いずれかの" 形式)
- `B_i \in \{1, 2\}` (LaTeX `\in` 形式)

**処理:**
1. `は` の前を変数名リスト、後ろを数値列として解析
2. `bounds[var].hi = Set(vals)`, `bounds[var].lo = Lit(min(vals))` を設定
3. マッチした場合は次の制約項目へ（以降のステップをスキップ）

---

### 3.2 正規化

`normalize_for_random(item)` を通す。内部で以下を実行:

1. **`normalize_constraint(s)`** (input_template.rs):
   - `≤` / `≦` → `<=`
   - `≥` / `≧` → `>=`
   - `\leq` / `\le` → `<=`
   - `\geq` / `\ge` → `>=`
   - `−` (全角マイナス) → `-`
   - `\times` / `×` → `*`
   - **スペースを全削除** (`replace(' ', "")`)

   > ⚠️ 日本語文字はここでは除去されない。スペース削除のみ。

2. `\neq` / `\ne` → `!=`
3. `\min\left(` / `\min(` → `min(`
4. `\max\left(` / `\max(` → `max(`
5. `\left(` → `(`, `\right)` → `)`

---

### 3.3 早期スキップ

正規化後の文字列に `dfrac`, `frac`, `sqrt` を含む場合 → `skipped` に追加して次の項目へ。
（分数・平方根を含む制約は数値評価できないため）

---

### 3.4 トークン分割

正規表現 `(<=|>=|!=|<|>)` で演算子を検出し、制約式をトークン列と演算子列に分割。

```
"1<=N<=50"
  → tokens=["1","N","50"],  ops=["<=","<="]

"1<=A_i,B_i<=N"
  → tokens=["1","A_i,B_i","N"],  ops=["<=","<="]

"A_i!=B_i"
  → tokens=["A_i","B_i"],  ops=["!="]

"N は 1 \le N \le 50 を満たす整数"
  (normalize後: "Nは1<=N<=50を満たす整数")
  → tokens=["Nは1","N","50を満たす整数"],  ops=["<=","<="]
```

**`ops.is_empty()` の場合**: `skipped` に追加して次の項目へ。

---

### 3.5 != 処理

`ops[j] == "!="` の場合:
- `extract_var_names(tokens[j])` と `extract_var_names(tokens[j+1])` で変数名を抽出
- 各1変数のペアを `var_not_eq` に追加（重複排除）

---

### 3.6 var_to_var 順序対の収集

`ops[j] == "<="` または `"<"` の場合:
- `(extract_var_names(tokens[j]), extract_var_names(tokens[j+1]))` のペアを `var_to_var` に追加

`ops[j] == ">="` または `">"` の場合 (逆向き):
- `(extract_var_names(tokens[j+1]), extract_var_names(tokens[j]))` の**逆順**ペアを追加
- `a > b` は `b <= a` を意味するため reversed pair `("b", "a")` を記録

例: `1<=x<=y<=50` → `var_to_var` に `("x","y")` が追加される。  
例: `N>=x>=1` → `var_to_var` に `("x","n")` が追加される。

> この情報は生成時のリトライ条件に使われる。x と y を独立に生成し、`x <= y` でなければ再生成。

---

### 3.7 変数ごとの上下限設定（メインループ）

各トークン `tokens[i]` に対して:

1. `extract_var_names(tokens[i].trim())` で変数名リストを取得
2. 変数が空ならスキップ
3. **lo の決定** (下記のいずれか、先に成立した方を採用):
   - `i > 0` かつ `ops[i-1] == "<="` または `"<"` → `parse_bound_val(tokens[i-1].trim())`
   - `i < ops.len()` かつ `ops[i] == ">="` または `">"` → `parse_bound_val(tokens[i+1].trim())`
4. **hi の決定** (下記のいずれか):
   - `i < ops.len()` かつ `ops[i] == "<="` または `"<"` → `parse_bound_val(tokens[i+1].trim())`
   - `i > 0` かつ `ops[i-1] == ">="` または `">"` → `parse_bound_val(tokens[i-1].trim())`
5. 各変数について `bounds[var]` を更新。ただし lo/hi が `None` の場合はデフォルト値を維持。

---

### 3.8 parsed_any チェック

Step 3.7 で有効な変数が1つも見つからなかった場合、かつ `!=` 演算子もない場合:
→ `skipped` に追加

---

### 3.9 Sum 制約の適用

ハンドラパイプラインのステップ5（`item.contains("の総和は")`）で `try_parse_sum_constraint_item` を呼ぶ。

**検出パターン**:
- `「X の総和は Y 以下」`
- `「X の総和は Y を超えない」`

**重要**: `split("以下").next()` は "以下" が存在しない場合に文字列全体を返してしまうため、`find_map` で正確に検索する:
```rust
let limit_raw = ["以下", "を超えない"]
    .iter()
    .find_map(|&d| after.find(d).map(|i| after[..i].trim()))
    .unwrap_or("");
```

**処理**:
1. `X` を内側変数名として取得（`"の総和は"` の前をスペース区切りで最後のトークン）
2. `Y` を上限値として評価（`normalize_constraint` + `eval_expr`）
3. `sum_based_t_hi = (Y / lo(X)).min(200)` で T/Q の上限をキャップ
4. `inner_hi = (Y / sum_based_t_hi).max(1)` で X の上限もキャップ（AllMax が制限超過にならないよう）
5. `SumConstraint { inner_var: X, limit: Y }` を `ConstraintParsed.sum_constraints` に格納

**スキップ条件**:
- `^` を含む（`N^2 の総和` 等）

**SumMaxSingle コーナーケース戦略**:

`make_strategy_list` が `sum_constraints` を受け取り、各 `SumConstraint` に対して `CaseStrategy::SumMaxSingle { inner_var, limit }` を生成する。このコーナーケースでは T=1・X=limit（MAX_ARRAY_SIZE でキャップ）を生成し、「N が最大になるケース」を確実にテストする。

```
AllMax (T=200, X=inner_hi)   ← X の上限が小さくても T 件分の大きな入力
AllMin
SmallSize(1/2/3)
SumMaxSingle (T=1, X=limit)  ← X が最大になる唯一のケース
Array系コーナーケース...
```

---

### 3.9.1 生成時の Sum 制約チェック (`generate_random_input`)

`process_blocks` の `OuterRepeat` 処理で、各内側イテレーション後に内側変数の値を累積する:
```
ctx["__sum_{var}"] += local_ctx[var]  // 外側 ctx に prefix 付きで蓄積
```

`generate_random_input` で生成後に確認:
```
sum_ok = sum_constraints.all(|sc| ctx["__sum_{sc.inner_var}"] <= sc.limit)
```

**リトライポリシー**:
- `Random` 戦略: 無制限リトライ（`Some(input)` を必ず返す）
- コーナーケース戦略（`AllMax`, `AllMin`, `SmallSize`, `Array*` 等）: 最大20回リトライ後、`None` を返しケースをスキップ

> ⚠️ **設計メモ（コーナーケースのスキップ）**: `None` が返された場合、呼び出し側（`run_random_tests` / `run_cross_check`）はそのケースを実行せず "skipped" として扱う。ただし `strategies` の for ループは継続し、そのスロットは消費済みとなる。**違反する入力を出力することは絶対にない**。
> コーナーケースは優先的に全潰しする要件があるため、スキップしても全戦略のスロットは消化される（その枠は単にスキップ表示になる）。

---

### 3.10 文字列制約の解析

2段階で文字列変数を確定する。

#### 「」マーカーによる Explicit charset 検出

`extract_constraints_items`（`input_template.rs`）が HTML を処理する際、`<li>` 要素内に `<code>` タグが 2 個以上ある場合は文字の列挙とみなし、各 `<code>X</code>` を `「X」` に置換する。

```rust
let processed = if li_html.matches("<code>").count() >= 2 {
    code_re.replace_all(li_html, "「$1」").to_string()
} else {
    li_html.to_string()
};
```

`parse_string_constraints` はその後 `extract_quoted_chars` で `「X」` マーカーから文字を抽出し `CharSet::Explicit` を構築する。これにより `からなる`/`いずれか` 等のテキストパターンマッチングが不要になり、HTML 構造だけで Explicit charset を確定できる。

**`<code>` 1個のケース**: 否定形 (`≠`を含む参照)・特定位置参照など、文字列ではない文脈で使われるため 「」 置換をスキップする。

#### ステップ A: `parse_string_constraints(items)` — 文字セット・長さの取得（主経路）

**検出条件**: 項目中に ` は` があること（英小文字/英大文字 のキーワードまたは「」マーカーがあること）。

**主要パターン**:
- `S は英小文字からなる長さ N の文字列` → `CharSet::LowerAlpha`, `hi_len = Var("n")`
- `S は英大文字からなる長さ N の文字列` → `CharSet::UpperAlpha`
- `S_{i,j} は 「#」 か 「.」` (= HTML `<code>#</code> か <code>.</code>`) → `CharSet::Explicit(['#','.'])`

**長さ仕様 (`parse_length_spec`)**:
- `長さ X 以上 Y 以下` → `lo_len=X, hi_len=Y`
- `長さ N の` → `lo_len=N, hi_len=N` (exact)
- どちらにも該当しない → `lo_len=1, hi_len=Lit(100)` (デフォルト)

続いて `apply_abs_length_constraints(items, &mut result)` で `|S| <= N` 形式の制約を文字列変数の長さに反映:
- `1 \le |S| \le N` → `spec.lo_len=1, spec.hi_len=Var("n")`

#### ステップ B: `is_string_symbol(sym)` — 命名規則によるフォールバック（未実装）

入力フォーマットブロックを走査し、`is_string_symbol` が true を返す変数（S / T / U / X 等）がステップ A で未登録の場合、デフォルト仕様（`LowerAlpha`, `lo_len=1`, `hi_len=Lit(100)`）で補完登録する。

#### ステップ C: `apply_abs_length_constraints(items, &mut string_vars)` — `|S| \le N` 形式の長さ反映

Step A・B が完了した **後で** 実行する。こうすることで Step B で追加登録された変数にも長さ制約が適用される。

この後 skipped のフィルタリング（3.11節）で、Step C で処理済みの `|S|` 制約を skipped から除外する。

**後処理**: 文字列変数として登録されたものは `bounds` から除去（数値変数と二重登録を防ぐ）。

---

### 3.11 skipped の決定方式（ハンドラパイプライン）

各制約項目はハンドラを順番に試み、**いずれかが handled = true を返したら skipped に追加しない**。末尾フィルターは存在しない。

ハンドラ順序:
1. Enum 制約
2. 文字列制約 (`からなる`) — pre-pass で処理済み
3. abs-length 制約 (`|S| <= N`) — pre-pass で処理済み
4. 相異なる制約
5. 総和制約
6. 数値不等式
7. Ignorable（`は` + `整数`/`正整数`）— アクション不要だが handled = true

詳細は § 11 参照。

---

## 4. parse_bound_val() の解析フロー

与えられた文字列 `expr` を `BoundVal` に変換する。以下の順に試行する。

### 4.1 min(...) 処理
`expr` が `min(...)` 形式の場合:
- 第1引数（最初の `,` または `)` まで）を再帰的に `parse_bound_val` で解析
- 例: `min(N, 1000000)` → `parse_bound_val("N")` → `Var("n")`

### 4.2 数値評価 (eval_expr)
`eval_expr(expr)` を試みる:
- 整数文字列 → `Lit(n)`
- `A*B` 形式 → 積
- `A^B` 形式 (B≤30) → 累乗
- 例: `3*10^5` → `Lit(300000)`

### 4.3 VarOffset (var-k 形式)
`expr.split_once('-')` で分割:
- 左辺 = ASCII 英数字のみかつ先頭が英字の変数名
- 右辺 = 整数 k
- → `VarOffset(var.to_lowercase(), -k)`
- 例: `N-1` → `VarOffset("n", -1)`

### 4.4 VarOffset (var+k 形式)
`expr.split_once('+')` で分割、同様:
- 例: `N+1` → `VarOffset("n", 1)`

### 4.5 単純変数名
`expr` が **ASCII 英数字のみ**（アンダースコアを含まない）かつ先頭が英字:
- → `Var(expr.to_lowercase())`
- 例: `N` → `Var("n")`, `MAX` → `Var("max")`

> ⚠️ **アンダースコア除外の理由**: `R_i` のような添字付きトークンが上界式として現れた場合（`1 ≤ R_i ≤ N` を解析して N の lo = `parse_bound_val("R_i")` が呼ばれる）、`Var("r_i")` と登録してしまうとランダム生成時に `ctx["r_i"]` が存在せず `resolve_bound` のフォールバックで 1,000,000,000 が返される。これにより N の lo が 10^9 になり、AllMax/AllMin の生成が壊れる。対策としてアンダースコアを含む名前は `Var` として返さないようにした。

### 4.6 非ASCII 分割フォールバック

日本語等の非ASCII文字を含む場合のフォールバック:
1. `expr.split(|c: char| !c.is_ascii())` で非ASCII文字を区切り文字としてセグメントに分割
2. 各セグメントを trim・空文字除去し **右から順に** `parse_bound_val` を再帰的に試みる
3. 成功した最初の結果を返す

**動作例**:

| 入力 (normalize後) | 分割結果 | 解析結果 |
|---|---|---|
| `"50を満たす整数"` | `["50", ""]` → `["50"]` | `Lit(50)` ✓ |
| `"Nは1"` | `["N", "1"]` → 右から: `"1"` | `Lit(1)` ✓ |
| `"NはM"` | `["N", "M"]` → 右から: `"M"` | `Var("m")` ✓ |

> `normalize_constraint` がスペースを全削除するため `"Nは1"` のような境界なしトークンが生じるが、非ASCII文字（`は`）を区切りとして分割することで正しくパースできる。

---

## 5. extract_var_names() の動作

`,` で分割して各部分から変数名を抽出:
- `_` があれば `_` 以前を base とする（例: `A_i` → `"a"`, `A_{i,j}` → `"a"`）
- `{`, `}` を除去
- **すべての文字が ASCII 英数字**かつ先頭が英字の場合のみ返す（非ASCII文字を含む場合は除外）

> `input_template.rs` の `base_var` / `base_vars` が同じ役割を担う。`base_var` は加えて `\mathrm`, `\text`, `|` 等のLaTeX除去も行うため汎用性が高い。将来的には `base_vars` に統一予定（1.1節参照）。

**例**:

| 入力 | 結果 | 備考 |
|---|---|---|
| `"A_i,B_i"` | `["a", "b"]` | ✓ |
| `"N"` | `["n"]` | ✓ |
| `"Nはx"` | `[]` | 非ASCII文字 `は` を含むため除外 |
| `"1を満たす整数"` | `[]` | 先頭が '1'（数字）なので除外 |

> 以前は先頭が ASCII 英字であれば通過させていたため `"Nはx"` → `["nはx"]` という bogus 変数が生成されていた。現在は `chars().all(|c| c.is_ascii_alphanumeric())` チェックにより正しく除外される。

---

## 6. parse_input_blocks() の処理フロー

```
入力: &[String]  ← 正規化済み入力フォーマット行
出力: Vec<InputBlock>
```

全関数は `input_template.rs` から `pub(crate)` でインポートする（`parse.rs` が独自実装しない）。

各行に対して以下の順に判定（先に合致したものを採用）:

1. **プレースホルダースキップ**: `is_case_placeholder_line()` / `is_query_placeholder_line()` / `\vdots` → スキップ
2. **NRepeat**: `parse_n_repeat(&norm, i)` → `InputBlock::NRepeat`  
   `x_1 y_1` ～ `x_N y_N` のような複数列・複数行パターン（**input_template.rs 共有**）
3. **Vertical (数値変数)**: `parse_vertical_scalars(&norm, i)` → `InputBlock::Vertical`  
   `B_1 ⋮ B_N` のような縦並びパターン（`S` 系は除外）（**input_template.rs 共有**）
4. **Vertical (文字列縦並び)**: `parse_grid_lines(&norm, i, None)` → `InputBlock::Vertical { width: None }`  
   `S_1 ⋮ S_H` のような1行1文字列の縦並び（**input_template.rs 共有**）
5. **Vertical (グリッド行)**: `parse_grid_row(lines, orig_lines, i)` → `InputBlock::Vertical { width: Some(...) }`  
   `S_{1,1}S_{1,2}...S_{1,W}` のような連結グリッド行。`width` に列数 `W` を格納し、生成時に各行を固定長文字列として出力する。  
   ※ `parse_grid_row` は `input_template.rs` の `parse_grid_row_block` + `parse_row_with_ellipsis` を組み合わせた `pub(crate)` ラッパー
7. **Array1D**: `parse_1d_array_line(ln)` → `InputBlock::Array1D`  
   `A_1 A_2 \ldots A_N` のような横並び配列（**input_template.rs 共有**）
8. **Scalars**: `\ldots` 等を含まない + 英字始まりトークンがある → `InputBlock::Scalars`
9. **Unsupported**: 上記に合致しない場合

---

## 7. build_input_blocks() による OuterRepeat 検出 (mod.rs)

`TaskSection.input_blocks: Vec<Vec<String>>` は複数の `<pre>` ブロックを含む場合がある。

```
入力: Vec<Vec<String>>  ← 複数 <pre> ブロックのリスト
出力: Vec<InputBlock>
```

**OuterRepeat 検出条件**:
- `input_blocks.len() >= 2`
- かつ `input_blocks[0]` のいずれかの行が `is_case_placeholder_line()` または `is_query_placeholder_line()` にマッチ

**OuterRepeat / TypedRepeat の判定フロー**:

1. `input_blocks[0]` からプレースホルダー行と `\vdots` を除いた外側ブロックを解析
2. 外側ブロックの最後の `Scalars` の最後の変数名を `count` とする
3. `input_blocks[1..]` の各ブロックの先頭行がすべて整数リテラルで始まる AND 先頭ブロックにクエリプレースホルダーあり → **TypedRepeat**
   - 各ブロックの先頭トークンを `type_val`、残りを `inner` として `TypedBranch` を構築
4. それ以外 → **OuterRepeat**
   - `input_blocks[1..]` を結合して `inner` とする

**TypedRepeat の生成**:
- `count` 回クエリを生成
- 各クエリでランダムに branch を選択
- `"{type_val} {inner_first_line}"` の形式で 1 行目を出力、後続行をそのまま出力

**OuterRepeat が検出されなかった場合**:
- 全 `<pre>` ブロックを結合して通常通り解析

---

## 8. 未対応・既知の制限

| ケース | 状態 | 影響 |
|---|---|---|
| `>` / `>=` による bounds / var_to_var | **対応済み** | `4>x>1` → x.lo=1, x.hi=4; `N>=x` → var_to_var に `("x","n")` |
| `N は x>1 を満たす` | **部分対応** | token[0]=`"Nはx"` は非ASCIIで除外されるため x の bounds は未設定 |
| `N は 4>x>1 を満たす` | **対応済み** | token[1]=`"x"` が `>` で挟まれ x.lo=1, x.hi=4 が設定される |
| `N は 1<=N<=50 を満たす整数` | **対応済み** | parse_bound_val の非ASCII分割で `Lit(1)` / `Lit(50)` を正返却 |
| `R_i` 等の添字付きトークンが bounds の lo/hi に現れる | **対応済み** | parse_bound_val がアンダースコアを含む名前を Var と見なさず None を返す（§4.5 参照） |
| `を超えない` 形式の sum 制約 | **対応済み** | `find_map` で正確に区切り文字を検索 |
| クエリ種別分岐入力（TypedRepeat） | **対応済み** | `TypedRepeat { count, branches }` として実装、abc442/d 等で動作確認 |
| 分数 `frac` / 平方根 `sqrt` 制約 | スキップ | skipped に追加・警告出力 |
| `N^2 の総和` sum 制約 | スキップ | sum 制約未適用 |
| グリッド入力（H×W 文字グリッド） | **対応済み** | `parse_grid_row` で検出 → `Vertical { width: Some(W) }` → `gen_string` で各行を固定幅で生成 |
| 文字列の順序制約（「昇順」「全て異なる」等） | 未対応 | abc441/b 等で RE が発生するが skipped 警告を出力 |
| `parse_string_vertical` の重複 | **対応済み** | `parse_grid_lines` を pub(crate) 化し戻り値を統一 |
| `is_string_symbol` による文字列フォールバック | **対応済み** | `apply_string_symbol_fallback` として実装 |

---

## 9. Q&A

**Q: `1<=x<=y<=50` は正しく処理できますか？**

A: **はい**。以下のように処理される:
- `x.lo = Lit(1)`, `x.hi = Var("y")`
- `y.lo = Lit(1)`, `y.hi = Lit(50)`
- `var_to_var` に `("x","y")` が追加される

生成時は `x` と `y` を独立に生成し、`x <= y` を満たさなければ再生成（`generate_random_input` の `loop`）。

**Q: `N は x>1 を満たす` は処理できますか？**

A: **部分的**。normalize後 `"Nはx>1を満たす"` → tokens = `["Nはx", "1を満たす"]`, ops = `[">"]`。
- token[0] = `"Nはx"`: `extract_var_names` が非ASCII `は` を含むため返す変数 = `[]`（bogus 変数名 `"nはx"` の登録は避けられる）
- x の bounds は設定されない → 別の制約項目（例: `1 ≤ x ≤ N`）が必要

**Q: `N は 4>x>1 を満たす` は処理できますか？**

A: **対応済み**。normalize後 `"Nは4>x>1を満たす"` → tokens = `["Nは4", "x", "1を満たす"]`, ops = `[">", ">"]`。
- token[1] = `"x"`: vars = `["x"]`
- lo: ops[1]=">" → `parse_bound_val("1を満たす")` = `Lit(1)` ✓
- hi: ops[0]=">" → `parse_bound_val("Nは4")` → 非ASCII分割: `["N","4"]` → 右から `Lit(4)` ✓
- 結果: x.lo=1, x.hi=4 ✓

---

## 10. abc440〜abc444 テスト結果レビュー Q&A

**Q: abc440/c で `--random` 実行すると一向に出力されないのはなぜ？**

A: `N の総和は 2×10^5 以下` という sum 制約があるにもかかわらず、T.hi=200000、N.hi=200000 のまま AllMax ケースを生成しようとするため、200000×200000=4×10^10 要素の生成でハングしていた。  
**Fix**: `try_parse_sum_constraint_item` で以下の 2 点を実施:
1. `sum_based_t_hi = (limit / lo).min(200)` で T.hi をキャップ
2. `inner_hi = (limit / sum_based_t_hi).max(1)` で N.hi もキャップ（AllMax が sum 制限を超えないよう）

😡Tのキャップはいいですが、それとは別にAllMaxについては特別にT=1、N=200000(Nの総和制限)が固定で生成されるようにしましょうか。動作確認済み（👍）
😡Nが最大を取るケースが実用上重要だからです。動作確認済み（👍）

**→ SumMaxSingle コーナーケースとして実装済み**。`CaseStrategy::SumMaxSingle { inner_var, limit }` により T=1・N=limit（MAX_ARRAY_SIZE でキャップ）のケースを生成する。abc443/d で T=1・N=200000 が生成されることを確認済み。

---

**Q: abc441/c で SmallSize(1) なのに N=300000 が生成されるのはなぜ？**

A: `gen_scalar` の旧コードが `n.max(lo).min(hi)` とクランプしていた。`bounds["n"].lo = Var("k")` → `bound_lit_hi("k")` → `k.hi = Var("n")` → n.hi=300000 と展開されて lo=300000 となり、`1.max(300000) = 300000` が返されていた。  
**Fix**: SmallSize ケースではクランプを外し `return n;` だけにする。

---

**Q: abc441/e の「S は A, B, C からなる」が skipped に入るのはなぜ？他の「からなる」は読み取れているのに。**

A: この制約項目には不等号演算子がなく、旧メインループの末尾条件 `if !parsed_any && !ops.iter().any(|o| o == "!=")` で `skipped.push` されていた。`parse_string_constraints` は正しく S を `string_vars` に登録できていたが、旧末尾フィルターが「からなる」+「string_vars に登録済み」のケースを除外していなかった。  
**Fix**: フィルターに「からなる」かつ変数が `string_vars` にあれば除外する条件を追加（現在はハンドラパイプライン刷新により自然解決）。

---

**Q: abc442/a で変数名が S なのに文字列と判定されないのはなぜ？**

A: `apply_string_symbol_fallback` は **Vertical ブロックのみ**を対象としている（Scalars の S/X/T/U は数値のことが多いため意図的に制限）。abc442/a は S 単独の Scalars ブロックなので検出されない。また「S は長さ 1 以上 10 以下の英小文字からなる文字列」は `parse_string_constraints` が処理すべきだが、変数名抽出や以上/以下形式の長さ解析が正しく動作していない可能性がある（要確認）。
😡文字列としては解釈されてますね、今試したところ。スキップとして出力されてしまうだけです。また、最新版ではskipとも表示されないことを確認しました。👍
---

**Q: abc443/a の「長さ N 以上 M 以下」（日本語の以上/以下）に対応できていますか？**

A: `parse_length_spec` に `長さ\s*(.+?)\s*以上\s*(.+?)\s*以下` の regex が実装済みで対応している。abc442/a・abc443/a で実際に機能するかは未テスト。

---

**Q: abc442/d、abc443/d — クエリ型分岐入力への対応は？**

A: **実装済み**。`InputBlock::TypedRepeat { count, branches: Vec<TypedBranch> }` として実装。
- `build_input_blocks` が task.html の複数 `<pre>` ブロックを解析し、各ブロックが整数リテラルで始まる場合に TypedRepeat と判定
- 生成時は Q 回クエリをランダムに branch 選択して出力（abc442/d: `1 x` / `2 l r` 等）
- abc442/d で全件 Accepted を確認

😡abc442のd.rsを見るとinput_temlate.rsは対応できているので、同じ方法で対応してください 動作確認済み（👍）

---

**Q: abc443/e、abc443/f、グリッド入力、OuterRepeat の対応は？**

A:
- **グリッド入力** (`S_{i,j} は # か .` 等): 固定長文字列（`CharSet::Explicit`）として扱う。Vertical ブロックで各行を文字列として生成するため既存の gen_string ロジックで対応可能。
😡 これは固定長の文字列と同じなので課題はないと思っています。任せました。
- **OuterRepeat**: abc443/d で T=200 テストケース形式が正しく動作することを確認済み。
- **abc443/f**: 全件 Accepted を確認済み（スカラー N 入力のシンプルな形式）。


---

## 11. 直近セッション Q&A（バグ修正・機能追加）

**Q: abc443/d で AllMax が「sum constraint unsatisfiable」とスキップされた。なぜ？**

A: 原因は 2 つの別々なバグの複合。

**バグ1: sum 制約が skipped に入っていた**

`"の総和は〜を超えない"` の解析で:
```rust
// 旧コード（バグあり）
let limit_raw = after.split("以下").next()  // "以下" がなくても全文字列を返す
    .or_else(|| after.split("を超えない").next())  // or_else は Some が返ると実行されない
```
`split("以下").next()` は "以下" が存在しない場合も `Some(全文字列)` を返すため `.or_else` が機能せず、`limit_raw = "3 \\times 10^5 を超えない"` のまま `eval_expr` が失敗していた。

**Fix**: `find_map` で正確に検索:
```rust
let limit_raw = ["以下", "を超えない"]
    .iter()
    .find_map(|&d| after.find(d).map(|i| after[..i].trim()))
    .unwrap_or("");
```

**バグ2: bounds["n"].lo が Var("r_i") で 10^9 に解決された**

`"1 ≤ R_i ≤ N"` の解析で、N の lo = `parse_bound_val("R_i")` が呼ばれ `Var("r_i")` を返していた。`resolve_bound(Var("r_i"), ...)` は ctx にも bounds にも "r_i" が存在しないためフォールバック値 1,000,000,000 を返し、N.lo = 10^9 となっていた。

結果: AllMax で N.lo=10^9 > N.hi=1500 → swap → N_actual=10^9 → MAX_ARRAY_SIZE でキャップされ N=200,000 → T=200, N=200,000 → sum = 40,000,000 >> 300,000 → 20 回リトライ後スキップ。

**Fix**: `parse_bound_val` がアンダースコアを含む名前を `Var` と見なさないようにした（§4.5）。

---

**Q: SumMaxSingle の優先順位を `AllMin` の直後に上げていたが、なぜ元に戻したのか？**

A: 当初は「デフォルト count=5 以内で SumMaxSingle が切り捨てられる」ことを避けるため早い順番に配置した。しかし `--random <数値>` で件数を増やせば済むため、優先順位不要と判断し SmallSize の後ろに移動した。コーナーケースの選択は将来的にランダム選択に移行予定のため、順序に依存した設計は避ける。

---

## 12. 生成時の制約反映まとめ

| 制約の種類 | 解析後データ | 生成での使用方法 |
|---|---|---|
| 数値範囲 `1<=X<=N` | `bounds["x"] = {lo:Lit(1), hi:Var("n")}` | `gen_val` で `[lo, hi]` から乱数 |
| 負の下限 `lo < 0` | `bounds["x"].lo = Lit(-n)` | `num_ty_for_base` 相当の判断で負数を生成（i64 範囲） |
| 離散集合 `X ∈ {1,2}` | `bounds["x"].hi = Set([1,2])` | Set から乱数インデックスで選択 |
| 順序制約 `x<=y` | `var_to_var: [("x","y")]` | 生成後チェック → 不満足なら再生成 |
| 不等制約 `a!=b` | `var_not_eq: [("a","b")]` | 生成後チェック → 不満足なら再生成 |
| 文字列（`からなる` あり） | `string_vars["s"] = {LowerAlpha, 1, Var("n")}` | `gen_string` で文字列生成 |
| 文字列（命名規則フォールバック） | `string_vars["s"] = {LowerAlpha, 1, Lit(100)}`（デフォルト） | `is_string_symbol` が true のとき `からなる` 未検出でも登録 |
| Sum 制約 `Nの総和≤Y` | `sum_constraints: [{inner_var:"n", limit:Y}]` + T.hi/N.hi 制限 | OuterRepeat 後に実総和チェック → ランダム: 無制限リトライ（必ず Some）、コーナーケース: 最大20回リトライ後 None でスキップ |
| SumMaxSingle (T=1, N=limit) | `CaseStrategy::SumMaxSingle { inner_var, limit }` | gen_scalar で T/Q 系=1、inner_var=limit（MAX_ARRAY_SIZE キャップ）、他=AllMax |
| OuterRepeat | `InputBlock::OuterRepeat{count,inner}` | `process_blocks` で count 回 inner を繰り返し、内側変数を `__sum_*` として外側 ctx に蓄積 |
| TypedRepeat（クエリ分岐） | `InputBlock::TypedRepeat{count,branches}` | count 回クエリ生成、各回ランダムに branch 選択して `"{type_val} {inner...}"` を出力 |
