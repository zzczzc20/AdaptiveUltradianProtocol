//! 会话状态机：Idle → 校准启动 → 弹性专注 → 落地（快照）→ 分型休息 → 下一块 / 收工。
//! 所有转移都显式传入 `now`（Unix 秒），无隐式时钟。

use crate::model::*;
use crate::rules::{self, BreakSignals, Budget, LandingLevel};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Calibrating,
    Focus,
    /// 落地：写快照
    Snapshot,
    Break,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EngineError {
    #[error("请先完成今日签到")]
    NeedCheckin,
    #[error("当前阶段不允许该操作")]
    WrongPhase,
    #[error("在软下限前提前结束需要说明原因")]
    NeedReason,
    #[error("本块已推迟过一次")]
    AlreadyPostponed,
    #[error("尚未到达可推迟的落地提示")]
    NotLanding,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineState {
    pub phase: Phase,
    pub checkin: Option<DayCheckin>,
    pub gold_done: u32,
    pub silver_done: u32,
    pub block: Option<Block>,
    /// 休息/快照等阶段的起点
    pub phase_start: i64,
    pub break_kind: Option<BreakKind>,
    pub break_planned_min: u32,
    /// 最近一次落地快照，作为下一块的“恢复卡”
    pub resume: Option<Snapshot>,
    /// 由传感层设置：当前是否处于自然断点
    pub at_breakpoint: bool,
    /// 今日是否已收工
    pub day_closed: bool,
    /// 最近一次专注时长（分钟），用于休息缩放
    pub last_focus_min: u32,
}

impl Default for EngineState {
    fn default() -> Self {
        EngineState {
            phase: Phase::Idle,
            checkin: None,
            gold_done: 0,
            silver_done: 0,
            block: None,
            phase_start: 0,
            break_kind: None,
            break_planned_min: 0,
            resume: None,
            at_breakpoint: false,
            day_closed: false,
            last_focus_min: 0,
        }
    }
}

/// 给 UI 的只读视图
#[derive(Debug, Clone, Serialize)]
pub struct StateView {
    pub phase: Phase,
    pub needs_checkin: bool,
    pub checkin: Option<DayCheckin>,
    pub budget_gold_left: u32,
    pub budget_silver_left: u32,
    pub budget_used_up: bool,
    pub day_closed: bool,
    pub block: Option<Block>,
    pub focus_min: u32,
    pub calibrate_remaining_sec: i64,
    pub landing: LandingLevel,
    pub at_breakpoint: bool,
    /// 圆弧进度 0..1（相对硬上限）
    pub arc_progress: f32,
    /// grey / blue / amber / red
    pub arc_zone: &'static str,
    pub break_kind: Option<BreakKind>,
    pub break_advice: Option<&'static str>,
    pub break_planned_min: u32,
    pub break_remaining_sec: i64,
    pub resume: Option<Snapshot>,
    pub soft_floor_reached: bool,
}

pub struct Engine {
    pub state: EngineState,
}

pub struct StartParams {
    pub id: i64,
    pub kind: BlockKind,
    pub mode: WorkMode,
    pub intent: String,
    pub criterion: String,
    /// 个人上限 U（已由规则引擎调整）
    pub personal_cap: u32,
}

impl Engine {
    pub fn new(state: EngineState) -> Self {
        Engine { state }
    }

    pub fn budget(&self) -> Option<Budget> {
        self.state.checkin.as_ref().map(|c| Budget {
            gold: c.gold_budget,
            silver: c.silver_budget,
            cap_delta: c.cap_delta,
            light_day: c.gold_budget <= 1,
            suggest_nap: c.gold_budget <= 1,
        })
    }

    /// 新的一天：清空当日计数（保留最近一次快照作为恢复卡）。
    pub fn new_day(&mut self) {
        let resume = self.state.resume.take();
        self.state = EngineState { resume, ..EngineState::default() };
    }

    pub fn checkin(&mut self, date: &str, sleep_hours: f32, energy: u8) -> &DayCheckin {
        let b = rules::daily_budget(sleep_hours, energy);
        self.state.checkin = Some(DayCheckin {
            date: date.to_string(),
            sleep_hours,
            energy,
            gold_budget: b.gold,
            silver_budget: b.silver,
            cap_delta: b.cap_delta,
            end_fatigue: None,
        });
        self.state.day_closed = false;
        self.state.checkin.as_ref().unwrap()
    }

    pub fn start_block(&mut self, now: i64, p: StartParams, s: &Settings) -> Result<(), EngineError> {
        if self.state.checkin.is_none() {
            return Err(EngineError::NeedCheckin);
        }
        if self.state.phase != Phase::Idle {
            return Err(EngineError::WrongPhase);
        }
        let delta = self.state.checkin.as_ref().map_or(0, |c| c.cap_delta);
        let cap = (p.personal_cap as i32 + delta).clamp(rules::CAP_MIN as i32, rules::CAP_MAX as i32) as u32;
        let cap = cap.min(s.ceiling_min);
        self.state.block = Some(Block {
            id: p.id,
            kind: Some(p.kind),
            mode: Some(p.mode),
            intent: p.intent,
            criterion: p.criterion,
            start: now,
            focus_start: now + (s.calibrate_min.clamp(1, 5) as i64) * 60,
            end: 0,
            soft_floor_min: s.soft_floor_min,
            cap_min: cap,
            end_mode: None,
            early_reason: None,
            postponed: false,
        });
        self.state.phase = Phase::Calibrating;
        self.state.phase_start = now;
        self.state.at_breakpoint = false;
        self.state.day_closed = false;
        Ok(())
    }

    /// 推进时间：校准结束自动进入专注。
    pub fn tick(&mut self, now: i64) {
        if self.state.phase == Phase::Calibrating {
            if let Some(b) = &self.state.block {
                if now >= b.focus_start {
                    self.state.phase = Phase::Focus;
                }
            }
        }
    }

    pub fn skip_calibration(&mut self, now: i64) -> Result<(), EngineError> {
        if self.state.phase != Phase::Calibrating {
            return Err(EngineError::WrongPhase);
        }
        if let Some(b) = self.state.block.as_mut() {
            b.focus_start = now;
        }
        self.state.phase = Phase::Focus;
        Ok(())
    }

    /// 校准阶段放弃（尚未进入专注）：不计入预算，直接回到 Idle。返回被放弃的块 id。
    pub fn abort_block(&mut self) -> Result<i64, EngineError> {
        if self.state.phase != Phase::Calibrating {
            return Err(EngineError::WrongPhase);
        }
        let id = self.state.block.take().map_or(0, |b| b.id);
        self.state.phase = Phase::Idle;
        Ok(id)
    }

    pub fn focus_min(&self, now: i64) -> u32 {
        match (&self.state.block, self.state.phase) {
            (Some(b), Phase::Focus) => ((now - b.focus_start).max(0) / 60) as u32,
            (Some(b), Phase::Snapshot) | (Some(b), Phase::Break) if b.end > 0 => {
                ((b.end - b.focus_start).max(0) / 60) as u32
            }
            _ => 0,
        }
    }

    pub fn set_breakpoint(&mut self, v: bool) {
        self.state.at_breakpoint = v;
    }

    pub fn landing_level(&self, now: i64) -> LandingLevel {
        match (&self.state.block, self.state.phase) {
            (Some(b), Phase::Focus) => {
                rules::landing_level(self.focus_min(now), b.cap_min, self.state.at_breakpoint, b.postponed)
            }
            _ => LandingLevel::None,
        }
    }

    /// “再 10 分钟”：每块只能推迟一次，且只在已出现落地提示时可用。
    pub fn postpone(&mut self, now: i64) -> Result<(), EngineError> {
        if self.landing_level(now) == LandingLevel::None {
            return Err(EngineError::NotLanding);
        }
        let focus = self.focus_min(now);
        let b = self.state.block.as_mut().ok_or(EngineError::WrongPhase)?;
        if b.postponed {
            return Err(EngineError::AlreadyPostponed);
        }
        if focus >= rules::HARD_CEILING {
            return Err(EngineError::NotLanding);
        }
        b.postponed = true;
        Ok(())
    }

    /// 进入落地（写快照）。软下限前结束必须给出原因。
    pub fn request_landing(&mut self, now: i64, early_reason: Option<EarlyReason>) -> Result<(), EngineError> {
        // 校准阶段也允许直接放弃：视作 0 分钟提前结束
        if !matches!(self.state.phase, Phase::Focus | Phase::Calibrating) {
            return Err(EngineError::WrongPhase);
        }
        let focus = self.focus_min(now);
        let b = self.state.block.as_mut().ok_or(EngineError::WrongPhase)?;
        let mode = if focus >= rules::HARD_CEILING {
            EndMode::Ceiling
        } else if focus < b.soft_floor_min {
            if early_reason.is_none() {
                return Err(EngineError::NeedReason);
            }
            b.early_reason = early_reason.map(|r| r.as_str().to_string());
            EndMode::Early
        } else {
            EndMode::Breakpoint
        };
        b.end_mode = Some(mode);
        b.end = now.max(b.focus_start);
        self.state.phase = Phase::Snapshot;
        self.state.phase_start = now;
        self.state.last_focus_min = ((b.end - b.focus_start).max(0) / 60) as u32;
        Ok(())
    }

    /// 提交快照 → 自动进入休息。返回已结束的块、休息处方。
    pub fn submit_snapshot(&mut self, now: i64, mut snap: Snapshot) -> Result<(Block, BreakKind, u32), EngineError> {
        if self.state.phase != Phase::Snapshot {
            return Err(EngineError::WrongPhase);
        }
        let block = self.state.block.clone().ok_or(EngineError::WrongPhase)?;
        snap.block_id = block.id;
        snap.created = now;

        match block.kind {
            Some(BlockKind::Gold) => self.state.gold_done += 1,
            Some(BlockKind::Silver) => self.state.silver_done += 1,
            _ => {}
        }

        let focus = self.state.last_focus_min;
        let kind = rules::recommend_break(BreakSignals {
            fatigue: snap.fatigue,
            stuck_min: 0,
            sedentary_min: focus,
            tense: false,
        });
        let mins = rules::break_minutes(focus);
        self.state.resume = Some(snap);
        self.state.break_kind = Some(kind);
        self.state.break_planned_min = mins;
        self.state.phase = Phase::Break;
        self.state.phase_start = now;
        Ok((block, kind, mins))
    }

    /// 休息中允许手动指定“卡壳/紧绷”信号以重新开处方。
    pub fn represcribe(&mut self, stuck: bool, tense: bool) -> Result<BreakKind, EngineError> {
        if self.state.phase != Phase::Break {
            return Err(EngineError::WrongPhase);
        }
        let fatigue = self.state.resume.as_ref().and_then(|s| s.fatigue);
        let kind = rules::recommend_break(BreakSignals {
            fatigue,
            stuck_min: if stuck { 20 } else { 0 },
            sedentary_min: self.state.last_focus_min,
            tense,
        });
        self.state.break_kind = Some(kind);
        Ok(kind)
    }

    pub fn break_remaining_sec(&self, now: i64) -> i64 {
        if self.state.phase != Phase::Break {
            return 0;
        }
        self.state.phase_start + self.state.break_planned_min as i64 * 60 - now
    }

    /// 结束休息回到 Idle。`occupied` 表示休息中是否被占用（切去做事/看信息）。
    pub fn end_break(&mut self, now: i64, actual: Option<BreakKind>, occupied: bool) -> Result<BreakRecord, EngineError> {
        if self.state.phase != Phase::Break {
            return Err(EngineError::WrongPhase);
        }
        let b = self.state.block.take().ok_or(EngineError::WrongPhase)?;
        let rec = BreakRecord {
            id: 0,
            block_id: b.id,
            recommended: self.state.break_kind.map_or("free", |k| k.as_str()).to_string(),
            actual: actual.map(|k| k.as_str().to_string()),
            planned_min: self.state.break_planned_min,
            actual_min: Some(((now - self.state.phase_start).max(0) / 60) as u32),
            occupied,
            start: self.state.phase_start,
            end: now,
        };
        self.state.phase = Phase::Idle;
        self.state.break_kind = None;
        self.state.phase_start = now;
        Ok(rec)
    }

    pub fn budget_used_up(&self) -> bool {
        match &self.state.checkin {
            Some(c) => self.state.gold_done >= c.gold_budget as u32 && self.state.silver_done >= c.silver_budget as u32,
            None => false,
        }
    }

    /// 收工仪式：记录当日末疲劳并关闭当天。
    pub fn close_day(&mut self, end_fatigue: Option<u8>) -> Result<Option<DayCheckin>, EngineError> {
        if self.state.phase != Phase::Idle {
            return Err(EngineError::WrongPhase);
        }
        if let Some(c) = self.state.checkin.as_mut() {
            c.end_fatigue = end_fatigue;
        }
        self.state.day_closed = true;
        Ok(self.state.checkin.clone())
    }

    pub fn view(&self, now: i64) -> StateView {
        let s = &self.state;
        let focus = self.focus_min(now);
        let landing = self.landing_level(now);
        let (gl, sl) = match &s.checkin {
            Some(c) => (
                (c.gold_budget as u32).saturating_sub(s.gold_done),
                (c.silver_budget as u32).saturating_sub(s.silver_done),
            ),
            None => (0, 0),
        };
        let zone = match (s.phase, landing) {
            (Phase::Calibrating, _) => "grey",
            (Phase::Focus, LandingLevel::Strong) => "red",
            (Phase::Focus, LandingLevel::None) => "blue",
            (Phase::Focus, _) => "amber",
            _ => "grey",
        };
        let ceiling = rules::HARD_CEILING as f32;
        StateView {
            phase: s.phase,
            needs_checkin: s.checkin.is_none(),
            checkin: s.checkin.clone(),
            budget_gold_left: gl,
            budget_silver_left: sl,
            budget_used_up: self.budget_used_up(),
            day_closed: s.day_closed,
            block: s.block.clone(),
            focus_min: focus,
            calibrate_remaining_sec: match (&s.block, s.phase) {
                (Some(b), Phase::Calibrating) => (b.focus_start - now).max(0),
                _ => 0,
            },
            landing,
            at_breakpoint: s.at_breakpoint,
            arc_progress: (focus as f32 / ceiling).min(1.0),
            arc_zone: zone,
            break_kind: s.break_kind,
            break_advice: s.break_kind.map(rules::break_advice),
            break_planned_min: s.break_planned_min,
            break_remaining_sec: self.break_remaining_sec(now),
            resume: s.resume.clone(),
            soft_floor_reached: s.block.as_ref().map_or(false, |b| focus >= b.soft_floor_min),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(id: i64, kind: BlockKind) -> StartParams {
        StartParams { id, kind, mode: WorkMode::Solo, intent: "写第三节".into(), criterion: "完成草稿".into(), personal_cap: 90 }
    }

    fn started() -> (Engine, Settings) {
        let s = Settings::default();
        let mut e = Engine::new(EngineState::default());
        e.checkin("2026-10-05", 7.5, 4);
        e.start_block(1_000, params(1, BlockKind::Gold), &s).unwrap();
        (e, s)
    }

    #[test]
    fn needs_checkin() {
        let mut e = Engine::new(EngineState::default());
        let r = e.start_block(0, params(1, BlockKind::Gold), &Settings::default());
        assert_eq!(r, Err(EngineError::NeedCheckin));
    }

    #[test]
    fn calibrate_then_focus() {
        let (mut e, _) = started();
        assert_eq!(e.state.phase, Phase::Calibrating);
        e.tick(1_000 + 179);
        assert_eq!(e.state.phase, Phase::Calibrating);
        e.tick(1_000 + 180);
        assert_eq!(e.state.phase, Phase::Focus);
    }

    #[test]
    fn abort_during_calibration_is_free() {
        let (mut e, _) = started();
        assert_eq!(e.abort_block(), Ok(1));
        assert_eq!(e.state.phase, Phase::Idle);
        assert_eq!((e.state.gold_done, e.state.silver_done), (0, 0));
        e.skip_calibration(0).unwrap_err();
    }

    #[test]
    fn early_stop_needs_reason() {
        let (mut e, _) = started();
        e.skip_calibration(1_000).unwrap();
        let now = 1_000 + 20 * 60;
        assert_eq!(e.request_landing(now, None), Err(EngineError::NeedReason));
        e.request_landing(now, Some(EarlyReason::Interrupted)).unwrap();
        assert_eq!(e.state.block.as_ref().unwrap().end_mode, Some(EndMode::Early));
        assert_eq!(e.state.phase, Phase::Snapshot);
    }

    #[test]
    fn full_cycle_with_budget_and_break() {
        let (mut e, _) = started();
        e.skip_calibration(1_000).unwrap();
        let end = 1_000 + 80 * 60;
        e.set_breakpoint(true);
        e.request_landing(end, None).unwrap();
        assert_eq!(e.state.block.as_ref().unwrap().end_mode, Some(EndMode::Breakpoint));
        let snap = Snapshot { fatigue: Some(4), flow: Some(4), first_step: "改标题".into(), ..Default::default() };
        let (blk, kind, mins) = e.submit_snapshot(end + 30, snap).unwrap();
        assert_eq!(blk.id, 1);
        assert_eq!(kind, BreakKind::Rest); // 疲劳 ≥4
        assert_eq!(mins, 15);
        assert_eq!(e.state.gold_done, 1);
        assert_eq!(e.break_remaining_sec(end + 30), 15 * 60);
        let rec = e.end_break(end + 30 + 14 * 60, Some(BreakKind::Rest), false).unwrap();
        assert_eq!(rec.actual_min, Some(14));
        assert_eq!(e.state.phase, Phase::Idle);
        assert_eq!(e.state.resume.as_ref().unwrap().first_step, "改标题");
    }

    #[test]
    fn postpone_once_only() {
        let (mut e, _) = started();
        e.skip_calibration(1_000).unwrap();
        e.set_breakpoint(true);
        let t = 1_000 + 90 * 60;
        assert_eq!(e.landing_level(t), LandingLevel::Light);
        e.postpone(t).unwrap();
        assert_eq!(e.postpone(t), Err(EngineError::AlreadyPostponed));
    }

    #[test]
    fn cannot_postpone_before_landing() {
        let (mut e, _) = started();
        e.skip_calibration(1_000).unwrap();
        assert_eq!(e.postpone(1_000 + 30 * 60), Err(EngineError::NotLanding));
    }

    #[test]
    fn hard_ceiling_forces_strong_prompt() {
        let (mut e, _) = started();
        e.skip_calibration(1_000).unwrap();
        let t = 1_000 + 120 * 60;
        assert_eq!(e.landing_level(t), LandingLevel::Strong);
        assert_eq!(e.view(t).arc_zone, "red");
        e.request_landing(t, None).unwrap();
        assert_eq!(e.state.block.as_ref().unwrap().end_mode, Some(EndMode::Ceiling));
    }

    #[test]
    fn low_energy_day_reduces_cap() {
        let s = Settings::default();
        let mut e = Engine::new(EngineState::default());
        e.checkin("d", 5.0, 3);
        e.start_block(0, params(1, BlockKind::Gold), &s).unwrap();
        assert_eq!(e.state.block.as_ref().unwrap().cap_min, 75);
        assert_eq!(e.state.checkin.as_ref().unwrap().gold_budget, 1);
    }

    #[test]
    fn budget_used_up_and_close_day() {
        let mut e = Engine::new(EngineState::default());
        e.checkin("d", 7.5, 4); // 3 金 2 银
        e.state.gold_done = 3;
        e.state.silver_done = 2;
        assert!(e.budget_used_up());
        e.close_day(Some(2)).unwrap();
        assert!(e.state.day_closed);
    }

    #[test]
    fn state_roundtrips_json() {
        let (e, _) = started();
        let j = serde_json::to_string(&e.state).unwrap();
        let s: EngineState = serde_json::from_str(&j).unwrap();
        assert_eq!(s.phase, Phase::Calibrating);
    }
}
