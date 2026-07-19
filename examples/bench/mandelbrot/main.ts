// mandelbrot — Mandelbrot-set escape iteration over an 800×800 grid, single
// call. Floating-point- and branch-heavy; the inner `x*x + y*y <= 4` recurrence
// is the classic data-dependent loop that defeats vectorization. Mirrors
// perry's suite `15_mandelbrot` (WIDTH=HEIGHT=800, MAX_ITER=100). The same
// source runs under node/bun as TypeScript.
function runMandelbrot(): number {
  const WIDTH = 800;
  const HEIGHT = 800;
  const MAX_ITER = 100;
  let totalIter = 0;
  for (let py = 0; py < HEIGHT; py = py + 1) {
    for (let px = 0; px < WIDTH; px = px + 1) {
      const cx = ((px - WIDTH / 2.0) * 4.0) / WIDTH;
      const cy = ((py - HEIGHT / 2.0) * 4.0) / HEIGHT;
      let x = 0.0;
      let y = 0.0;
      let iter = 0;
      while (x * x + y * y <= 4.0 && iter < MAX_ITER) {
        const xtemp = x * x - y * y + cx;
        y = 2.0 * x * y + cy;
        x = xtemp;
        iter = iter + 1;
      }
      totalIter = totalIter + iter;
    }
  }
  return totalIter;
}
function main(): void {
  console.log(runMandelbrot());
}

main();
export {};
