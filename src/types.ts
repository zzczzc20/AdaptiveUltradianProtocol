// 与 crates/flowarc-core 中的 serde 结构一一对应

export type Phase = "idle" | "calibrating" | "focus" | "snapshot" | "break";
export type BlockKind = "gold" | "silver" | "admin";
export type WorkMode = "solo" | "ai_collab";
export type LandingLevel = "none" | "light" | "direct" | "strong";
export type EarlyReason = "fatigue" | "interrupted" | "done" | "other";
export type BreakKind = "rest" | "walk" | "move" | "calm" | "free";
export type Completion = "yes" | "partial" | "no";

export interface Block {
  id: number;
  kind: BlockKind | null;
  mode: WorkMode | null;
  intent: string;
  criterion: string;
  start: number;
  focus_start: number;
  end: number;
  soft_floor_min: number;
  cap_min: number;
  postponed: boolean;
}

export interface ContextItem {
  kind: string;
  value: string;
  keep: boolean;
}

export interface Snapshot {
  block_id: number;
  stopped_at: string;
  open_question: string;
  first_step: string;
  flow: number | null;
  fatigue: number | null;
  completion: string | null;
  context: ContextItem[];
  created: number;
}

export interface DayCheckin {
  date: string;
  sleep_hours: number;
  energy: number;
  gold_budget: number;
  silver_budget: number;
  cap_delta: number;
  end_fatigue: number | null;
}

export interface StateView {
  phase: Phase;
  needs_checkin: boolean;
  checkin: DayCheckin | null;
  budget_gold_left: number;
  budget_silver_left: number;
  budget_used_up: boolean;
  day_closed: boolean;
  block: Block | null;
  focus_min: number;
  calibrate_remaining_sec: number;
  landing: LandingLevel;
  at_breakpoint: boolean;
  arc_progress: number;
  arc_zone: "grey" | "blue" | "amber" | "red";
  break_kind: BreakKind | null;
  break_advice: string | null;
  break_planned_min: number;
  break_remaining_sec: number;
  resume: Snapshot | null;
  soft_floor_reached: boolean;
}

export interface Settings {
  cap_min: number;
  soft_floor_min: number;
  ceiling_min: number;
  calibrate_min: number;
  context_retention_days: number;
  capture_context: boolean;
  repo_path: string;
  sensing_enabled: boolean;
  parking_shortcut: string;
}

export interface ParkingItem {
  id: number;
  text: string;
  created: number;
  block_id: number | null;
}

export interface DaySummary {
  date: string;
  gold_done: number;
  silver_done: number;
  deep_minutes: number;
  breakpoint_landings: number;
  blocks: number;
  mean_flow: number | null;
  end_fatigue: number | null;
  parking: string[];
}

export interface Insights {
  personal_cap: number;
  peak_hours: number[];
  flow_mean_2d: number | null;
  extra_block_reduction: number;
  landing_note: string | null;
}

export interface LandingResult {
  view: StateView;
  context: ContextItem[];
}

export interface SnapshotInput {
  stopped_at: string;
  open_question: string;
  first_step: string;
  flow: number | null;
  fatigue: number | null;
  completion: string | null;
  context: ContextItem[];
}
