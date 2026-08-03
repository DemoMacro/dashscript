// fetch-smoke — DashScript: WinterTC `fetch` (a native reqwest-backed async
// mapping, never degraded to the engine). `fetch(url)` → `__ds::ds_fetch`, and
// `await fetch(url)` records `r: DsResponse`, so `r.status`/`.ok`/`.headers`
// lower to the wrapper's zero-arg accessors. `fetch(url, init)` with a plain
// object `init` (method/body/headers) → `__ds::ds_fetch_with`. The Response
// body methods `await r.json()` / `await r.arrayBuffer()` lower to the
// wrapper's async `json`/`array_buffer` fns (consuming the body). Run the built
// binary to exercise the live network path against a real URL.

async function probe(url: string): Promise<void> {
  const r = await fetch(url);
  console.log(r.status);
  console.log(r.ok);
}

async function post(url: string): Promise<void> {
  const r = await fetch(url, {
    method: "POST",
    body: "hi",
    headers: { "Content-Type": "text/plain" },
  });
  console.log(r.status);
}

async function fetchJson(url: string): Promise<void> {
  const r = await fetch(url);
  await r.json();
}

async function fetchBytes(url: string): Promise<void> {
  const r = await fetch(url);
  await r.arrayBuffer();
}

await probe("https://example.com");
await post("https://example.com");
await fetchJson("https://example.com");
await fetchBytes("https://example.com");

export {};
