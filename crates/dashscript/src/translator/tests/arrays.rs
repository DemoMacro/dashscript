use super::super::Translator;

#[test]
fn translates_array_literal_to_vec_macro() {
    let src = "function main(): void { const xs: number[] = [1, 2, 3]; console.log(xs.length); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("vec![1_f64, 2_f64, 3_f64]"), "got:\n{rust}");
}

#[test]
fn translates_array_index() {
    let src = "function f(): void { const xs: number[] = [1, 2]; console.log(xs[0]); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("xs[0_f64 as usize]"), "got:\n{rust}");
}

#[test]
fn translates_in_operator_on_array_to_index_bound() {
    let src = "function f(xs: number[], i: number): boolean { return i in xs; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("< xs.len()"), "got:\n{rust}");
}

#[test]
fn translates_array_map_to_iter_copied_map_collect() {
    let src =
        "function f(): void { const xs: number[] = [1, 2, 3]; const ys = xs.map(n => n * 2); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("xs.iter().copied().map(|n| n * 2_f64).collect::<Vec<_>>()"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_array_filter_to_iter_copied_filter_collect() {
    let src =
        "function f(): void { const xs: number[] = [1, 2, 3]; const ys = xs.filter(n => n > 1); }";
    let rust = Translator::new().translate(src).expect("should translate");
    // `.filter`'s closure receives &Item after `.copied()`, so the param is
    // destructured (`|&n|`) and the body reads the owned value.
    assert!(
        rust.contains("xs.iter().copied().filter(|&n| n > 1_f64).collect::<Vec<_>>()"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_array_slice_to_index_range_to_vec() {
    let src = "function f(): void { const xs: number[] = [1, 2, 3, 4]; const ys = xs.slice(1, 3); const zs = xs.slice(2); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("xs[1_f64 as usize..3_f64 as usize].to_vec()"),
        "got:\n{rust}"
    );
    assert!(
        rust.contains("xs[2_f64 as usize..].to_vec()"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_array_index_of_to_position() {
    let src = "function f(): void { const xs: number[] = [1, 2, 3]; const i = xs.indexOf(2); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".position(|y| y == 2_f64)"), "got:\n{rust}");
    assert!(rust.contains(".unwrap_or(-1_f64)"), "got:\n{rust}");
}

#[test]
fn translates_array_find_index_to_position() {
    let src = "function f(): void { const xs: number[] = [1, 2, 3]; const i = xs.findIndex((n) => n > 1); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".position(|n| n > 1_f64)"), "got:\n{rust}");
    assert!(rust.contains(".unwrap_or(-1_f64)"), "got:\n{rust}");
}

#[test]
fn translates_array_at_to_signed_runtime_index() {
    let src = "function f(xs: number[], i: number): number { return xs.at(i); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("__at_i >= 0_f64"), "got:\n{rust}");
    assert!(rust.contains("len() as f64 + __at_i"), "got:\n{rust}");
}

#[test]
fn translates_array_flat_map_to_flat_map_collect() {
    let src =
        "function f(): void { const xs: number[] = [1, 2]; const ys = xs.flatMap((n) => [n, n]); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".flat_map(|n|"), "got:\n{rust}");
    assert!(rust.contains(".collect::<Vec<_>>()"), "got:\n{rust}");
}

#[test]
fn translates_array_literal_with_expression_elements() {
    let src = "function f(n: number): number[] { return [n, n * 2, n + 1]; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("vec![n, n * 2_f64, n + 1_f64]"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_array_flat_to_concat() {
    let src = "function f(xss: number[][]): number[] { return xss.flat(); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".concat()"), "got:\n{rust}");
}

#[test]
fn translates_array_last_index_of_to_rposition() {
    let src = "function f(xs: number[]): number { return xs.lastIndexOf(2); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".rposition(|y| y == "), "got:\n{rust}");
}

#[test]
fn translates_array_fill_to_vec_fill() {
    let src = "function f(): void { let xs: number[] = [0]; xs.fill(1); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".fill(1_f64)"), "got:\n{rust}");
}

#[test]
fn translates_array_for_each_to_for_each() {
    let src =
        "function f(): void { const xs: number[] = [1, 2]; xs.forEach((n) => console.log(n)); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".for_each(|n|"), "got:\n{rust}");
}

#[test]
fn translates_array_includes_to_contains() {
    let src = "function f(): boolean { const xs: number[] = [1, 2, 3]; return xs.includes(2); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("xs.contains(&2_f64)"), "got:\n{rust}");
}

#[test]
fn translates_array_find_to_iter_copied_find() {
    let src =
        "function f(): void { const xs: number[] = [1, 2, 3]; const r = xs.find((n) => n > 1); }";
    let rust = Translator::new().translate(src).expect("should translate");
    // `.find`'s closure receives `&Item`, so its param is `|&n|`.
    assert!(rust.contains(".iter().copied().find(|&n|"), "got:\n{rust}");
}

#[test]
fn translates_array_some_every_to_any_all() {
    let src = "function f(): void { const xs: number[] = [1, 2, 3]; const a = xs.some((n) => n > 2); const b = xs.every((n) => n > 0); }";
    let rust = Translator::new().translate(src).expect("should translate");
    // `any`/`all` take the item by value → a plain `|n|` (not `|&n|`).
    assert!(rust.contains(".any(|n|"), "got:\n{rust}");
    assert!(rust.contains(".all(|n|"), "got:\n{rust}");
}

#[test]
fn translates_array_join_to_vec_string_join() {
    let src = "function f(): void { const xs: number[] = [1, 2, 3]; const s = xs.join(\"-\"); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".map(|x| x.to_string())"), "got:\n{rust}");
    assert!(rust.contains(".collect::<Vec<_>>()"), "got:\n{rust}");
    assert!(rust.contains(".join(\"-\")"), "got:\n{rust}");
}

#[test]
fn translates_array_reduce_with_seed_to_fold() {
    let src = "function f(): number { const xs: number[] = [1, 2, 3]; return xs.reduce((a, b) => a + b, 0); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".fold(0_f64, |a, b|"), "got:\n{rust}");
}

#[test]
fn translates_array_reduce_without_seed_to_reduce() {
    let src = "function f(): void { const xs: number[] = [1, 2, 3]; const r = xs.reduce((a, b) => a + b); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains(".iter().copied().reduce(|a, b|"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_array_index_assign_to_usize_index() {
    let src = "function f(): void { let xs: number[] = [1, 2, 3]; xs[0] = 9; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("xs[0_f64 as usize] = 9_f64"), "got:\n{rust}");
}

#[test]
fn translates_array_concat_to_slice_concat() {
    let src = "function f(): void { const a: number[] = [1, 2]; const b: number[] = [3]; const c = a.concat(b); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("[a.as_slice(), b.as_slice()].concat()"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_array_reverse_to_in_place_reverse() {
    let src = "function f(): void { let xs: number[] = [1, 2]; xs.reverse(); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("xs.reverse();"), "got:\n{rust}");
}

#[test]
fn translates_array_sort_to_numeric_sort_by() {
    let src = "function f(): void { let xs: number[] = [2, 1]; xs.sort(); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains(
            "xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(::core::cmp::Ordering::Equal));",
        ),
        "got:\n{rust}"
    );
}

#[test]
fn translates_array_find_last_to_rev_find() {
    let src = "function f(xs: number[]): number { return xs.findLast((n) => n > 1)!; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".rev().find("), "got:\n{rust}");
}

#[test]
fn translates_array_find_last_index_to_rposition() {
    let src = "function f(xs: number[]): number { return xs.findLastIndex((n) => n > 1); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".rposition("), "got:\n{rust}");
}

#[test]
fn translates_array_reduce_right_to_rev_fold() {
    let src = "function f(xs: number[]): number { return xs.reduceRight((a, b) => a + b, 0); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".rev().fold("), "got:\n{rust}");
}

#[test]
fn translates_array_to_sorted_to_clone_sort() {
    let src = "function f(xs: number[]): number[] { return xs.toSorted(); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".clone()"), "got:\n{rust}");
    assert!(
        rust.contains(".sort_by(|a, b| a.partial_cmp(b)"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_array_to_reversed_to_rev_collect() {
    let src = "function f(xs: number[]): number[] { return xs.toReversed(); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains(".iter().copied().rev().collect::<Vec<_>>()"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_array_to_spliced_to_clone_splice() {
    let src = "function f(xs: number[]): number[] { return xs.toSpliced(1, 2); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".clone()"), "got:\n{rust}");
    assert!(rust.contains(".splice("), "got:\n{rust}");
}

#[test]
fn translates_array_with_to_clone_index_assign() {
    let src = "function f(xs: number[]): number[] { return xs.with(0, 9); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".clone()"), "got:\n{rust}");
    assert!(rust.contains("__v[0_f64 as usize] = 9_f64"), "got:\n{rust}");
}

#[test]
fn translates_array_shift_unshift_pop() {
    let src =
        "function f(): void { let xs: number[] = [1, 2]; xs.unshift(0); xs.shift(); xs.pop(); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".insert(0,"), "got:\n{rust}");
    assert!(rust.contains(".remove(0)"), "got:\n{rust}");
    assert!(rust.contains(".pop()"), "got:\n{rust}");
}

#[test]
fn translates_array_of() {
    let src = "function f(): number[] { return Array.of(1, 2, 3); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("vec![1_f64, 2_f64, 3_f64]"), "got:\n{rust}");
}

#[test]
fn translates_array_is_array_vec_true() {
    let src = "function f(xs: number[]): boolean { return Array.isArray(xs); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("true") && !rust.contains("Array"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_array_from_clone() {
    let src = "function f(xs: number[]): number[] { return Array.from(xs); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".clone()"), "got:\n{rust}");
}

#[test]
fn translates_array_from_mapped() {
    let src = "function f(xs: number[]): number[] { return Array.from(xs, n => n * 2); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".iter().copied().map("), "got:\n{rust}");
}

#[test]
fn translates_array_splice() {
    let src = "function f(xs: number[]): void { xs.splice(1, 2); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".splice("), "got:\n{rust}");
}

#[test]
fn translates_array_to_string_join() {
    let src = "function f(xs: number[]): string { return xs.toString(); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".join(\",\")"), "got:\n{rust}");
}

#[test]
fn translates_array_copy_within() {
    let src = "function f(xs: number[]): void { xs.copyWithin(0, 1, 3); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".copy_within("), "got:\n{rust}");
}

#[test]
fn translates_array_filter_with_named_callback() {
    // A named function reference (the test262 `callbackfn` convention) is
    // passed straight through. `filter`'s predicate takes `&T`, so a named
    // `fn(Item) -> bool` is wrapped to deref (`|__cb| f(*__cb)`).
    let src = "function isPos(n: number): boolean { return n > 0; }\nfunction f(): void { const xs: number[] = [1, 2]; const ys = xs.filter(isPos); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".filter("), "got:\n{rust}");
    assert!(rust.contains("is_pos(*__cb)"), "got:\n{rust}");
}

#[test]
fn translates_array_every_with_named_callback() {
    // `every`/`some`/`forEach` take the item by value, so a named callback is
    // passed bare (no deref wrap).
    let src = "function allPos(n: number): boolean { return n > 0; }\nfunction f(): boolean { const xs: number[] = [1, 2]; return xs.every(allPos); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".all(all_pos)"), "got:\n{rust}");
}

#[test]
fn translates_array_prototype_call_to_method() {
    // `Array.prototype.map.call(xs, cb)` — borrowing an Array prototype method
    // via `.call` — lowers like `xs.map(cb)` (the Vec receiver is the first arg).
    let src = "function f(): void { const xs: number[] = [1, 2]; const ys = Array.prototype.map.call(xs, n => n * 2); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains(".iter().copied().map(|n| n * 2_f64)"),
        "got:\n{rust}"
    );
    assert!(!rust.contains("prototype"), "got:\n{rust}");
}

#[test]
fn translates_typeof_global_constructor_to_function() {
    // `typeof Array` → "function" (a global builtin constructor is callable),
    // not "object" (the fallback for an unknown identifier). Mirrors
    // test262's `Array/constructor.js`.
    let src = "function f(): void { console.log(typeof Array); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("\"function\""), "got:\n{rust}");
}
