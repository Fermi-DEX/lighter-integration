// viz.js — the only two charts on the page: a cadence sparkline (single-series
// line, change-over-time) and a receipt-latency histogram (single-series
// columns, magnitude). Both dark-surface only. Mark specs per the dataviz
// skill: 2px round line + ≥8px end dot with a 2px surface ring; columns ≤24px
// with a 4px rounded cap square at the baseline and a 2px surface gap; hairline
// recessive axes; a hover layer; text in ink tokens, never the series hue.

const SVGNS = "http://www.w3.org/2000/svg";
function svgEl(tag, attrs) {
  const n = document.createElementNS(SVGNS, tag);
  for (const k in attrs) n.setAttribute(k, attrs[k]);
  return n;
}

// samples: array of numbers (ticks/sec). Renders into `host` (cleared).
export function cadenceSparkline(host, samples, opts) {
  opts = opts || {};
  host.innerHTML = "";
  const W = host.clientWidth || 320;
  const H = opts.height || 56;
  const padL = 6, padR = 34, padT = 8, padB = 8;
  const svg = svgEl("svg", { width: "100%", height: H, viewBox: "0 0 " + W + " " + H, class: "spark" });
  if (samples.length < 2) {
    svg.appendChild(svgEl("line", { x1: padL, y1: H - padB, x2: W - padR, y2: H - padB, class: "axis-line" }));
    host.appendChild(svg);
    return;
  }
  const max = Math.max(...samples, 1);
  const min = 0;
  const n = samples.length;
  const xw = W - padL - padR;
  const yh = H - padT - padB;
  const X = (i) => padL + (i / (n - 1)) * xw;
  const Y = (v) => padT + (1 - (v - min) / (max - min || 1)) * yh;
  // baseline
  svg.appendChild(svgEl("line", { x1: padL, y1: H - padB, x2: W - padR, y2: H - padB, class: "axis-line" }));
  let d = "";
  for (let i = 0; i < n; i++) d += (i === 0 ? "M" : "L") + X(i).toFixed(1) + " " + Y(samples[i]).toFixed(1);
  svg.appendChild(svgEl("path", { d, class: "spark-line", fill: "none" }));
  const lx = X(n - 1), ly = Y(samples[n - 1]);
  svg.appendChild(svgEl("circle", { cx: lx, cy: ly, r: 4, class: "spark-dot-ring" }));
  svg.appendChild(svgEl("circle", { cx: lx, cy: ly, r: 2.5, class: "spark-dot" }));
  const lab = svgEl("text", { x: W - padR + 5, y: ly + 3.5, class: "spark-label" });
  lab.textContent = samples[n - 1].toFixed(1);
  svg.appendChild(lab);
  // hover crosshair
  const hv = svgEl("line", { x1: 0, y1: padT, x2: 0, y2: H - padB, class: "spark-hover", style: "display:none" });
  svg.appendChild(hv);
  const tip = svgEl("text", { class: "spark-tip", style: "display:none" });
  svg.appendChild(tip);
  svg.addEventListener("mousemove", (e) => {
    const rect = svg.getBoundingClientRect();
    const px = ((e.clientX - rect.left) / rect.width) * W;
    let i = Math.round(((px - padL) / xw) * (n - 1));
    i = Math.max(0, Math.min(n - 1, i));
    hv.setAttribute("x1", X(i));
    hv.setAttribute("x2", X(i));
    hv.style.display = "";
    tip.textContent = samples[i].toFixed(1) + " tk/s";
    tip.setAttribute("x", Math.min(X(i) + 4, W - 44));
    tip.setAttribute("y", padT + 8);
    tip.style.display = "";
  });
  svg.addEventListener("mouseleave", () => {
    hv.style.display = "none";
    tip.style.display = "none";
  });
  host.appendChild(svg);
}

// latencies: array of µs. Bins into columns; overlays p50/p95 guides.
export function latencyHistogram(host, latencies, opts) {
  opts = opts || {};
  host.innerHTML = "";
  const W = host.clientWidth || 360;
  const H = opts.height || 150;
  const padL = 30, padR = 10, padT = 10, padB = 22;
  const svg = svgEl("svg", { width: "100%", height: H, viewBox: "0 0 " + W + " " + H, class: "hist" });
  host.appendChild(svg);
  if (!latencies.length) {
    const t = svgEl("text", { x: W / 2, y: H / 2, class: "hist-empty", "text-anchor": "middle" });
    t.textContent = "awaiting submissions…";
    svg.appendChild(t);
    return;
  }
  const sorted = latencies.slice().sort((a, b) => a - b);
  const pick = (p) => sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * p))];
  const p50 = pick(0.5), p95 = pick(0.95), max = sorted[sorted.length - 1];
  const nb = Math.min(16, Math.max(6, Math.round(Math.sqrt(latencies.length))));
  const hi = max || 1;
  const bw = hi / nb;
  const bins = new Array(nb).fill(0);
  for (const v of latencies) bins[Math.min(nb - 1, Math.floor(v / bw))]++;
  const ymax = Math.max(...bins, 1);
  const plotW = W - padL - padR, plotH = H - padT - padB;
  const X = (v) => padL + (v / hi) * plotW;
  const Y = (c) => padT + (1 - c / ymax) * plotH;
  const base = H - padB;
  // y gridline at top count
  svg.appendChild(svgEl("line", { x1: padL, y1: padT, x2: W - padR, y2: padT, class: "grid-line" }));
  const yt = svgEl("text", { x: padL - 5, y: padT + 4, class: "ax-tick", "text-anchor": "end" });
  yt.textContent = ymax;
  svg.appendChild(yt);
  const y0 = svgEl("text", { x: padL - 5, y: base + 3, class: "ax-tick", "text-anchor": "end" });
  y0.textContent = "0";
  svg.appendChild(y0);
  // baseline
  svg.appendChild(svgEl("line", { x1: padL, y1: base, x2: W - padR, y2: base, class: "axis-line" }));
  const slot = plotW / nb;
  const gap = 2; // surface gap between columns
  const colW = Math.min(24, slot - gap);
  let tallest = 0;
  for (let i = 0; i < nb; i++) if (bins[i] > bins[tallest]) tallest = i;
  for (let i = 0; i < nb; i++) {
    if (bins[i] === 0) continue;
    const x = padL + i * slot + (slot - colW) / 2;
    const h = base - Y(bins[i]);
    const r = svgEl("rect", { x: x.toFixed(1), y: Y(bins[i]).toFixed(1), width: colW.toFixed(1), height: Math.max(0.5, h).toFixed(1), rx: 3, class: "hist-bar" });
    const lo = Math.round(i * bw), up = Math.round((i + 1) * bw);
    r.appendChild(svgEl("title", {})).textContent = lo + "–" + up + "µs · " + bins[i] + " receipts";
    svg.appendChild(r);
    if (i === tallest) {
      const cap = svgEl("text", { x: (x + colW / 2).toFixed(1), y: (Y(bins[i]) - 3).toFixed(1), class: "hist-cap", "text-anchor": "middle" });
      cap.textContent = bins[i];
      svg.appendChild(cap);
    }
  }
  // p50 / p95 guides
  for (const g of [{ v: p50, l: "p50" }, { v: p95, l: "p95" }]) {
    const gx = X(g.v);
    svg.appendChild(svgEl("line", { x1: gx.toFixed(1), y1: padT, x2: gx.toFixed(1), y2: base, class: "guide-line" }));
    const t = svgEl("text", { x: Math.min(gx + 3, W - padR - 22).toFixed(1), y: padT + 9, class: "guide-label" });
    t.textContent = g.l + " " + Math.round(g.v) + "µs";
    svg.appendChild(t);
  }
  // x axis label
  const xl = svgEl("text", { x: W - padR, y: H - 5, class: "ax-tick", "text-anchor": "end" });
  xl.textContent = "µs → max " + Math.round(max);
  svg.appendChild(xl);
}
