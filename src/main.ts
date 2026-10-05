import "./styles.css";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import { draft, fmtClock, h, toast } from "./dom";
import type {
  BlockKind, BreakKind, ContextItem, EarlyReason, Insights, ParkingItem, Settings, StateView, WorkMode,
} from "./types";

const root = document.getElementById("app") as HTMLElement;

let view: StateView;
let overlayScreen: "settings" | "shutdown" | null = null;
let reopenedAfterClose = false;
let pendingContext: ContextItem[] = [];
let currentKey = "";
let updater: ((v: StateView) => void) | null = null;

const KIND_LABEL: Record<BlockKind, string> = { gold: "金块（最难的事）", silver: "银块（次要深度）", admin: "行政/碎片" };
const BREAK_NAME: Record<BreakKind, string> = { rest: "休息", walk: "散步", move: "走动", calm: "放松", free: "自由" };

function errMsg(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}
async function run<T>(p: Promise<T>): Promise<T | undefined> {
  try { return await p; } catch (e) { toast(errMsg(e)); return undefined; }
}
function setView(v: StateView | undefined): void {
  if (v) { view = v; route(); }
}

// ───────────── 路由 ─────────────
function screenKey(v: StateView): string {
  if (overlayScreen) return overlayScreen;
  if (v.needs_checkin) return "checkin";
  switch (v.phase) {
    case "calibrating": return "calibrating";
    case "focus": return "focus";
    case "snapshot": return "snapshot";
    case "break": return "break";
    default: return v.day_closed && !reopenedAfterClose ? "closed" : "idle";
  }
}
const SCREENS: Record<string, (v: StateView) => (v: StateView) => void> = {
  checkin: screenCheckin, idle: screenIdle, calibrating: screenArc, focus: screenArc,
  snapshot: screenSnapshot, break: screenBreak, shutdown: screenShutdown, closed: screenClosed, settings: screenSettings,
};
function route(): void {
  const key = screenKey(view);
  if (key !== currentKey) {
    currentKey = key;
    root.replaceChildren();
    updater = SCREENS[key](view);
  } else updater?.(view);
}

function header(title: string, extra: Node[] = []): HTMLElement {
  return h("div", { class: "top" },
    h("div", {}, h("h1", {}, title)),
    h("div", { class: "row" }, ...extra));
}
function segButtons<T extends string | number>(opts: [T, string][], initial: T, onPick: (v: T) => void): HTMLElement {
  const wrap = h("div", { class: "seg" });
  const btns = opts.map(([v, label]) => {
    const b = h("button", { class: "chip" + (v === initial ? " on" : ""), type: "button" }, label);
    b.addEventListener("click", () => {
      btns.forEach((x) => x.classList.remove("on"));
      b.classList.add("on");
      onPick(v);
    });
    return b;
  });
  wrap.append(...btns);
  return wrap;
}

// ───────────── 晨间签到（10 秒） ─────────────
function screenCheckin(_v: StateView): (v: StateView) => void {
  let energy = 3;
  const sleep = h("input", { type: "number", step: "0.5", min: "0", max: "14", value: "7" });
  root.append(
    header("早上好", [gearButton()]),
    h("div", { class: "sub" }, "10 秒签到，决定今天的深度工作预算。"),
    h("div", { class: "card" },
      h("label", {}, "昨晚睡了多久（小时）"), sleep,
      h("label", {}, "此刻精力（1 很低 – 5 很好）"),
      segButtons<number>([[1, "1"], [2, "2"], [3, "3"], [4, "4"], [5, "5"]], 3, (v) => (energy = v)),
      h("div", { class: "row", style: "margin-top:16px" },
        h("button", { class: "primary grow", on: { click: async () => setView(await run(api.checkin(parseFloat((sleep as HTMLInputElement).value) || 7, energy))) } }, "开始今天"))),
    h("div", { class: "hint" }, "睡眠 ≥7h 且精力 ≥3：3 个金块；睡眠 6–7h 或精力 2：2 个金块并下调上限；睡眠 <6h 或精力 1：1 个金块，偏执行/复盘。"),
  );
  return () => {};
}

function gearButton(): HTMLElement {
  return h("button", { class: "icon ghost", title: "设置与隐私", on: { click: () => { overlayScreen = "settings"; route(); } } }, "⚙");
}
function parkButton(): HTMLElement {
  return h("button", { class: "icon", title: "停车场（全局快捷键也可呼出）", on: { click: () => openParking() } }, "📥 停车场");
}

// ───────────── 空闲：恢复卡 + 新建块 ─────────────
function screenIdle(v: StateView): (v: StateView) => void {
  let kind: BlockKind = v.budget_gold_left > 0 ? "gold" : "silver";
  let mode: WorkMode = "solo";
  const intent = h("input", { type: "text", placeholder: "这一块要推进什么？", value: draft.get("intent") });
  const criterion = h("input", { type: "text", placeholder: "完成标准：怎样算做成？（可留空）", value: draft.get("criterion") });
  intent.addEventListener("input", () => draft.set("intent", (intent as HTMLInputElement).value));
  criterion.addEventListener("input", () => draft.set("criterion", (criterion as HTMLInputElement).value));

  const budget = h("div", { class: "sub" });
  const resumeBox = h("div");
  const insightBox = h("div", { class: "hint" });
  const doneBox = h("div");

  const start = h("button", { class: "primary grow", on: { click: async () => {
    const r = await run(api.startBlock(kind, mode, (intent as HTMLInputElement).value.trim(), (criterion as HTMLInputElement).value.trim()));
    if (r) { draft.clear("intent"); draft.clear("criterion"); setView(r); }
  } } }, "开始（先校准 1–5 分钟）");

  root.append(
    header("准备开始", [parkButton(), gearButton()]), budget, resumeBox,
    h("div", { class: "card" },
      h("h2", {}, "新的一块"),
      h("label", {}, "类型"),
      segButtons<BlockKind>([["gold", "金块"], ["silver", "银块"], ["admin", "行政"]], kind, (k) => (kind = k)),
      h("label", {}, "工作方式（用于日后对比）"),
      segButtons<WorkMode>([["solo", "独立思考"], ["ai_collab", "AI 协作"]], mode, (m) => (mode = m)),
      h("label", {}, "意图"), intent, h("label", {}, "完成标准"), criterion,
      h("div", { class: "row", style: "margin-top:14px" }, start)),
    doneBox, insightBox,
  );

  const KIND_HINT = KIND_LABEL; void KIND_HINT;
  const render = (s: StateView): void => {
    budget.textContent = s.checkin
      ? `今日预算：金块剩 ${s.budget_gold_left}，银块剩 ${s.budget_silver_left}（已按 睡眠 ${s.checkin.sleep_hours}h / 精力 ${s.checkin.energy} 计算）`
      : "";
    resumeBox.replaceChildren();
    if (s.resume && (s.resume.first_step || s.resume.stopped_at || s.resume.open_question)) {
      const r = s.resume;
      resumeBox.append(h("div", { class: "card" },
        h("div", { class: "sub" }, "上次停在这里 · 恢复卡"),
        r.first_step ? h("div", { class: "firststep" }, "第一步：" + r.first_step) : null,
        r.stopped_at ? h("p", {}, "停在：" + r.stopped_at) : null,
        r.open_question ? h("p", {}, "未决问题：" + r.open_question) : null,
        r.context.length ? h("div", { class: "ctx" }, ...r.context.map((c) => h("span", { class: "pill", title: c.value }, `${c.kind}: ${c.value}`))) : null,
        r.context.length ? h("div", { class: "row", style: "margin-top:10px" }, h("button", { on: { click: async () => {
          try { await navigator.clipboard.writeText(r.context.map((c) => `${c.kind}: ${c.value}`).join("\n")); toast("上下文已复制"); } catch { toast("无法访问剪贴板"); }
        } } }, "复制上下文")) : null));
    }
    doneBox.replaceChildren();
    if (s.budget_used_up) {
      doneBox.append(h("div", { class: "card" },
        h("div", {}, "今日深度预算已用完。可以进入收工仪式了。"),
        h("div", { class: "row", style: "margin-top:10px" }, h("button", { class: "primary", on: { click: () => { overlayScreen = "shutdown"; route(); } } }, "收工仪式"))));
    } else {
      doneBox.append(h("div", { class: "row" }, h("button", { class: "ghost", on: { click: () => { overlayScreen = "shutdown"; route(); } } }, "提前收工")));
    }
  };
  render(v);
  void api.getInsights().then((i: Insights) => {
    const parts = [`个人上限 U = ${i.personal_cap} 分钟（随你的反馈缓慢调整）`];
    if (i.peak_hours.length) parts.push(`高心流时段：${i.peak_hours.map((x) => x + ":00").join("、")}`);
    if (i.extra_block_reduction > 0) parts.push("最近心流偏低，今日不建议加块");
    insightBox.textContent = parts.join(" · ");
  }).catch(() => {});
  return render;
}

// ───────────── 校准 / 专注：环境色弧 ─────────────
function screenArc(v: StateView): (v: StateView) => void {
  const R = 100, C = 2 * Math.PI * R;
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", "0 0 240 240");
  const mk = (cls: string) => {
    const c = document.createElementNS(ns, "circle");
    c.setAttribute("cx", "120"); c.setAttribute("cy", "120"); c.setAttribute("r", String(R));
    c.setAttribute("fill", "none"); c.setAttribute("stroke-width", "14"); c.setAttribute("class", cls);
    c.setAttribute("stroke-dasharray", String(C));
    return c;
  };
  svg.append(mk("arc-bg"));
  const fg = mk("arc-fg zone-grey");
  svg.append(fg);

  const minEl = h("div", { class: "arc-min" });
  const label = h("div", { class: "arc-label" });
  const arcBox = h("div", { class: "arc-wrap" }, svg, h("div", { class: "arc-center" }, minEl, label));
  const info = h("div", { class: "sub", style: "text-align:center" });
  const cardBox = h("div");
  const actions = h("div", { class: "row wrap", style: "justify-content:center" });
  let reasonOpen = false;

  root.append(header(v.phase === "calibrating" ? "校准启动" : "专注中", [parkButton(), gearButton()]), arcBox, info, cardBox, actions);

  const landNow = async (reason: EarlyReason | null) => {
    const r = await run(api.requestLanding(reason));
    if (r) { pendingContext = r.context; setView(r.view); }
  };
  const bpBtn = h("button", { class: "ghost", on: { click: async () => setView(await run(api.markBreakpoint(!view.at_breakpoint))) } });

  const render = (s: StateView): void => {
    const b = s.block;
    const calibrating = s.phase === "calibrating";
    let progress = s.arc_progress;
    if (calibrating && b) {
      const total = Math.max(1, b.focus_start - b.start);
      progress = 1 - s.calibrate_remaining_sec / total;
    }
    fg.setAttribute("stroke-dashoffset", String(C * (1 - progress)));
    fg.setAttribute("class", `arc-fg zone-${s.arc_zone}`);
    minEl.textContent = calibrating ? fmtClock(s.calibrate_remaining_sec) : `${s.focus_min}′`;
    label.textContent = b ? (b.intent || "专注") : "";
    info.textContent = calibrating
      ? "写下目标、进入状态。可以现在就进入专注。"
      : b ? `软下限 ${b.soft_floor_min}′ · 个人上限 ${b.cap_min}′ · 硬上限 120′（鼠标悬停圆弧查看分钟数）` : "";
    bpBtn.textContent = s.at_breakpoint ? "✓ 已在自然断点（点此取消）" : "我现在到了断点";

    cardBox.replaceChildren();
    if (s.landing !== "none") {
      const msg = {
        light: "你已到个人上限，且处在自然断点——适合现在落地。",
        direct: "已明显超出个人上限。建议现在落地：写快照，然后休息。",
        strong: "已到 120 分钟硬上限。请现在落地，边际收益已经很低。",
        none: "",
      }[s.landing];
      cardBox.append(h("div", { class: "card landing" + (s.landing === "strong" ? " strong" : "") },
        h("div", {}, msg),
        h("div", { class: "row", style: "margin-top:10px" },
          h("button", { class: "primary", on: { click: () => landNow(null) } }, "写快照"),
          s.landing !== "strong" && b && !b.postponed ? h("button", { on: { click: async () => setView(await run(api.postpone())) } }, "再 10 分钟") : null)));
    }

    actions.replaceChildren();
    if (calibrating) {
      actions.append(
        h("button", { class: "primary", on: { click: async () => setView(await run(api.skipCalibration())) } }, "直接进入专注"),
        h("button", { class: "danger ghost", on: { click: async () => setView(await run(api.abortBlock())) } }, "放弃这一块"));
    } else {
      if (s.landing === "none") {
        if (s.soft_floor_reached) actions.append(h("button", { class: "primary", on: { click: () => landNow(null) } }, "落地（写快照）"));
        else if (!reasonOpen) actions.append(h("button", { on: { click: () => { reasonOpen = true; render(view); } } }, "提前结束…"));
        else {
          actions.append(h("span", { class: "sub" }, "提前结束的原因："),
            ...([["fatigue", "累了"], ["interrupted", "被打断"], ["done", "做完了"], ["other", "其他"]] as [EarlyReason, string][])
              .map(([r, t]) => h("button", { on: { click: () => landNow(r) } }, t)),
            h("button", { class: "ghost", on: { click: () => { reasonOpen = false; render(view); } } }, "取消"));
        }
      }
      actions.append(bpBtn);
    }
  };
  render(v);
  return render;
}

// ───────────── 落地快照（≤60 秒） ─────────────
function screenSnapshot(v: StateView): (v: StateView) => void {
  const ctx: ContextItem[] = pendingContext.map((c) => ({ ...c }));
  let flow: number | null = null, fatigue: number | null = null, completion: string | null = null;
  const stopped = h("textarea", { placeholder: "停在哪里了？", value: draft.get("stopped") });
  const question = h("textarea", { placeholder: "脑子里还没写下来的想法 / 未决问题", value: draft.get("question") });
  const first = h("textarea", { placeholder: "回来后的第一步（如果…就…）", value: draft.get("first") });
  for (const [el, k] of [[stopped, "stopped"], [question, "question"], [first, "first"]] as const) {
    el.addEventListener("input", () => draft.set(k, (el as HTMLTextAreaElement).value));
  }
  const ctxBox = h("div", { class: "ctx" });
  const renderCtx = () => {
    ctxBox.replaceChildren(...ctx.filter((c) => c.keep).map((c) => {
      const x = h("button", { title: "删除这一条", type: "button" }, "×");
      x.addEventListener("click", () => { c.keep = false; renderCtx(); });
      return h("span", { class: "pill", title: c.value }, `${c.kind}: ${c.value}`, x);
    }));
  };
  renderCtx();
  const slider = (cb: (n: number) => void) => {
    const out = h("span", { class: "pill" }, "—");
    const r = h("input", { type: "range", min: "1", max: "5", step: "1", value: "3" });
    r.addEventListener("input", () => { const n = parseInt((r as HTMLInputElement).value, 10); out.textContent = String(n); cb(n); });
    return h("div", { class: "row" }, r, out);
  };
  const submit = async (skip: boolean) => {
    const r = await run(api.submitSnapshot({
      stopped_at: skip ? "" : (stopped as HTMLTextAreaElement).value.trim(),
      open_question: skip ? "" : (question as HTMLTextAreaElement).value.trim(),
      first_step: skip ? "" : (first as HTMLTextAreaElement).value.trim(),
      flow, fatigue, completion, context: skip ? [] : ctx,
    }));
    if (r) { draft.clear("stopped"); draft.clear("question"); draft.clear("first"); pendingContext = []; setView(r); }
  };
  root.addEventListener("keydown", onKey);
  function onKey(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") { e.preventDefault(); void submit(false); }
  }
  const stopListening = new MutationObserver(() => { if (!root.contains(stopped)) { root.removeEventListener("keydown", onKey); stopListening.disconnect(); } });
  stopListening.observe(root, { childList: true });

  root.append(
    header("落地快照", [parkButton()]),
    h("div", { class: "sub" }, "60 秒内写完即可，所有项都可以跳过。⌘/Ctrl + Enter 提交。"),
    h("div", { class: "card" },
      h("label", {}, "停在哪里"), stopped, h("label", {}, "未写下的想法 / 未决问题"), question, h("label", {}, "回来后的第一步"), first,
      h("label", {}, "心流 1–5（可不动表示不评）"), slider((n) => (flow = n)),
      h("label", {}, "疲劳 1–5"), slider((n) => (fatigue = n)),
      h("label", {}, "完成标准达成了吗"),
      segButtons<string>([["yes", "达成"], ["partial", "部分"], ["no", "未达成"]], "", (c) => (completion = c)),
      ctx.length ? h("label", {}, "自动捕获的上下文（可删除，仅存本机）") : null, ctx.length ? ctxBox : null),
    h("div", { class: "row" },
      h("button", { class: "primary grow", on: { click: () => submit(false) } }, "提交并休息"),
      h("button", { class: "ghost", on: { click: () => submit(true) } }, "跳过")),
  );
  void v;
  return () => {};
}

// ───────────── 分型休息 ─────────────
function screenBreak(v: StateView): (v: StateView) => void {
  let occupied = false, actual: BreakKind | null = null;
  const timeEl = h("div", { class: "break-time" });
  const kindEl = h("h2", {});
  const advEl = h("p", {});
  const actions = h("div", { class: "row wrap", style: "justify-content:center" });
  const bpBox = h("div", { class: "row wrap", style: "justify-content:center;margin:14px 0" });
  const occ = h("input", { type: "checkbox" });
  occ.addEventListener("change", () => (occupied = (occ as HTMLInputElement).checked));

  bpBox.append(h("span", { class: "sub" }, "这次的状态："),
    h("button", { on: { click: async () => setView(await run(api.represcribe(true, false))) } }, "卡壳了"),
    h("button", { on: { click: async () => setView(await run(api.represcribe(false, true))) } }, "紧绷"));

  root.append(
    header("休息", [parkButton()]),
    h("div", { class: "break-screen" }, timeEl, kindEl, advEl, bpBox,
      h("label", { style: "text-align:center" }, occ, " 我中途被占用了（看了消息/信息流）"),
      h("div", { class: "hint" }, "休息里不开会产生新待办的东西。"),
      h("div", { style: "margin-top:20px" }, actions)));

  const render = (s: StateView): void => {
    const rem = s.break_remaining_sec;
    timeEl.textContent = rem >= 0 ? fmtClock(rem) : "+" + fmtClock(-rem);
    timeEl.style.color = rem < 0 ? "var(--amber)" : "";
    const k = s.break_kind ?? "free";
    actual = k;
    kindEl.textContent = `${BREAK_NAME[k]} · ${s.break_planned_min} 分钟`;
    advEl.textContent = s.break_advice ?? "";
    actions.replaceChildren(
      h("button", { class: "primary", on: { click: async () => setView(await run(api.endBreak(actual, occupied))) } }, rem <= 0 ? "休息结束" : "提前结束休息"));
  };
  render(v);
  return render;
}

// ───────────── 收工仪式 ─────────────
function screenShutdown(_v: StateView): (v: StateView) => void {
  let fatigue: number | null = null;
  const body = h("div");
  root.append(header("收工仪式", [parkButton()]), h("div", { class: "sub" }, "5 分钟：清空脑中的开放循环，给明天留一个清晰的起点。"), body);
  void Promise.all([api.getSummary(), api.listParking()]).then(([sum, park]) => {
    body.append(
      h("div", { class: "card" }, h("h2", {}, "今日小结"),
        stat("完成块数", `${sum.blocks}（金 ${sum.gold_done} / 银 ${sum.silver_done}）`),
        stat("深度专注", `${sum.deep_minutes} 分钟`),
        stat("在断点落地", `${sum.breakpoint_landings} / ${sum.blocks}`),
        stat("平均心流", sum.mean_flow ? sum.mean_flow.toFixed(1) : "—")),
      h("div", { class: "card" }, h("h2", {}, "停车场（明天处理）"), parkingList(park)),
      h("div", { class: "card" }, h("label", {}, "此刻的疲劳 1–5"),
        segButtons<number>([[1, "1"], [2, "2"], [3, "3"], [4, "4"], [5, "5"]], 0, (n) => (fatigue = n))),
      h("div", { class: "row" },
        h("button", { class: "primary grow", on: { click: async () => {
          if (await run(api.closeDay(fatigue))) { overlayScreen = null; reopenedAfterClose = false; setView(await run(api.getState())); }
        } } }, "收工"),
        h("button", { class: "ghost", on: { click: () => { overlayScreen = null; route(); } } }, "返回")));
  });
  return () => {};
}
function stat(k: string, v: string): HTMLElement {
  return h("div", { class: "stat" }, h("span", {}, k), h("span", {}, v));
}
function parkingList(items: ParkingItem[]): HTMLElement {
  if (!items.length) return h("div", { class: "sub" }, "空空如也。");
  return h("ul", { class: "plain" }, ...items.map((p) => {
    const li = h("li", {}, h("span", { class: "grow" }, p.text));
    const done = h("button", { class: "icon ghost", title: "已处理" }, "✓");
    done.addEventListener("click", async () => { await run(api.completeParking(p.id)); li.remove(); });
    li.append(done);
    return li;
  }));
}

function screenClosed(v: StateView): (v: StateView) => void {
  const box = h("div");
  root.append(header("今天已收工", [gearButton()]), box);
  void api.getSummary().then((sum) => box.append(
    h("div", { class: "card" }, stat("深度专注", `${sum.deep_minutes} 分钟`), stat("完成块数", String(sum.blocks)),
      stat("末疲劳", v.checkin?.end_fatigue ? String(v.checkin.end_fatigue) : "—")),
    h("div", { class: "row" }, h("button", { on: { click: () => { reopenedAfterClose = true; route(); } } }, "还想再来一块"))));
  return () => {};
}

// ───────────── 设置与隐私 ─────────────
function screenSettings(_v: StateView): (v: StateView) => void {
  const body = h("div");
  root.append(header("设置与隐私", [h("button", { class: "ghost", on: { click: () => { overlayScreen = null; route(); } } }, "返回")]), body);
  void api.getSettings().then((s: Settings) => {
    const num = (v: number, min: number, max: number) => h("input", { type: "number", min: String(min), max: String(max), value: String(v) });
    const cap = num(s.cap_min, 60, 120), floor = num(s.soft_floor_min, 30, 60), cal = num(s.calibrate_min, 1, 5), ret = num(s.context_retention_days, 0, 365);
    const repo = h("input", { type: "text", value: s.repo_path, placeholder: "可选：用于抓取 git 分支与改动摘要的仓库目录" });
    const sens = h("input", { type: "checkbox", checked: s.sensing_enabled });
    const capt = h("input", { type: "checkbox", checked: s.capture_context });
    const v = (e: HTMLElement) => parseInt((e as HTMLInputElement).value, 10);
    body.append(
      h("div", { class: "card" }, h("h2", {}, "节律"),
        h("label", {}, "个人上限初值 U（60–120 分钟，之后会按反馈自动微调）"), cap,
        h("label", {}, "软下限（30–60 分钟）"), floor,
        h("label", {}, "校准启动（1–5 分钟）"), cal,
        h("div", { class: "hint" }, "硬上限固定为 120 分钟。")),
      h("div", { class: "card" }, h("h2", {}, "隐私"),
        h("label", {}, sens, " 启用空闲/前台应用感知（只读“是否活跃”和应用名，不记录按键内容、截图、剪贴板）"),
        h("label", {}, capt, " 落地时自动捕获窗口标题与 git 状态（每项可删除）"),
        h("label", {}, "上下文保留天数（0 = 不保留）"), ret,
        h("label", {}, "git 仓库目录"), repo,
        h("div", { class: "hint" }, "全局快捷键（呼出停车场）：" + s.parking_shortcut + "；修改后需重启生效。")),
      h("div", { class: "row" }, h("button", { class: "primary grow", on: { click: async () => {
        const saved = await run(api.saveSettings({
          ...s, cap_min: v(cap) || 90, soft_floor_min: v(floor) || 45, calibrate_min: v(cal) || 3,
          context_retention_days: Number.isNaN(v(ret)) ? 30 : v(ret), repo_path: (repo as HTMLInputElement).value.trim(),
          sensing_enabled: (sens as HTMLInputElement).checked, capture_context: (capt as HTMLInputElement).checked,
        }));
        if (saved) toast("已保存");
      } } }, "保存")),
      h("div", { class: "card" }, h("h2", {}, "数据"),
        h("div", { class: "row wrap" },
          h("button", { on: { click: async () => { const p = await run(api.exportData("json")); if (p) toast("已导出：" + p); } } }, "导出 JSON"),
          h("button", { on: { click: async () => { const p = await run(api.exportData("csv")); if (p) toast("已导出：" + p); } } }, "导出 CSV"),
          h("button", { class: "danger", on: { click: async () => {
            if (window.confirm("将永久删除所有本地数据（记录、快照、设置），无法恢复。继续？")) {
              const r = await run(api.deleteAllData()); if (r) { overlayScreen = null; toast("已清空"); setView(r); }
            }
          } } }, "删除全部数据"))),
    );
  });
  return () => {};
}

// ───────────── 停车场（全局快捷键 / 按钮） ─────────────
function openParking(): void {
  if (document.querySelector(".overlay")) return;
  const input = h("input", { type: "text", placeholder: "一句话记下，回车后回到刚才的事" });
  const close = () => ov.remove();
  const ov = h("div", { class: "overlay", on: { click: (e: Event) => { if (e.target === ov) close(); } } },
    h("div", { class: "card" }, h("h2", {}, "停车场"), input, h("div", { class: "hint" }, "Enter 保存并关闭 · Esc 取消")));
  input.addEventListener("keydown", async (e) => {
    if (e.key === "Escape") close();
    if (e.key === "Enter") {
      const t = (input as HTMLInputElement).value.trim();
      if (t) { await run(api.addParking(t)); toast("已记入停车场"); }
      close();
    }
  });
  document.body.append(ov);
  input.focus();
}

// ───────────── 启动 ─────────────
async function boot(): Promise<void> {
  try {
    view = await api.getState();
  } catch (e) {
    root.append(h("div", { class: "card" }, "无法连接后端。请使用 `npm run tauri dev` 启动（不能直接在浏览器中打开）。", h("div", { class: "hint" }, errMsg(e))));
    return;
  }
  route();
  window.setInterval(async () => {
    try { view = await api.getState(); route(); } catch { /* 忽略瞬时错误 */ }
  }, 1000);
  await listen("parking-hotkey", () => openParking());
  window.addEventListener("keydown", (e) => {
    if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.code === "Space") { e.preventDefault(); openParking(); }
  });
}
void boot();
