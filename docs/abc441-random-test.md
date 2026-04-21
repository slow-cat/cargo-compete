# abc441 ランダムテスト結果

実行日: 2026-04-19 | --random 5 --no-test

## 問題 a

**結果**: AC:5/5  max: 2 ms  
**スキップ制約**: (なし)  

**考察**:
制約は `1 <= N,M <= 1e4`。corner1はN=M=10000の最大サイズ、corner2はN=M=1の最小サイズ。入力は1行2値で正しく生成されている。

**stdin例（3件）**:

ケース1 (`corner1`) — Accepted (2ms):
```
10000 10000
10000 10000
```

ケース2 (`corner2`) — Accepted (0ms):
```
1 1
1 1
```

ケース3 (`corner3`) — Accepted (0ms):
```
7010 7335
8523 6538
```

---

## 問題 b

**結果**: AC:0 RE:5 TLE:0  max: 2 ms  
**スキップ制約**: (なし)  

**考察**:
**文字列入力問題**。制約はS,T: 長さN,M の英小文字文字列、Q: クエリ数、omega_i: 長さM の英小文字文字列。入力生成は正しくS/T/omega_iが英小文字文字列として生成されている（corner1: N=M=26の最大、26文字の文字列群）。REの原因は解法テンプレートが `w: [usize; 2], w_q: usize` という古いパース結果を保持しており、文字列型を期待する入力で型ミスマッチが起きているため。入力生成自体は正常。

**stdin例（3件）**:

ケース1 (`corner1`) — Runtime Error (exit status: 101) (2ms):
```
26 26
yjxlmablshpofqembtbiaqpjjb
aapmqfwiqnbjilxeswibrdicnb
100
ripptqvrkypawncextxnwuiqgezamnuvtisuijgsualuhbwuqkywgsiutnkguosobosgwrihoikdzntxrtdoaauecuhivdkueofm
eygqohlnwgtdpuvvlcsljanppkkjysortagekbuvdcthvthtwyghkunpniwhgnfdqvqkbtieueyczhnumxnhiqzhbjvofrvwbrqh
opqrucwrmiwkbcsgjekxndvrihenhvdyxeaxrrezgnfregcbxkdsprgbfktlchulfnsxloijtudtmxgijmmnuuqyjdzkxepypkld
weqosgqzdjztddzuzxbbawzflclbayhejwvdiumsyyvcwmznqjvulichjsyeohrmhklyiwsyidjavwmsxqixgyokctusqsedbjtv
...(省略)
```

ケース2 (`corner2`) — Runtime Error (exit status: 101) (0ms):
```
1 1
k
r
1
m
```

ケース3 (`corner3`) — Runtime Error (exit status: 101) (0ms):
```
18 14
fnjqlbpeqqtsmplus
ruoxyfop
1
nhbygqvnfdcxtbowmljaphjdbnspjxmjrwskzlkrysikeqbwttafhhoekzfn
```

---

## 問題 c

**結果**: AC:2 RE:3 TLE:0  max: 140 ms  
**スキップ制約**: (なし)  

**考察**:
制約は `1 <= N,M <= 2e5`、`1 <= C <= 1e9`。corner1はN=M=200000の最大サイズで140msかかりREが出る（解法未実装）。AC2件は小規模ケース（N=M=1〜2）。

**stdin例（3件）**:

ケース1 (`corner1`) — Accepted (140ms):
```
200000 200000 1000000000
...(省略)
```

ケース2 (`corner2`) — Accepted (41ms):
```
200000 1 1
...(省略)
```

ケース3 (`corner3`) — Runtime Error (exit status: 101) (132ms):
```
300000 252211 385503058
...(省略)
```

---

## 問題 d

**結果**: AC:5/5  max: 373 ms  
**スキップ制約**: (なし)  

**考察**:
制約は `1 <= N,M <= 2e5`、`1 <= K <= 10`、`1 <= A,B <= 1e9`。corner1はN=M=200000、K=10の最大サイズで373msにACしている。

**stdin例（3件）**:

ケース1 (`corner1`) — Accepted (373ms):
```
200000 200000 10 1000000000 1000000000
200000 200000 100000000
200000 200000 100000000
200000 200000 100000000
200000 200000 100000000
200000 200000 100000000
200000 200000 100000000
200000 200000 100000000
200000 200000 100000000
200000 200000 100000000
200000 200000 100000000
200000 200000 100000000
200000 200000 100000000
200000 200000 100000000
200000 200000 100000000
200000 200000 100000000
200000 200000 100000000
200000 200000 100000000
200000 200000 100000000
200000 200000 100000000
...(省略)
```

ケース2 (`corner2`) — Accepted (0ms):
```
1 1 1 1 1
1 1 1
```

ケース3 (`corner3`) — Accepted (0ms):
```
156359 1 5 620490051 717930845
133714 151307 17457036
```

---

## 問題 e

**結果**: AC:5/5  max: 32 ms  
**スキップ制約**: (なし)  

**考察**:
制約は `1 <= N <= 5e5`。corner1はN=500000の最大サイズで32msにACしている。

**stdin例（3件）**:

ケース1 (`corner1`) — Accepted (32ms):
```
500000
...(省略)
```

ケース2 (`corner2`) — Accepted (0ms):
```
1
A
```

ケース3 (`corner3`) — Accepted (14ms):
```
267518
...(省略)
```

---

## 問題 f

**結果**: AC:5/5  max: 3 ms  
**スキップ制約**: (なし)  

**考察**:
制約は `1 <= N <= 1000`、`1 <= X <= 1e9`。corner1はN=1000、X=1e9の最大サイズで3msにACしている。

**stdin例（3件）**:

ケース1 (`corner1`) — Accepted (3ms):
```
1000 1000000000
1000000000 1000000000
1000000000 1000000000
1000000000 1000000000
1000000000 1000000000
1000000000 1000000000
1000000000 1000000000
1000000000 1000000000
1000000000 1000000000
1000000000 1000000000
1000000000 1000000000
1000000000 1000000000
1000000000 1000000000
1000000000 1000000000
1000000000 1000000000
1000000000 1000000000
1000000000 1000000000
1000000000 1000000000
1000000000 1000000000
1000000000 1000000000
1000000000 1000000000
1000000000 1000000000
1000000000 1000000000
...(省略)
```

ケース2 (`corner2`) — Accepted (0ms):
```
1 50000
1 1
```

ケース3 (`corner3`) — Accepted (0ms):
```
1 811152807
324267285 574376781
```

---

## 問題 g

**結果**: AC:1 RE:4 TLE:0  max: 2 ms  
**スキップ制約**: `タイプ 3 のクエリが存在する。`  

**考察**:
制約に「タイプ3のクエリが存在する」という条件があるが、これはパース不能としてスキップされる。それ以外の制約は正しく生成されており、REは解法未実装によるもの。

**stdin例（3件）**:

ケース1 (`corner1`) — Runtime Error (exit status: 101) (2ms):
```
200000 200000
1000000000 200000 200000 1000000000
1000000000 200000 200000
```

ケース2 (`corner2`) — Accepted (0ms):
```
200000 1
1 1 1 1
1 1 1
```

ケース3 (`corner3`) — Runtime Error (exit status: 101) (0ms):
```
200000 3806
445199935 148100 196183 17383111
15516427 121722 190063
```

---
