# abc452 ランダムテスト結果

実行日: 2026-04-19 | --random 5 --no-test

## 問題 a

**結果**: AC:5/5  max: 1 ms  
**スキップ制約**: (なし)  

**考察**:
全ケースACした（クラッシュ・TLEなし）。解法未実装のため空出力だが、入力生成は正常に機能している。

**stdin例（3件）**:

ケース1 (`corner1`) — Accepted (1ms):
```
1000000000 1000000000
```

ケース2 (`corner2`) — Accepted (0ms):
```
1 1
```

ケース3 (`corner3`) — Accepted (0ms):
```
410010256 98135269
```

---

## 問題 b

**結果**: AC:5/5  max: 1 ms  
**スキップ制約**: (なし)  

**考察**:
全ケースACした（クラッシュ・TLEなし）。解法未実装のため空出力だが、入力生成は正常に機能している。

**stdin例（3件）**:

ケース1 (`corner1`) — Accepted (1ms):
```
10 10
```

ケース2 (`corner2`) — Accepted (0ms):
```
3 3
```

ケース3 (`corner3`) — Accepted (0ms):
```
5 9
```

---

## 問題 c

**結果**: AC:1 RE:4 TLE:0  max: 2 ms  
**スキップ制約**: (なし)  

**考察**:
制約は `1 <= N <= 10`。corner1はN=10の最大。AC1件（corner2=最小N=1）、RE4件。解法未実装。

**stdin例（3件）**:

ケース1 (`corner1`) — Runtime Error (exit status: 101) (2ms):
```
10
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
1000000000
vrjrzhnyekmraffjwyvgbirxvnhhstdolekcexdeeoqoqcaqdlscgiaukbldpgoxnmgtdbnvzrjbikxagcwratwtydajzfxozkcn
moeblrprbesxiuiolzqjkblzihwlzrgbkmbfburjjsrrteardwwltszvmtooxknlrfxoinqgmdcylykwfazwxwuzjtorqrfteowp
```

ケース2 (`corner2`) — Accepted (0ms):
```
1
1000000000 1
1
d
z
```

ケース3 (`corner3`) — Runtime Error (exit status: 101) (0ms):
```
1
1000000000 464420007
39493434
luzpqhccryktzvridsejlaegnqkwwfzdlmprwpoaxpymgvstlfwuwzwdnsnwkttiexanprtktnxrnksjhpranccayaxk
yanixfwbefvnvksxsoqbeehctfnhxhyjp
```

---

## 問題 d

**結果**: AC:5/5  max: 13 ms  
**スキップ制約**: (なし)  

**考察**:
**文字列入力問題（|S|,|T|制約）**。制約に `|S| を S の長さとして 1 \le |S| \le 2×10^5`、`|T| を T の長さとして 1 \le |T| \le 50`。修正前は両制約をスキップし警告を出していたが、修正によりS（最大2e5文字）・T（最大50文字）が英小文字文字列として正しく生成されている。corner1では約200KB相当の長文字列Sと適切な長さのTが生成され、全5件AC（旧バージョンでは警告あり、現在は警告なし）。

**stdin例（3件）**:

ケース1 (`corner1`) — Accepted (13ms):
```
...(省略)
```

ケース2 (`corner2`) — Accepted (0ms):
```
g
e
```

ケース3 (`corner3`) — Accepted (9ms):
```
...(省略)
```

---

## 問題 e

**結果**: AC:5/5  max: 191 ms  
**スキップ制約**: (なし)  

**考察**:
全ケースACした（クラッシュ・TLEなし）。解法未実装のため空出力だが、入力生成は正常に機能している。

**stdin例（3件）**:

ケース1 (`corner1`) — Accepted (191ms):
```
200000 200000
...(省略)
```

ケース2 (`corner2`) — Accepted (1ms):
```
1 1
1
1
```

ケース3 (`corner3`) — Accepted (0ms):
```
1 1
172652
137422
```

---

## 問題 f

**結果**: AC:5/5  max: 137 ms  
**スキップ制約**: `0\le K\le\dfrac{N(N-1)}2`  

**考察**:
制約に `0 <= K <= N(N-1)/2` があり、dfrac形式の複雑な制約としてスキップされる。それ以外は正常に生成されており全5件AC。

**stdin例（3件）**:

ケース1 (`corner1`) — Accepted (137ms):
```
200000 1000000000
...(省略)
```

ケース2 (`corner2`) — Accepted (0ms):
```
1 1
1
```

ケース3 (`corner3`) — Accepted (0ms):
```
1 676601716
979331091
```

---

## 問題 g

**結果**: AC:5/5  max: 40 ms  
**スキップ制約**: (なし)  

**考察**:
全ケースACした（クラッシュ・TLEなし）。解法未実装のため空出力だが、入力生成は正常に機能している。

**stdin例（3件）**:

ケース1 (`corner1`) — Accepted (40ms):
```
200000
...(省略)
```

ケース2 (`corner2`) — Accepted (0ms):
```
1
1
```

ケース3 (`corner3`) — Accepted (0ms):
```
1
5
```

---
