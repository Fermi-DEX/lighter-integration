// util.js — DOM + formatting helpers. ES module.

export function el(tag, attrs, children) {
  const n = document.createElement(tag);
  if (attrs) {
    for (const k in attrs) {
      if (k === "class") n.className = attrs[k];
      else if (k === "html") n.innerHTML = attrs[k];
      else if (k === "text") n.textContent = attrs[k];
      else if (k.slice(0, 2) === "on" && typeof attrs[k] === "function") n.addEventListener(k.slice(2), attrs[k]);
      else if (attrs[k] != null) n.setAttribute(k, attrs[k]);
    }
  }
  if (children != null) {
    const arr = Array.isArray(children) ? children : [children];
    for (const c of arr) {
      if (c == null) continue;
      n.appendChild(typeof c === "string" || typeof c === "number" ? document.createTextNode(String(c)) : c);
    }
  }
  return n;
}

export function clear(node) {
  while (node.firstChild) node.removeChild(node.firstChild);
  return node;
}

// A hex value truncated to head+tail, click to expand, with a copy button.
export function hexChip(value, opts) {
  opts = opts || {};
  const full = String(value == null ? "" : value);
  const head = opts.head || 6;
  const tail = opts.tail || 4;
  const short = full.length <= 2 + head + tail + 3 ? full : full.slice(0, 2 + head) + "…" + full.slice(-tail);
  const chip = el("span", { class: "hex", title: full });
  const txt = el("span", { class: "hex-txt", text: opts.expanded ? full : short });
  let expanded = !!opts.expanded;
  txt.addEventListener("click", () => {
    expanded = !expanded;
    txt.textContent = expanded ? full : short;
  });
  const copy = el("button", { class: "copy", title: "copy", text: "⧉" });
  copy.addEventListener("click", async (e) => {
    e.stopPropagation();
    try {
      await navigator.clipboard.writeText(full);
      copy.textContent = "✓";
      setTimeout(() => (copy.textContent = "⧉"), 900);
    } catch (_) {
      copy.textContent = "!";
    }
  });
  chip.appendChild(txt);
  chip.appendChild(copy);
  return chip;
}

// A verification badge: ✓ verified in-browser / ✗ mismatch, expandable to the
// list of individual checks (each "what was checked").
export function badge(result) {
  const ok = !!result.ok;
  const wrap = el("span", { class: "vbadge " + (ok ? "ok" : "bad") });
  const head = el("span", { class: "vbadge-head", text: ok ? "✓ verified in-browser" : "✗ mismatch" });
  wrap.appendChild(head);
  const list = el("div", { class: "vbadge-checks" });
  for (const c of result.checks || []) {
    list.appendChild(
      el("div", { class: "vcheck " + (c.ok ? "ok" : "bad") }, [
        el("span", { class: "vc-mark", text: c.ok ? "✓" : "✗" }),
        el("span", { class: "vc-label", text: c.label }),
        el("span", { class: "vc-detail", text: c.detail || "" }),
      ])
    );
  }
  wrap.appendChild(list);
  head.addEventListener("click", () => wrap.classList.toggle("open"));
  return wrap;
}

export function statusChip(text, tone) {
  return el("span", { class: "schip " + (tone || ""), text });
}

export function num(n) {
  if (n == null) return "—";
  return Number(n).toLocaleString("en-US");
}

export function ago(ms) {
  if (!ms) return "—";
  const d = Date.now() - ms;
  if (d < 1000) return d + "ms";
  if (d < 60000) return (d / 1000).toFixed(1) + "s";
  return Math.floor(d / 60000) + "m" + Math.floor((d % 60000) / 1000) + "s";
}

// A bounded list container: keep only the latest N children + a count label.
export function cappedList(node, cap) {
  return {
    node,
    prepend(child) {
      node.insertBefore(child, node.firstChild);
      while (node.children.length > cap) node.removeChild(node.lastChild);
    },
    replaceAll(children) {
      clear(node);
      for (const c of children.slice(0, cap)) node.appendChild(c);
    },
  };
}

export async function getJSON(path) {
  const r = await fetch(path);
  if (!r.ok) throw new Error(path + " → " + r.status);
  return r.json();
}

export async function postJSON(path) {
  const r = await fetch(path, { method: "POST" });
  if (!r.ok) throw new Error(path + " → " + r.status);
  return r.json();
}
