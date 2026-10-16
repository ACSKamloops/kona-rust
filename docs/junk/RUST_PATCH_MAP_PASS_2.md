# RUST PATCH MAP PASS 2

## Executive Summary
This document defines a precise, phased implementation roadmap for aligning the Rust port of Kona with confirmed behavior gaps in the upstream C Kona interpreter (`kona-master3`). The recommended patch order follows a logical dependency sequence, highest-impact and lowest-level mathematical abstractions first, ensuring a stable foundation before resolving parser, evaluation, and adverb gaps.

### Recommended Patch Order:
1. **Patch 1: Float Compatibility and Sentinel Promotions** (Fundamental math layer; blocks all comparisons).
2. **Patch 2: Mixed-Type Ordering and Length-First List Comparison** (Sorting layer; blocks all list comparisons).
3. **Patch 3: Parser and Monadic Suffix Verbs** (Parser/evaluator layer; fixes assignment vs. monadic distinction).
4. **Patch 4: Adverbs scan and Dot-Trap Argument Collapsing** (Evaluation layer; resolves valence and scan indexing errors).
5. **Patch 5: Exact bitwise Unique/Group Hashing and Dictionary Context evaluation** (Data structure layer; completes unique/group/find exactness and dictionary namespace lookups).

---

## Summary Table

| Behavior | C Oracle Status | Rust File/Function | Current Rust Problem | Minimal Patch | Validation |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Float Tolerance (`FC`)** | Confirmed (ULP < 513) | [src/types.rs:float_eq](file:///home/juker/Desktop/Kona%20rust/src/types.rs#L243-L251) | Uses static epsilon `1e-13` which fails boundary comparisons. | Replace static check with bitwise adaptive ULP comparison matching `FC`. | `cargo test --bin %TESTS_BIN% test_c_suite_comparison` |
| **Mixed List Sorting (`KC`)** | Confirmed type code hierarchy | [src/verbs/list.rs:cmp_k](file:///home/juker/Desktop/Kona%20rust/src/verbs/list.rs#L161-L224) | Groups all atoms before lists, violating type sorting code. | Implement a `c_type_code` function and sort list items strictly by hierarchy. | `cargo test verbs::list::test_c_kona_compat_sorting` |
| **Length-First Comparison** | Confirmed for non-char lists | [src/verbs/list.rs:cmp_k](file:///home/juker/Desktop/Kona%20rust/src/verbs/list.rs#L161-L224) | Compares lists element-by-element lexicographically first. | If lists are non-character lists, compare their lengths first before items. | `cargo test verbs::list::test_c_kona_compat_sorting` |
| **NaN Radix Grading Order** | Confirmed NaNs sort first | [src/verbs/list.rs:cmp_k](file:///home/juker/Desktop/Kona%20rust/src/verbs/list.rs#L161-L224) | `partial_cmp` returns `None` for NaNs, causing unstable order. | Map NaNs to sort before all other floats during float grading comparisons. | `cargo test verbs::list::test_c_kona_compat_sorting` |
| **Float Unique (`?`) and Group (`=`)** | Confirmed exact 64-bit comparison | [src/verbs/list.rs:group](file:///home/juker/Desktop/Kona%20rust/src/verbs/list.rs#L240-L261) / `distinct` | Uses tolerant comparisons, merging distinct close float elements. | Introduce a wrapper `ExactK` performing exact bitwise hashing and comparisons. | `cargo test verbs::list::test_c_kona_compat_unique_group` |
| **Exact vs. Tolerant Find (`?`)** | Confirmed linear search tolerant; hashed exact | [src/verbs/list.rs:find](file:///home/juker/Desktop/Kona%20rust/src/verbs/list.rs#L608-L622) | Does not distinguish exact bitwise hash-find from linear find. | Use exact bit-matching for hashed lookup while keeping linear search tolerant. | `cargo test verbs::list::test_c_kona_compat_find` |
| **Monadic Suffix `:`** | Confirmed monadic verbs end in `:` | [src/eval.rs:eval_words_flow](file:///home/juker/Desktop/Kona%20rust/src/eval.rs#L999-L1212) | Evaluates all verbs ending in `:` as in-place assignments. | Only treat suffix `:` as assignment if LHS is a valid name or bracket target. | `cargo test --lib c_tests` |
| **Matrix Scan Indexing** | Confirmed dyadic indexing scan | [src/adverbs.rs:can_be_monadic](file:///home/juker/Desktop/Kona%20rust/src/adverbs.rs#L130-L153) | Treats lists and matrices as monadic, causing valence errors. | Modify `can_be_monadic` to return `false` for matrices, lists, and dicts. | `cargo test adverbs::additional_tests::test_matrix_scan_indexing` |
| **Dot-Trap Arg Collapse** | Confirmed collapses char-vectors | [src/eval.rs:apply_func](file:///home/juker/Desktop/Kona%20rust/src/eval.rs#L1444-L1453) | Fails with valence error when unpacking char strings. | If first argument is char atom, collapse arguments back to string. | `cargo test eval::tests::test_dot_trap_char_collapse` |
| **Namespace Dictionary Eval** | Confirmed string eval in local scope | [src/eval.rs:apply_index_chain](file:///home/juker/Desktop/Kona%20rust/src/eval.rs#L1605-L1656) | Evaluates dict string lookups in global context. | Build a temporary environment of keys and values for dictionary string evaluation. | `cargo test eval::tests::test_dict_string_evaluation` |
| **Null Promotion (`I2F`)** | Confirmed `0N` promotes to `0n` | [src/verbs/math.rs:apply_op_atoms](file:///home/juker/Desktop/Kona%20rust/src/verbs/math.rs#L470-L524) | Casts `i64::MIN` via `as f64`, yielding infinity or valid float. | Use a dedicated `i2f` helper mapping `i64::MIN` directly to `f64::NAN`. | `cargo test` |

---

## Agent Findings

### 1. numeric_ordering_agent
* **Observed Facts**:
  * Upstream C Kona compares float equality based on adaptive Unit in the Last Place (ULP) differences (the `FC` macro in `src/kg.c:18-57`), evaluating float values within 513-1021 ULPs of each other as equal.
  * Float comparisons in Rust utilize static/relative epsilon check (`(x - y).abs() <= 1e-13 * x.abs().max(y.abs())` in `src/types.rs:float_eq`).
  * The integer Null sentinel `0N` (`i64::MIN`) is cast directly via `as f64` in `verbs/math.rs`, producing valid float `-9.22e18` instead of float Null `f64::NAN`.
  * Mixed type ordering (`KC` macro in `src/kg.c:59`) sorts strictly by type code hierarchy: `SymbolVector (-4) < CharVector (-3) < FloatVector (-2) < IntVector (-1) < GeneralList (0) < IntAtom (1) < FloatAtom (2) < CharAtom (3) < SymbolAtom (4) < Dict (5) < Nil (6) < Func (7)`.
* **Proposed Changes**:
  * Replace static epsilon check in [src/types.rs:float_eq](file:///home/juker/Desktop/Kona%20rust/src/types.rs#L243-L251) with adaptive bitwise ULP float comparison.
  * Implement `fc_compare` in `types.rs` that maps NaNs to sort before all other floats (returning `Ordering::Less`).
  * Replace raw `as f64` casts of integer atoms with a helper `i2f` function in `src/verbs/math.rs` that explicitly checks for `i64::MIN` and returns `f64::NAN`.

### 2. grade_hash_agent
* **Observed Facts**:
  * Monadic unique `?` and group `=` on float vectors cast float memory to raw 64-bit unsigned integers, performing exact bitwise hashing and comparisons (`intRange` and `intGroup` in `src/vg.c:120/289`).
  * Dyadic find `x ? y` performs tolerant float comparison. Hashed find `(x; _hash x) ? y` uses exact bitwise comparison.
  * Rust uses tolerant float comparison for both, causing close (but bitwise distinct) floats to incorrectly group together.
* **Proposed Changes**:
  * In [src/verbs/list.rs:cmp_k](file:///home/juker/Desktop/Kona%20rust/src/verbs/list.rs#L161-L224), implement direct comparison based on `c_type_code` and ULP-based `fc_compare`.
  * In `cmp_k`, non-character vectors and lists must compare lengths first: `if len_a != len_b { return len_a.cmp(&len_b); }`.
  * Define `ExactK` wrapper struct in `src/verbs/list.rs` that implements `PartialEq`, `Eq`, and `Hash` using exact bit-patterns for float elements. Use this wrapper in `group` and `distinct` collections to ensure bitwise float unique/group behavior.

### 3. parser_suffix_agent
* **Observed Facts**:
  * Monadic suffix verbs end in `:` (e.g. `*:`, `+:`, `~:`, `-:`). When not applied to a variable LHS, they evaluate monadically. When applied to a variable, they operate as in-place assignments (dyadic like `a+:1` or monadic like `a-:` / `a[0]-:`).
  * The Rust evaluator [src/eval.rs:eval_words_flow](file:///home/juker/Desktop/Kona%20rust/src/eval.rs#L999-L1212) incorrectly treats all verbs ending in `:` as in-place assignments, failing with `type` errors when used monadically without a LHS target, or failing during monadic assignments without RHS.
* **Proposed Changes**:
  * Adjust `eval_words_flow` match branch for `Word::Verb(name)` to verify if the preceding element is a valid assignment target (`Word::Name` or `Word::Bracket` indexed name).
  * If it has a target, evaluate it as in-place assignment. If no RHS (accum) exists, evaluate it as monadic in-place assignment (e.g. `a-:`).
  * If it lacks a target, evaluate it strictly as a monadic verb projection (e.g. `*:0`).

### 4. adverb_trap_dict_agent
* **Observed Facts**:
  * Matrix scans (e.g. `m\[1;2]`) act as dyadic indexing scans. Rust's `can_be_monadic` in `src/adverbs.rs` returns `true` for general lists, incorrectly routing it to monadic scans.
  * In dot-trap `.[f; x; :]` with monadic char function `f` and char string `x`, C Kona collapses unpacked char atoms back to a single string to avoid valence errors. Rust checks valence on unpacked arguments and errors.
  * Dict lookup with string evaluations (e.g. `d["c+1"]`) executes the expression in the dictionary namespace context. Rust evaluations occur in the global scope.
* **Proposed Changes**:
  * Update `can_be_monadic` in `src/adverbs.rs` to return `false` for Lists, Vectors, and Dictionaries.
  * In [src/eval.rs:apply_func](file:///home/juker/Desktop/Kona%20rust/src/eval.rs#L1444-L1453), detect if arguments exceed parameters on user-defined functions; if they are all char atoms, collapse them back to a single `CharList` (string) before performing valence checks.
  * In `apply_index_chain` and `apply_verb` of `src/eval.rs`, when indexing a dictionary with a string argument, parse the string and evaluate it using a temporary environment containing dictionary keys and values.

### 5. test_regression_agent
* **Observed Facts**:
  * Rust port fails **149 out of 1207** test cases in the `tests.c` suite comparison.
  * Mismatches on float tolerances account for 82 of these failures (e.g., lines 779-782, 756).
  * Parser and monadic suffix problems account for 38 failures (e.g., assignments like `z-:`, and evaluations like `a@*:0`).
  * Indexing and adverbs scans account for 29 failures.
* **Proposed Changes**:
  * Leverage `cargo test --bin kona test_c_suite_comparison` to validate C compatibility.
  * Ensure unit tests are appended to `src/verbs/list.rs` and `src/eval.rs` to isolate each behavioral gap before running the full C suite.

---

## Patch Sequence

### Patch 1: Float Compatibility and Sentinel Promotions
* **Files touched**:
  * [src/types.rs](file:///home/juker/Desktop/Kona%20rust/src/types.rs)
  * [src/verbs/math.rs](file:///home/juker/Desktop/Kona%20rust/src/verbs/math.rs)
* **Functions touched**:
  * `src/types.rs`: `float_eq`, `compare_numeric` (re-route through `fc_compare`), `fc_compare` [NEW], `c_type_code` [NEW].
  * `src/verbs/math.rs`: `reciprocal`, `sqrt_verb`, `apply_op_atoms` (mixed types promotion).
* **Exact behavior target**:
  * Replace epsilon check with adaptive ULP comparison in `float_eq`.
  * Ensure integer Null promotion maps `i64::MIN` directly to `f64::NAN`.
* **Tests to run**:
  * `cargo test`
  * `cargo test --bin kona test_c_suite_comparison` (verifying tests.c lines 756, 779-782).
* **Regression risk**: Low. Essential for overall comparison logic; minimal risk if exact bitwise hashing is preserved for `Hash` and `PartialEq` implementations of `KData` in `types.rs`.

### Patch 2: Mixed-Type Ordering and Length-First List Comparison
* **Files touched**:
  * [src/verbs/list.rs](file:///home/juker/Desktop/Kona%20rust/src/verbs/list.rs)
* **Functions touched**:
  * `src/verbs/list.rs`: `cmp_k`, `numeric_tolerant_eq`.
* **Exact behavior target**:
  * Implement sorting/grading comparison matching the KC hierarchy (using `c_type_code`).
  * Check lengths of lists/vectors first (except character vectors) during ordering.
  * Establish stable ordering for NaNs (grade NaNs first).
* **Tests to run**:
  * `cargo test verbs::list::test_c_kona_compat_sorting`
* **Regression risk**: Medium. Changes sorting order of mixed lists. Must ensure existing test cases relying on lexicographical sort of vectors are not broken.

### Patch 3: Parser and Monadic Suffix Verbs
* **Files touched**:
  * [src/eval.rs](file:///home/juker/Desktop/Kona%20rust/src/eval.rs)
* **Functions touched**:
  * `src/eval.rs`: `eval_words_flow` (under `Word::Verb` arm).
* **Exact behavior target**:
  * Correctly classify and evaluate monadic suffix colons (like `*:`, `+:`) as monadic projections when they are not preceded by variables.
  * Correctly support monadic in-place assignments (e.g. `a-:`).
* **Tests to run**:
  * `cargo test --lib c_tests` (specifically targets suffix assignment tests).
* **Regression risk**: Medium. Modifies the parser evaluator flow; must ensure nested assignments and dyadic in-place assignments (e.g. `a+:1`) continue to function without regression.

### Patch 4: Matrix Scan and Dot-Trap Collapsing
* **Files touched**:
  * [src/adverbs.rs](file:///home/juker/Desktop/Kona%20rust/src/adverbs.rs)
  * [src/eval.rs](file:///home/juker/Desktop/Kona%20rust/src/eval.rs)
* **Functions touched**:
  * `src/adverbs.rs`: `can_be_monadic`.
  * `src/eval.rs`: `apply_func`.
* **Exact behavior target**:
  * Matrix scans route to dyadic scan indexing by disabling monadic scan resolution.
  * String arguments in dot-traps collapse back to a single argument on UserDefined functions.
* **Tests to run**:
  * `cargo test adverbs::additional_tests::test_matrix_scan_indexing`
  * `cargo test eval::tests::test_dot_trap_char_collapse`
* **Regression risk**: Low. Restricted to list adverbs scan checks and valence errors fallback collapsing.

### Patch 5: Exact Unique/Group Hashing and Dictionary Context Eval
* **Files touched**:
  * [src/verbs/list.rs](file:///home/juker/Desktop/Kona%20rust/src/verbs/list.rs)
  * [src/eval.rs](file:///home/juker/Desktop/Kona%20rust/src/eval.rs)
* **Functions touched**:
  * `src/verbs/list.rs`: `group`, `distinct`.
  * `src/eval.rs`: `apply_index_chain`, `apply_verb` (dyadic `@` dictionary indexing).
* **Exact behavior target**:
  * Unique `?` and group `=` utilize exact 64-bit bitwise float equality.
  * Dictionary string lookups evaluate within the dictionary context.
* **Tests to run**:
  * `cargo test verbs::list::test_c_kona_compat_unique_group`
  * `cargo test verbs::list::test_c_kona_compat_find`
  * `cargo test eval::tests::test_dict_string_evaluation`
* **Regression risk**: Low. Affects exact bitwise uniqueness and string-dictionary namespace lookups.

---

## Do Not Touch

1. **`src/parser.rs`**:
   * Lexer and parser invariants are correct. The suffix `:` is already successfully parsed as a verb token (`Word::Verb("*:")`) by `combine_monadic_verbs`. Modifications here would degrade tokenizer stability.
2. **`KData` PartialEq / Hash Invariants in `src/types.rs`**:
   * The core `PartialEq` and `Hash` implementations for `KData` (which compare raw 64-bit float bits) **must not** be modified to use tolerant ULP comparison. If changed, standard Rust hash maps and sets (used globally in the compiler and state environments) would violate Rust's collection invariants. Exact unique/group behaviors must instead be handled via a custom local wrapper `ExactK` within `src/verbs/list.rs`.
3. **Reshape Overflow guarding**:
   * Rust's safe overflow bounds panics on `take` overflow checks (e.g. `0N#1` bounds checking) must remain safe. We must not attempt to reproduce C Kona's memory pool corruption or segfault behavior.

---

## Open Questions
* *None*. The subagent research passes have successfully resolved all identified gaps and verified exact C source code logic maps against the Kona oracle. The implementation path is clear.
