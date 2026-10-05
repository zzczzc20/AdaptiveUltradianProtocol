type Child = Node | string | null | undefined | false;
type Attrs = Record<string, unknown> & { class?: string; on?: Record<string, (e: Event) => void> };

/** 极简 DOM 构造器（避免引入框架） */
export function h<K extends keyof HTMLElementTagNameMap>(tag: K, attrs: Attrs = {}, ...kids: (Child | Child[])[]): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v === undefined || v === null || v === false) continue;
    if (k === "class") el.className = String(v);
    else if (k === "on") for (const [ev, fn] of Object.entries(v as Record<string, (e: Event) => void>)) el.addEventListener(ev, fn);
    else if (k === "value") (el as unknown as HTMLInputElement).value = String(v);
    else if (k in el && typeof v !== "string") (el as unknown as Record<string, unknown>)[k] = v;
    else el.setAttribute(k, v === true ? "" : String(v));
  }
  for (const kid of kids.flat()) {
    if (kid === null || kid === undefined || kid === false) continue;
    el.append(typeof kid === "string" ? document.createTextNode(kid) : kid);
  }
  return el;
}

export function fmtClock(sec: number): string {
  const s = Math.max(0, Math.round(sec));
  const m = Math.floor(s / 60);
  return `${String(m).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

let toastTimer: number | undefined;
export function toast(msg: string): void {
  document.querySelector(".toast")?.remove();
  const t = h("div", { class: "toast" }, msg);
  document.body.append(t);
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => t.remove(), 3500);
}

/** 草稿存储：仅在 sessionStorage 可用时使用，失败静默 */
export const draft = {
  get(k: string): string {
    try { return sessionStorage.getItem("flowarc." + k) ?? ""; } catch { return ""; }
  },
  set(k: string, v: string): void {
    try { sessionStorage.setItem("flowarc." + k, v); } catch { /* ignore */ }
  },
  clear(prefix: string): void {
    try {
      for (const k of Object.keys(sessionStorage)) if (k.startsWith("flowarc." + prefix)) sessionStorage.removeItem(k);
    } catch { /* ignore */ }
  },
};
