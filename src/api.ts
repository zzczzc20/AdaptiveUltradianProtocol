import { invoke } from "@tauri-apps/api/core";
import type {
  BlockKind, BreakKind, DaySummary, EarlyReason, Insights, LandingResult, ParkingItem, Settings,
  SnapshotInput, StateView, WorkMode,
} from "./types";

export const api = {
  getState: () => invoke<StateView>("get_state"),
  checkin: (sleepHours: number, energy: number) => invoke<StateView>("submit_checkin", { sleepHours, energy }),
  startBlock: (kind: BlockKind, mode: WorkMode, intent: string, criterion: string) =>
    invoke<StateView>("start_block", { kind, mode, intent, criterion }),
  skipCalibration: () => invoke<StateView>("skip_calibration"),
  abortBlock: () => invoke<StateView>("abort_block"),
  markBreakpoint: (value: boolean) => invoke<StateView>("mark_breakpoint", { value }),
  postpone: () => invoke<StateView>("postpone"),
  requestLanding: (reason: EarlyReason | null) => invoke<LandingResult>("request_landing", { reason }),
  submitSnapshot: (input: SnapshotInput) => invoke<StateView>("submit_snapshot", { input }),
  represcribe: (stuck: boolean, tense: boolean) => invoke<StateView>("represcribe", { stuck, tense }),
  endBreak: (actual: BreakKind | null, occupied: boolean) => invoke<StateView>("end_break", { actual, occupied }),
  addParking: (text: string) => invoke<void>("add_parking", { text }),
  listParking: () => invoke<ParkingItem[]>("list_parking"),
  completeParking: (id: number) => invoke<void>("complete_parking", { id }),
  closeDay: (endFatigue: number | null) => invoke<DaySummary>("close_day", { endFatigue }),
  getSummary: () => invoke<DaySummary>("get_summary"),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  getInsights: () => invoke<Insights>("get_insights"),
  exportData: (format: "json" | "csv") => invoke<string>("export_data", { format }),
  deleteAllData: () => invoke<StateView>("delete_all_data"),
};
