/// WHATWG URLPattern API helper — `__ds::DsURLPattern`. A `new URLPattern(input)`
/// (a WinterTC Web API) lowers here. A string `input` is parsed as a WHATWG
/// URLPattern constructor string (`UrlPatternInit::parse_constructor_string`);
/// an undefined/absent `input` is the empty pattern (every component `*`,
/// `UrlPatternInit::default`). `new URLPattern(new URL(…))` lowers to `from_str`
/// on the URL's href (the dispatcher ToString's the URL). A pattern that fails
/// to compile (an unclosed `(` group, …) panics a `TypeError` — the ES URLPattern
/// constructor's error class (`panic_any(DsError)`). Backed by the `urlpattern`
/// crate (denoland's WHATWG reference); the `URLPattern` runtime dep pulls it
/// and this slice, plus `Error` (for `DsError`). Pure-Rust — WinterTC never
/// degrades a Web API to the engine. Instance methods (`test`/`exec`) are not
/// yet lowered.
pub const URLPATTERN_HELPER: &str = r#"pub struct DsURLPattern(pub urlpattern::UrlPattern);
impl DsURLPattern {
    /// `new URLPattern("pattern")` — parse the constructor string, then compile
    /// the pattern. Either failure panics a `TypeError` (the ES error class).
    pub fn from_str(s: &str) -> Self {
        // `parse_constructor_string<R: RegExp>` returns `UrlPatternInit` (which
        // carries no `R`), so `R` cannot be inferred from context — name the
        // default engine explicitly. urlpattern 0.6 binds `R = regex::Regex`.
        let init = urlpattern::UrlPatternInit::parse_constructor_string::<regex::Regex>(s, None)
            .unwrap_or_else(|_| ::std::panic::panic_any(DsError::new("TypeError", "Invalid URLPattern")));
        Self(urlpattern::UrlPattern::parse(init, urlpattern::UrlPatternOptions::default())
            .unwrap_or_else(|_| ::std::panic::panic_any(DsError::new("TypeError", "Invalid URLPattern"))))
    }
    /// `new URLPattern(undefined, undefined)` / `new URLPattern()` — the empty
    /// pattern (every component `*`).
    pub fn empty() -> Self {
        Self(urlpattern::UrlPattern::parse(
            urlpattern::UrlPatternInit::default(),
            urlpattern::UrlPatternOptions::default(),
        )
        .unwrap_or_else(|_| ::std::panic::panic_any(DsError::new("TypeError", "Invalid URLPattern"))))
    }
}
"#;

/// WHATWG URL API helper — `__ds::DsUrlSearchParams`. An ordered name/value
/// list (ES `URLSearchParams` preserves insertion order), backed by
/// `Vec<(String, String)>`. Parsing and serialization route through
/// `form_urlencoded` (the WHATWG `application/x-www-form-urlencoded` reference
/// parser — the same one servo/url uses), so `+`→space and `%xx`
/// percent-decoding/encoding match the spec. `toString` is `Display`, so
/// template-literal interpolation of a `URLSearchParams` works without a
/// separate `DsDisplay` impl.
pub const URL_HELPER: &str = "\
/// WHATWG URL — `__ds::DsUrl`. Wraps `url::Url` (servo/url, the spec reference
/// parser). ES `URL` exposes the parsed components as zero-arg accessors; both
/// `JSON.stringify(url)` and `url.toString()` serialize to the `href` (the
/// WHATWG serialized URL), so `Display` is the href and `Serialize` is a string
/// (matching ES `URL.toJSON()`). `new URL(input[, base])` parses via `Url::parse`
/// / `Url::options().base_url(...)`; a parse error panics (ES throws
/// `TypeError` — the WPT verdict reads the panic prefix).
type DsUrlRef = ::std::rc::Rc<::std::cell::RefCell<url::Url>>;
/// Shared query operations on a `DsUrlRef`'s query string — used by both
/// `DsUrl::sp_*` (the live-view methods) and `DsUrlSearchParams`, so the
/// standalone object and the `url.searchParams` view share one implementation.
fn dsq_pairs(u: &DsUrlRef) -> ::std::vec::Vec<(::std::string::String, ::std::string::String)> {
    u.borrow()
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}
fn dsq_set_pairs(u: &DsUrlRef, pairs: &[(::std::string::String, ::std::string::String)]) {
    let serialized = form_urlencoded::Serializer::new(::std::string::String::new())
        .extend_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .finish();
    u.borrow_mut().set_query(if serialized.is_empty() {
        ::std::option::Option::None
    } else {
        ::std::option::Option::Some(&serialized)
    });
}
pub struct DsUrl(DsUrlRef);
impl DsUrl {
    /// `new URL(input)` — parse an absolute URL. Generic over `AsRef<str>` so
    /// the constructor emit passes either a `String` or a `&str` literal
    /// unchanged.
    pub fn parse<S: ::std::convert::AsRef<str>>(input: S) -> Self {
        Self(::std::rc::Rc::new(::std::cell::RefCell::new(
            url::Url::parse(input.as_ref()).expect(\"invalid URL\"),
        )))
    }
    /// `new URL(input, base)` — resolve `input` against `base`. The base is
    /// parsed first (its own failure panics), then `input` resolves against it.
    pub fn parse_with_base<I: ::std::convert::AsRef<str>, B: ::std::convert::AsRef<str>>(
        input: I,
        base: B,
    ) -> Self {
        let base = url::Url::parse(base.as_ref()).expect(\"invalid base URL\");
        Self(::std::rc::Rc::new(::std::cell::RefCell::new(
            url::Url::options()
                .base_url(::std::option::Option::Some(&base))
                .parse(input.as_ref())
                .expect(\"invalid URL\"),
        )))
    }
    /// `url.href` — the WHATWG serialized URL.
    pub fn href(&self) -> String {
        self.0.borrow().to_string()
    }
    /// `url.origin` — the ASCII serialization of the origin (`https://example.com`).
    pub fn origin(&self) -> String {
        self.0.borrow().origin().ascii_serialization()
    }
    /// `url.protocol` — the scheme plus `:` (`https:`).
    pub fn protocol(&self) -> String {
        format!(\"{}:\", self.0.borrow().scheme())
    }
    /// `url.host` — `hostname:port` (port omitted if not present).
    pub fn host(&self) -> String {
        let u = self.0.borrow();
        match u.port() {
            ::std::option::Option::Some(p) => {
                format!(\"{}:{}\", u.host_str().unwrap_or(\"\"), p)
            }
            ::std::option::Option::None => u.host_str().unwrap_or(\"\").to_string(),
        }
    }
    /// `url.hostname` — the host without the port.
    pub fn hostname(&self) -> String {
        self.0.borrow().host_str().unwrap_or(\"\").to_string()
    }
    /// `url.pathname` — the path (`/path`).
    pub fn pathname(&self) -> String {
        self.0.borrow().path().to_string()
    }
    /// `url.search` — `?` plus the query, or `\"\"` if absent.
    pub fn search(&self) -> String {
        self.0
            .borrow()
            .query()
            .map(|q| format!(\"?{}\", q))
            .unwrap_or_default()
    }
    /// `url.hash` — `#` plus the fragment, or `\"\"` if absent.
    pub fn hash(&self) -> String {
        self.0
            .borrow()
            .fragment()
            .map(|f| format!(\"#{}\", f))
            .unwrap_or_default()
    }
    /// `url.port` — the port as a string, or `\"\"` if absent.
    pub fn port(&self) -> String {
        self.0.borrow().port().map(|p| p.to_string()).unwrap_or_default()
    }
    /// `url.username` — the username, or `\"\"` if absent.
    pub fn username(&self) -> String {
        self.0.borrow().username().to_string()
    }
    /// `url.password` — the password, or `\"\"` if absent.
    pub fn password(&self) -> String {
        self.0.borrow().password().unwrap_or(\"\").to_string()
    }
    // ---- `url.searchParams` live view ----
    // The query lives inside the wrapped `url::Url`; these read it via
    // `query_pairs()` and write it back via `set_query`, so a mutation
    // (`delete`/`append`/`set`) is visible to the next `href`/`search`/`size`.
    fn sp_pairs(&self) -> Vec<(String, String)> {
        dsq_pairs(&self.0)
    }
    fn sp_set_pairs(&self, pairs: &[(String, String)]) {
        dsq_set_pairs(&self.0, pairs)
    }
    pub fn sp_size(&self) -> usize {
        self.sp_pairs().len()
    }
    pub fn sp_get<S: ::std::convert::AsRef<str>>(&self, name: S) -> Option<String> {
        let name = name.as_ref();
        self.sp_pairs().into_iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }
    pub fn sp_has<S: ::std::convert::AsRef<str>>(&self, name: S) -> bool {
        let name = name.as_ref();
        self.sp_pairs().iter().any(|(k, _)| k == name)
    }
    pub fn sp_has_value<N: ::std::convert::AsRef<str>, V: ::std::convert::AsRef<str>>(
        &self,
        name: N,
        value: V,
    ) -> bool {
        let name = name.as_ref();
        let value = value.as_ref();
        self.sp_pairs().iter().any(|(k, v)| k == name && v == value)
    }
    pub fn sp_delete<S: ::std::convert::AsRef<str>>(&self, name: S) {
        let name = name.as_ref();
        let mut p = self.sp_pairs();
        p.retain(|(k, _)| k != name);
        self.sp_set_pairs(&p);
    }
    pub fn sp_delete_value<N: ::std::convert::AsRef<str>, V: ::std::convert::AsRef<str>>(
        &self,
        name: N,
        value: V,
    ) {
        let name = name.as_ref();
        let value = value.as_ref();
        let mut p = self.sp_pairs();
        p.retain(|(k, v)| !(k == name && v == value));
        self.sp_set_pairs(&p);
    }
    pub fn sp_append<N: ::std::convert::AsRef<str>, V: ::std::convert::AsRef<str>>(
        &self,
        name: N,
        value: V,
    ) {
        let mut p = self.sp_pairs();
        p.push((name.as_ref().to_string(), value.as_ref().to_string()));
        self.sp_set_pairs(&p);
    }
    pub fn sp_set<N: ::std::convert::AsRef<str>, V: ::std::convert::AsRef<str>>(
        &self,
        name: N,
        value: V,
    ) {
        let name = name.as_ref();
        let value = value.as_ref();
        let mut p = self.sp_pairs();
        if let ::std::option::Option::Some(e) = p.iter_mut().find(|(k, _)| k == name) {
            e.1 = value.to_string();
        } else {
            p.push((name.to_string(), value.to_string()));
        }
        self.sp_set_pairs(&p);
    }
    pub fn sp_get_all<S: ::std::convert::AsRef<str>>(&self, name: S) -> Vec<String> {
        let name = name.as_ref();
        self.sp_pairs()
            .into_iter()
            .filter(|(k, _)| k == name)
            .map(|(_, v)| v)
            .collect()
    }
    pub fn sp_sort(&self) {
        let mut p = self.sp_pairs();
        p.sort_by(|a, b| a.0.cmp(&b.0));
        self.sp_set_pairs(&p);
    }
    pub fn sp_to_string(&self) -> String {
        let p = self.sp_pairs();
        form_urlencoded::Serializer::new(String::new())
            .extend_pairs(p.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .finish()
    }
    /// `url.searchParams.forEach(cb)` — see `DsUrlSearchParams::for_each`.
    /// Same value-first/key-second order; operates on the URL's live query.
    /// `FnMut` (not `Fn`) so a callback accumulating into a captured outer
    /// binding (`keys.push(key)`, the universal WPT `forEach` pattern) compiles.
    pub fn sp_for_each<F: FnMut(String, String)>(&self, mut f: F) {
        for (k, v) in self.sp_pairs() {
            f(v, k);
        }
    }
    /// `url.searchParams.entries()` — see `DsUrlSearchParams::entries_vec`.
    /// Operates on the URL's live query; same `[name, value]` array shape.
    pub fn sp_entries_vec(&self) -> Vec<Vec<String>> {
        self.sp_pairs().into_iter().map(|(k, v)| vec![k, v]).collect()
    }
    /// `url.searchParams.keys()` — see `DsUrlSearchParams::keys_vec`.
    pub fn sp_keys_vec(&self) -> Vec<String> {
        self.sp_pairs().into_iter().map(|(k, _)| k).collect()
    }
    /// `url.searchParams.values()` — see `DsUrlSearchParams::values_vec`.
    pub fn sp_values_vec(&self) -> Vec<String> {
        self.sp_pairs().into_iter().map(|(_, v)| v).collect()
    }
    /// `url.searchParams` — a live view of this URL's query. Returns a
    /// `DsUrlSearchParams` sharing the same ref-counted `url::Url` (an `Rc`
    /// clone), so a mutation through the view (`params.append(…)`) is
    /// immediately visible to `url.href`/`url.search`/the next
    /// `url.searchParams.size` — the ES live-view semantics.
    pub fn sp_view(&self) -> DsUrlSearchParams {
        DsUrlSearchParams(self.0.clone())
    }
    /// `url.search = s` — the WHATWG search setter. Strips a leading `?`,
    /// then sets the query (empty → no query, so `url.search` reads back as
    /// `\"\"`).
    pub fn set_search<S: ::std::convert::AsRef<str>>(&self, s: S) {
        let s = s.as_ref();
        let q = s.strip_prefix('?').unwrap_or(s);
        self.0.borrow_mut().set_query(if q.is_empty() {
            ::std::option::Option::None
        } else {
            ::std::option::Option::Some(q)
        });
    }
}
impl ::core::fmt::Display for DsUrl {
    /// `url.toString()` / string coercion — the href.
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        write!(f, \"{}\", &*self.0.borrow())
    }
}
impl ::serde::Serialize for DsUrl {
    /// `JSON.stringify(url)` / `url.toJSON()` — ES serializes a URL as its href
    /// string (a JSON string, quoted), so `Serialize` emits the href as a `str`.
    fn serialize<S: ::serde::Serializer>(
        &self,
        s: S,
    ) -> ::core::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.0.borrow().to_string())
    }
}
// ---- `URL.<static>` as `DsUrl` associated functions ----
// The `URL` constructor object's static methods are not instance methods (no
// `&self`) — they are associated functions on `DsUrl`, so the emit carries the
// `__ds::DsUrl` marker and the `Url` runtime dep fires (the helper slice ships
// alongside the `DsUrl` type, the same dep `new URL(…)` pulls). `URL.parse`
// returns `Option<DsUrl>` (ES `null` on a parse failure, not a throw);
// `URL.canParse` is the boolean form. ES `ToString` is applied at the call
// site, so an `undefined` argument arrives as the string `\"undefined\"`, which
// fails to parse (matching `URL.canParse(undefined)` → false).
impl DsUrl {
    /// `URL.canParse(url)` — true iff `url` parses as an absolute URL.
    pub fn can_parse<S: ::std::convert::AsRef<str>>(url: S) -> bool {
        url::Url::parse(url.as_ref()).is_ok()
    }
    /// `URL.canParse(url, base)` — true iff `url` resolves against `base`. A
    /// `base` that is itself unparseable fails the whole parse (returns false).
    pub fn can_parse_with_base<U: ::std::convert::AsRef<str>, B: ::std::convert::AsRef<str>>(
        url: U,
        base: B,
    ) -> bool {
        match url::Url::parse(base.as_ref()) {
            ::std::result::Result::Ok(b) => url::Url::options()
                .base_url(::std::option::Option::Some(&b))
                .parse(url.as_ref())
                .is_ok(),
            ::std::result::Result::Err(_) => false,
        }
    }
    /// `URL.parse(url)` — `Some(DsUrl)` on success, `None` on failure (ES
    /// `null`). Each call builds a fresh `Rc<RefCell<Url>>`, so
    /// `URL.parse(x) !== URL.parse(x)` (object identity differs), matching the
    /// WPT `unique object` assertion.
    pub fn parse_opt<S: ::std::convert::AsRef<str>>(url: S) -> ::std::option::Option<DsUrl> {
        url::Url::parse(url.as_ref())
            .ok()
            .map(|u| Self(::std::rc::Rc::new(::std::cell::RefCell::new(u))))
    }
    /// `URL.parse(url, base)` — resolve `url` against `base`, `None` on failure.
    pub fn parse_opt_with_base<U: ::std::convert::AsRef<str>, B: ::std::convert::AsRef<str>>(
        url: U,
        base: B,
    ) -> ::std::option::Option<DsUrl> {
        match url::Url::parse(base.as_ref()) {
            ::std::result::Result::Ok(b) => url::Url::options()
                .base_url(::std::option::Option::Some(&b))
                .parse(url.as_ref())
                .ok()
                .map(|u| Self(::std::rc::Rc::new(::std::cell::RefCell::new(u)))),
            ::std::result::Result::Err(_) => ::std::option::Option::None,
        }
    }
}
pub struct DsUrlSearchParams(DsUrlRef);
impl DsUrlSearchParams {
    /// `new URLSearchParams(s)` — parse `s` as
    /// `application/x-www-form-urlencoded`. The standalone object owns a
    /// throwaway `url::Url` (only its query string matters); query read/write
    /// routes through the same `dsq_*` machinery as the `url.searchParams`
    /// live view, so a standalone and a view behave identically. A leading
    /// `?` is stripped (ES accepts both `\"a=b\"` and `\"?a=b\"`).
    pub fn from_query<S: AsRef<str>>(init: S) -> Self {
        let init = init.as_ref();
        let q = init.strip_prefix('?').unwrap_or(init);
        let pairs: ::std::vec::Vec<(::std::string::String, ::std::string::String)> =
            form_urlencoded::parse(q.as_bytes())
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
        let inner = ::std::rc::Rc::new(::std::cell::RefCell::new(
            url::Url::parse(\"http://localhost/\").expect(\"fallback URL\"),
        ));
        dsq_set_pairs(&inner, &pairs);
        Self(inner)
    }
    /// `new URLSearchParams()` / `new URLSearchParams(undefined)` — empty.
    pub fn new() -> Self {
        Self(::std::rc::Rc::new(::std::cell::RefCell::new(
            url::Url::parse(\"http://localhost/\").expect(\"fallback URL\"),
        )))
    }
    /// `params.get(name)` — the first value for `name`, or `None` (ES `null`).
    /// Generic over `AsRef<str>` so a `String` or `&str` argument (both TS
    /// `string`) is accepted without a call-site borrow.
    pub fn get<S: AsRef<str>>(&self, name: S) -> Option<String> {
        let name = name.as_ref();
        dsq_pairs(&self.0).into_iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }
    /// `params.has(name)` — whether any pair's name is `name`.
    pub fn has<S: AsRef<str>>(&self, name: S) -> bool {
        let name = name.as_ref();
        dsq_pairs(&self.0).iter().any(|(k, _)| k == name)
    }
    /// `params.has(name, value)` (ES2024) — whether a `(name, value)` pair
    /// exists. The single-arg `has(name)` is the common form; the two-arg
    /// form matches both name and value.
    pub fn has_value<N: AsRef<str>, V: AsRef<str>>(&self, name: N, value: V) -> bool {
        let name = name.as_ref();
        let value = value.as_ref();
        dsq_pairs(&self.0).iter().any(|(k, v)| k == name && v == value)
    }
    /// `params.set(name, value)` — WHATWG set: update the first matching pair's
    /// value in place, drop any later matches, or append if none. Not
    /// delete-all-then-append — that would move the pair to the end; the spec
    /// keeps the first match position: `set('a','B')` on `'a=b&c=d'` yields
    /// `a=B&c=d`.
    pub fn set<N: AsRef<str>, V: AsRef<str>>(&self, name: N, value: V) {
        let name = name.as_ref();
        let value = value.as_ref().to_string();
        let mut p = dsq_pairs(&self.0);
        let mut found = false;
        // Keep the first match (to update in place), drop later matches.
        p.retain(|(k, _)| {
            if k == name {
                if found {
                    false
                } else {
                    found = true;
                    true
                }
            } else {
                true
            }
        });
        if found {
            for pair in &mut p {
                if pair.0 == name {
                    pair.1 = value;
                    break;
                }
            }
        } else {
            p.push((name.to_string(), value));
        }
        dsq_set_pairs(&self.0, &p);
    }
    /// `params.append(name, value)` — append a pair (duplicates kept).
    pub fn append<N: AsRef<str>, V: AsRef<str>>(&self, name: N, value: V) {
        let mut p = dsq_pairs(&self.0);
        p.push((name.as_ref().to_string(), value.as_ref().to_string()));
        dsq_set_pairs(&self.0, &p);
    }
    /// `params.delete(name)` — remove every pair named `name`.
    pub fn delete<S: AsRef<str>>(&self, name: S) {
        let name = name.as_ref();
        let mut p = dsq_pairs(&self.0);
        p.retain(|(k, _)| k != name);
        dsq_set_pairs(&self.0, &p);
    }
    /// `params.delete(name, value)` (ES2024) — remove only pairs matching both
    /// `name` and `value`; the single-arg `delete(name)` removes every pair
    /// with that name.
    pub fn delete_value<N: AsRef<str>, V: AsRef<str>>(&self, name: N, value: V) {
        let name = name.as_ref();
        let value = value.as_ref();
        let mut p = dsq_pairs(&self.0);
        p.retain(|(k, v)| !(k == name && v == value));
        dsq_set_pairs(&self.0, &p);
    }
    /// `params.getAll(name)` — every value for `name`, in insertion order.
    pub fn get_all<S: AsRef<str>>(&self, name: S) -> Vec<String> {
        let name = name.as_ref();
        dsq_pairs(&self.0)
            .into_iter()
            .filter(|(k, _)| k == name)
            .map(|(_, v)| v)
            .collect()
    }
    /// `params.sort()` — sort by name. Rust's `sort_by` is stable, matching
    /// ES (equal names keep their relative order).
    pub fn sort(&self) {
        let mut p = dsq_pairs(&self.0);
        p.sort_by(|a, b| a.0.cmp(&b.0));
        dsq_set_pairs(&self.0, &p);
    }
    /// `params.size` — the number of name/value pairs.
    #[inline]
    pub fn len(&self) -> usize {
        dsq_pairs(&self.0).len()
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// `params.forEach(cb)` — invoke `cb(value, key)` for each pair in
    /// insertion order. WHATWG URLSearchParams uses value-first/key-second
    /// order (the opposite of `Map.forEach`); the third callback arg (the
    /// params object) and `thisArg` are reflection the static path drops.
    /// `cb` takes owned `String`s so `keys.push(key)` type-checks against a
    /// `Vec<String>` accumulator (the `assert_array_equals` operand shape).
    /// `FnMut` (not `Fn`) so a callback that mutates a captured outer binding
    /// (the `keys.push`/`values.push` accumulator pattern) compiles.
    pub fn for_each<F: FnMut(String, String)>(&self, mut f: F) {
        for (k, v) in dsq_pairs(&self.0) {
            f(v, k);
        }
    }
    /// `params.entries()` — every `[name, value]` pair as a materialized
    /// `Vec<Vec<String>>` (insertion order). The static path trades the ES
    /// iterator wrapper for a `Vec`; each pair is a two-element `[name, value]`
    /// array matching the live `DsUrlSearchParamsIter` item shape, so a WPT
    /// `assert_array_equals(entry, [\"a\", \"1\"])` holds. `for (const [k, v] of
    /// params.entries())` then destructures each array via index access.
    pub fn entries_vec(&self) -> Vec<Vec<String>> {
        dsq_pairs(&self.0).into_iter().map(|(k, v)| vec![k, v]).collect()
    }
    /// `params.keys()` — every name as a `Vec<String>` (insertion order).
    pub fn keys_vec(&self) -> Vec<String> {
        dsq_pairs(&self.0).into_iter().map(|(k, _)| k).collect()
    }
    /// `params.values()` — every value as a `Vec<String>` (insertion order).
    pub fn values_vec(&self) -> Vec<String> {
        dsq_pairs(&self.0).into_iter().map(|(_, v)| v).collect()
    }
}
impl ::core::fmt::Display for DsUrlSearchParams {
    /// `params.toString()` — serialize back to
    /// `application/x-www-form-urlencoded`. `form_urlencoded::Serializer`
    /// percent-encodes per the WHATWG byte set.
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        let p = dsq_pairs(&self.0);
        let mut s = form_urlencoded::Serializer::new(String::new());
        for (k, v) in &p {
            s.append_pair(k, v);
        }
        write!(f, \"{}\", s.finish())
    }
}
/// Iterator over a `DsUrlSearchParams`'s `[name, value]` pairs — what ES
/// `for (const entry of params)` / `params.entries()` yields. Each item is a
/// two-element `[name, value]` array (so `assert_array_equals(entry, [\"a\",
/// \"1\"])` holds). The iterator is **live**: it shares the `DsUrlRef` and
/// re-reads the query list each step (advancing a cursor), so a mutation to
/// the underlying URL mid-iteration (`url.search = …`) is visible to later
/// steps — the WHATWG URLSearchParams iterator semantics a WPT fixture
/// exercises.
pub struct DsUrlSearchParamsIter {
    inner: DsUrlRef,
    idx: usize,
}
impl ::std::iter::Iterator for DsUrlSearchParamsIter {
    type Item = ::std::vec::Vec<::std::string::String>;
    fn next(&mut self) -> ::std::option::Option<Self::Item> {
        let pairs = dsq_pairs(&self.inner);
        if self.idx < pairs.len() {
            let (k, v) = pairs[self.idx].clone();
            self.idx += 1;
            ::std::option::Option::Some(vec![k, v])
        } else {
            ::std::option::Option::None
        }
    }
}
impl<'a> ::std::iter::IntoIterator for &'a DsUrlSearchParams {
    type Item = ::std::vec::Vec<::std::string::String>;
    type IntoIter = DsUrlSearchParamsIter;
    fn into_iter(self) -> Self::IntoIter {
        DsUrlSearchParamsIter { inner: self.0.clone(), idx: 0 }
    }
}
";
