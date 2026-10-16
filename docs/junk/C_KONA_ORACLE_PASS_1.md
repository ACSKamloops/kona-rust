# C KONA ORACLE PASS 1

## Summary Table

| Behavior | Status | C Source | K Repro | C Output | Rust Risk |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Float Tolerance (`FC`)** | Confirmed | `src/kg.c:18-57` | `1.0 ~ 1.0 + 1.13e-13`<br>`1.0 ~ 1.0 + 1.14e-13` | `1`<br>`0` | **High**: Rust uses static epsilon `1e-13` or similar. |
| **Mixed List Total Order (`KC`)** | Confirmed | `src/kg.c:59-66` | `< (3; 1 2)` | `1 0` | **High**: Rust sorts atoms before vectors/lists. |
| **Length-First Comparison** | Confirmed | `src/kg.c:67-70` | `< (20 20; 1 1 1)` | `0 1` | **High**: Rust sorts lexicographically first. |
| **NaN Radix Grading Order** | Confirmed | `src/vg.c:24` | `< (1.0; 0n; -2.0)` | `1 2 0` | **High**: Rust `partial_cmp` does not grade NaNs first. |
| **Float unique (`?`) and group (`=`)** | Confirmed | `src/vg.c:120/289` | `= (1.0; 1.0 + 1e-15)` | `(,0\n ,1)` | **High**: Rust uses tolerant comparison, merging elements. |
| **Exact vs Tolerant Find (`?`)** | Confirmed | `src/vq.c:16` vs `src/vg.c:673` | `x:(1.0; 1.0+1e-15)`<br>`x ? 1.0 + 1e-15`<br>`(x; _hash x) ? 1.0 + 1e-15` | <br>`0`<br>`1` | **High**: Rust does not support exact bitwise hash-find. |
| **Function Valence Constraints** | Confirmed | `src/kx.c:433-440` | `{x}[1;2;3]` | `valence error` | **Low**: Rust implements standard valence checking. |
| **Matrix Scan Indexing** | Confirmed | `src/kx.c:147` | `m:3 4#!12; m\\[1;2]` | `6` | **High**: Rust incorrectly treats matrix scan as monadic. |
| **Dot-Trap Arg Collapse** | Confirmed | `src/kx.c:433-441` | `.[{x}; "2+2"; :]` | `(0\n "2+2")` | **High**: Rust throws `(1; "valence")` error. |
| **Dot-Trap Error Wrapper** | Confirmed | `src/vd.c:254` | `.[{x}; "2+2"; :]` | `(0\n "2+2")` | **High**: Rust lacks collapses on char vector arguments. |
| **Tetradic `@` Delegation** | Confirmed | `src/vd.c:281` | `@[0 1 2; 1; +; 10]` | `0 11 2` | **Low**: Rust enlists indexing appropriately. |
| **Dictionary Entry Pointer Check** | Confirmed | `src/v.c:38` | `d:\`a\`b!1 2; d\`a` | `1` | **Low**: Rust matches symbol equality correctly. |
| **Namespace Eval in Dict** | Confirmed | `src/v.c:131` | `c:2; d.c:42; d["c+1"]` | `43` | **High**: Rust evaluates in wrong context (yielding `3`). |
| **Numeric Sentinels** | Confirmed | `src/ts.h:53-56` | `0N` | `-9223372036854775808`| **Medium**: Rust must parse `0N` to `i64::MIN`. |
| **Null Promotion (`I2F`)** | Confirmed | `src/ts.h:57` | `0i+0N` | `0n` | **High**: Rust casts `i64::MIN` to float, yielding `0i`. |
| **Reshape Overflow (`0N#1`)** | Confirmed | `src/vg.c:443`<br>`src/km.c:136/126` | `0N#1` | `Segmentation fault` | **Low**: Rust panics safely without memory corruption. |

---

## Findings

### 1. `src/kg.c`

#### Float Comparison (`FC`)
* **Claim**: Floating-point comparison uses adaptive ULP differences ($<513$), making float values within $1.1368 \times 10^{-13}$ of $1.0$ equal.
* **Evidence**: `src/kg.c:18-57`.
* **K Repro**:
  ```k
  1.0 ~ 1.0 + 1.13e-13
  1.0 ~ 1.0 + 1.14e-13
  ```
* **C Output**:
  ```k
  1
  0
  ```
* **Status**: Confirmed.
* **Rust Risk**: **High**. The Rust port uses a static epsilon comparison (`(x - y).abs() < 1e-13`), causing mismatches near boundaries.

#### Mixed List Total Ordering (`KC`)
* **Claim**: Sorting and grading on mixed lists uses the internal C type code hierarchy.
* **Evidence**: `src/kg.c:59-66`.
* **K Repro**:
  ```k
  < (3; 1 2)
  ```
* **C Output**:
  ```k
  1 0
  ```
* **Status**: Confirmed.
* **Rust Risk**: **High**. Rust groups all atoms before vectors/lists in `cmp_k` sorting.

#### Length-First List Comparison
* **Claim**: Non-character vectors and lists are compared by length first.
* **Evidence**: `src/kg.c:67-70`.
* **K Repro**:
  ```k
  < (20 20; 1 1 1)
  ```
* **C Output**:
  ```k
  0 1
  ```
* **Status**: Confirmed.
* **Rust Risk**: **High**. The Rust port compares numeric vectors element-by-element lexicographically first.

---

### 2. `src/vg.c`

#### Radix/Merge Grade Behavior
* **Claim**: Grading functions (`<` and `>`) delegate to `grade_updown` utilizing `KC` comparisons.
* **Evidence**: `src/vg.c:27` (`grade_updown`).
* **K Repro**:
  ```k
  < (20 20; 1 1 1)
  ```
* **C Output**:
  ```k
  0 1
  ```
* **Status**: Confirmed.
* **Rust Risk**: **High**. Handled via Phase 2 total ordering changes.

#### NaN Ordering
* **Claim**: Floating-point radix sorting maps `isnan(a)` to `LLONG_MIN`, causing NaNs to sort before all other floats.
* **Evidence**: `src/vg.c:24` (`FtoI`).
* **K Repro**:
  ```k
  < (1.0; 0n; -2.0)
  ```
* **C Output**:
  ```k
  1 2 0
  ```
* **Status**: Confirmed.
* **Rust Risk**: **High**. Rust lacks a total order for NaNs, causing sorting failures in lists containing `0n`.

#### Unique and Group Hashing
* **Claim**: Monadic unique `?` and group `=` on float vectors cast float memory to raw 64-bit unsigned integers (`uI`), performing exact bitwise hashing and comparisons.
* **Evidence**: `src/vg.c:120` (`intRange`) and `src/vg.c:289` (`intGroup`), both using the pointer cast `#define kU(x) ((uI*)((x)->k))`.
* **K Repro**:
  ```k
  = (1.0; 1.0 + 1e-15)
  ```
* **C Output**:
  ```k
  (,0
   ,1)
  ```
* **Status**: Confirmed.
* **Rust Risk**: **High**. Rust uses tolerant float comparisons for unique/group, grouping bitwise distinct elements.

#### Exact vs Tolerant Find (`?`)
* **Claim**: Find `x ? y` performs tolerant float comparison. However, hashed find `(x; _hash x) ? y` uses exact bitwise comparison.
* **Evidence**: `src/vq.c:16` (linear search via `FC`) vs `src/vg.c:673` (`hash_find` via `_hg` exact bitwise comparison).
* **K Repro**:
  ```k
  x:(1.0; 1.0 + 1e-15)
  x ? 1.0 + 1e-15
  (x; _hash x) ? 1.0 + 1e-15
  ```
* **C Output**:
  ```k
  0
  1
  ```
* **Status**: Confirmed.
* **Rust Risk**: **High**. Rust does not support exact bitwise hash-find lookups.

---

### 3. `src/kx.c`

#### Function Application and Valence Rules
* **Claim**: User-defined functions execute valence checks during execution, raising valence error on extra arguments.
* **Evidence**: `src/kx.c:433-440` (`vf_ex`).
* **K Repro**:
  ```k
  {x}[1;2;3]
  ```
* **C Output**:
  ```k
  valence error
  ```
* **Status**: Confirmed.
* **Rust Risk**: **Low**. Rust mirrors this behavior.

#### Matrix Scan Indexing
* **Claim**: A matrix left-hand operand with scan `\` (e.g. `m\[1;2]`) acts as a dyadic indexing function.
* **Evidence**: `src/kx.c:147` (`scanDyad`).
* **K Repro**:
  ```k
  m:3 4#!12; m\\[1;2]
  ```
* **C Output**:
  ```k
  6
  ```
* **Status**: Confirmed.
* **Rust Risk**: **High**. Rust port incorrectly treats matrix scans as monadic.

#### Dot-Trap Argument Collapse
* **Claim**: If a character function `f` is monadic (`valence == 1`) and the argument `x` is a char vector (e.g., `"2+2"`), C Kona's evaluator promotes `"2+2"` to `("2"; "+"; "2")` (length 3). Because length (3) exceeds valence (1), it enters collapse check: since `kK(g)[0]->t == 3` (char atom) and `f->n == 3` (char function), it collapses it back to `"2+2"` and passes it as a single argument.
* **Evidence**: `src/kx.c:433-441` (`vf_ex`).
* **K Repro**:
  ```k
  .[{x}; "2+2"; :]
  ```
* **C Output**:
  ```k
  (0
   "2+2")
  ```
* **Status**: Confirmed.
* **Rust Risk**: **High**. Rust returns `(1; "valence")` due to lack of collapse logic.

---

### 4. `src/vd.c`

#### Dot/Amend/Trap Behavior
* **Claim**: Error trap `.[f; x; :]` executes `f` via `vf_ex(&f, x)`.
* **Evidence**: `src/vd.c:254` (`dot_tetradic`).
* **K Repro**:
  ```k
  .[{x}; "2+2"; :]
  ```
* **C Output**:
  ```k
  (0
   "2+2")
  ```
* **Status**: Confirmed.
* **Rust Risk**: **High**. Handled via Phase 5 trap changes.

#### Tetradic `@` Behavior
* **Claim**: `@` tetradic amend `@[x; i; f; y]` enlists `i` and delegates to `.` tetradic amend `.[x; ,i; f; y]`.
* **Evidence**: `src/vd.c:281` (`at_tetradic`).
* **K Repro**:
  ```k
  @[0 1 2; 1; +; 10]
  ```
* **C Output**:
  ```k
  0 11 2
  ```
* **Status**: Confirmed.
* **Rust Risk**: **Low**. Rust mirrors this delegation.

---

### 5. `src/v.c`

#### Dictionary Lookup
* **Claim**: Dictionary lookups compare interned symbol keys directly via pointer equality `==`.
* **Evidence**: `src/v.c:38` (`DE`).
* **K Repro**:
  ```k
  d:`a`b!1 2; d`a
  ```
* **C Output**:
  ```k
  1
  ```
* **Status**: Confirmed.
* **Rust Risk**: **Low**. Rust port matches symbol comparison.

#### String Evaluation in Dictionary Namespace
* **Claim**: Dictionary indexing with a string compiles and evaluates the string in the dictionary's namespace context by passing a pointer to the dictionary `&a` to `wd_`.
* **Evidence**: `src/v.c:131` (`at_verb`).
* **K Repro**:
  ```k
  c:2; d.c:42; d["c+1"]
  ```
* **C Output**:
  ```k
  43
  ```
* **Status**: Confirmed.
* **Rust Risk**: **High**. Rust evaluates `"c+1"` globally, yielding `3`.

---

### 6. `src/ts.h`

#### Numeric Sentinels
* **Claim**: Sentinels represent K limits: `II` (integer infinity, `LLONG_MAX`), `IN` (integer null, `LLONG_MIN`), `FI` (float infinity, `1/0.`), and `FN` (float null, `0/0.`).
* **Evidence**: `src/ts.h:53-56`.
* **K Repro**:
  ```k
  0N
  ```
* **C Output**:
  ```k
  -9223372036854775808
  ```
* **Status**: Confirmed.
* **Rust Risk**: **Medium**.

#### Null Promotion Behavior (`I2F`)
* **Claim**: Promoted integer Null `IN` evaluates to float Null `FN` (not float infinity).
* **Evidence**: `src/ts.h:57` (`#define I2F(x) (II==(x)?FI:-II==(x)?-FI:IN==(x)?FN:(x))`).
* **K Repro**:
  ```k
  0i+0N
  ```
* **C Output**:
  ```k
  0n
  ```
* **Status**: Confirmed.
* **Rust Risk**: **High**. Rust casts `i64::MIN` to float, yielding infinity (`0i`) instead of Null (`0n`).

#### Reshape Overflow Segfault (`0N#1`)
* **Claim**: `0N#1` bypasses `newK` overflow checks because `n = LLONG_MIN` is negative. `sz(t, n)` overflows: `LLONG_MIN * 8` wraps to `0`, causing `newK` to allocate a 24-byte block while setting size `n` to `LLONG_MIN`. Subsequent iteration/printing segfaults.
* **Evidence**: `src/vg.c:443` (`take`), `src/km.c:136` (`newK`), and `src/km.c:126` (`sz` calculation).
* **K Repro**:
  ```k
  0N#1
  ```
* **C Output**:
  ```k
  Segmentation fault (core dumped)
  ```
* **Status**: Confirmed.
* **Rust Risk**: **Low**. Rust panics safely instead of segfaulting.

---

## Unresolved Questions

None. All 16 behaviors mapped to targeted C files have been successfully validated and replicated on the C Kona oracle.

---

## Recommended Next Oracle Pass

If further precision is required, a second pass should target:
1. **Adverb Composition**: Tracing how `over` `/` and `scan` `\` compile multi-verb compositions in `src/kx.c:321`.
2. **Namespace Context Execution**: Tracing path creation logic inside `denameRecurse` in `src/v.c:64`.
3. **Format/String Representation**: Tracing float formatting limits in `src/c.c`.
