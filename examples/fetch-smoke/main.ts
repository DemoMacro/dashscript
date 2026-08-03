// fetch-smoke — DashScript: WinterTC `fetch` (a native reqwest-backed async
// mapping, never degraded to the engine). `fetch(url)` → `__ds::ds_fetch`, and
// `await fetch(url)` records `r: DsResponse`, so `r.status`/`.ok`/`.headers`
// lower to the wrapper's zero-arg accessors. Run the built binary to exercise
// the live network path against a real URL.

async function probe(url: string): Promise<void> {
  const r = await fetch(url);
  console.log(r.status);
  console.log(r.ok);
}

await probe("https://example.com");

export {};
