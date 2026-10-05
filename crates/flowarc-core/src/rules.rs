//! 规则引擎（MVP 层）：全部为纯函数。

use crate::model::BreakKind;
use serde::{Deserialize, Serialize};

pub const CAP_MIN: u32 = 60;
pub const CAP_MAX: u32 = 120;
pub const HARD_CEILING: u32 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    pub gold: u8,
    pub silver: u8,
    /// 对个人上限 U 的当日修正（分钟）
    pub cap_delta: i32,
    /// 低能量日：建议执行/复盘型任务
    pub light_day: bool,
    pub suggest_nap: bool,
}

/// 按睡眠与精力决定当日预算。
pub fn daily_budget(sleep_hours: f32, energy: u8) -> Budget {
    if sleep_hours < 6.0 || energy <= 1 {
        Budget { gold: 1, silver: 1, cap_delta: -15, light_day: true, suggest_nap: true }
    } else if sleep_hours < 7.0 || energy == 2 {
        Budget { gold: 2, silver: 1, cap_delta: -15, light_day: false, suggest_nap: false }
    } else {
        Budget { gold: 3, silver: 2, cap_delta: 0, light_day: false, suggest_nap: false }
    }
}

/// 一个已完成块的摘要，用于调整个人上限。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockOutcome {
    pub focus_min: u32,
    pub cap_min: u32,
    pub flow: Option<u8>,
    pub fatigue: Option<u8>,
    /// 完成标准是否达成（Some(false) 表示未达成）
    pub criterion_met: Option<bool>,
}

/// 根据最近的块结果调整个人上限 U。
/// `recent` 按时间由旧到新排列。
pub fn adjust_cap(current: u32, recent: &[BlockOutcome]) -> u32 {
    let current = current.clamp(CAP_MIN, CAP_MAX);
    let Some(last) = recent.last() else { return current };

    // 下调：疲劳 ≥4，或临近 U 时完成标准未达成
    let near_cap = last.focus_min + 10 >= last.cap_min;
    if last.fatigue.map_or(false, |f| f >= 4) || (near_cap && last.criterion_met == Some(false)) {
        return current.saturating_sub(5).clamp(CAP_MIN, CAP_MAX);
    }

    // 上调：连续 3 块都跑到 U 且 flow ≥4、fatigue ≤2
    if recent.len() >= 3 {
        let ok = recent[recent.len() - 3..].iter().all(|b| {
            b.focus_min + 3 >= b.cap_min
                && b.flow.map_or(false, |f| f >= 4)
                && b.fatigue.map_or(false, |f| f <= 2)
        });
        if ok {
            return (current + 5).clamp(CAP_MIN, CAP_MAX);
        }
    }
    current
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LandingLevel {
    None,
    /// ≥U 且在断点：轻提示
    Light,
    /// ≥U+10：直接提示
    Direct,
    /// ≥120：强提示
    Strong,
}

pub fn landing_level(focus_min: u32, cap_min: u32, at_breakpoint: bool, postponed: bool) -> LandingLevel {
    if focus_min >= HARD_CEILING {
        return LandingLevel::Strong;
    }
    // 推迟一次后，直接提示的门槛顺延 10 分钟
    let direct_at = cap_min + 10 + if postponed { 10 } else { 0 };
    if focus_min >= direct_at {
        return LandingLevel::Direct;
    }
    if focus_min >= cap_min && (at_breakpoint || postponed) {
        return LandingLevel::Light;
    }
    LandingLevel::None
}

/// 休息时长（分钟），随专注时长缩放。
pub fn break_minutes(focus_min: u32) -> u32 {
    match focus_min {
        0..=44 => 5,
        45..=60 => 10,
        61..=90 => 15,
        _ => 20,
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BreakSignals {
    pub fatigue: Option<u8>,
    /// 被同一障碍卡住的分钟数
    pub stuck_min: u32,
    /// 连续久坐分钟数
    pub sedentary_min: u32,
    pub tense: bool,
}

/// 休息处方：疲劳 > 卡壳 > 久坐 > 紧绷 > 自由。
pub fn recommend_break(s: BreakSignals) -> BreakKind {
    if s.fatigue.map_or(false, |f| f >= 4) {
        BreakKind::Rest
    } else if s.stuck_min >= 20 {
        BreakKind::Walk
    } else if s.sedentary_min >= 60 {
        BreakKind::Move
    } else if s.tense {
        BreakKind::Calm
    } else {
        BreakKind::Free
    }
}

/// 休息建议文案（中文）。
pub fn break_advice(kind: BreakKind) -> &'static str {
    match kind {
        BreakKind::Rest => "闭上眼睛休息，或小睡 10 分钟。",
        BreakKind::Walk => "起身散步 10–15 分钟，带着刚才卡住的那个问题，不必刻意去想。",
        BreakKind::Move => "起身轻度走动 5 分钟以上，活动一下髋部和肩颈。",
        BreakKind::Calm => "做 5 分钟循环叹息（鼻吸两次、口呼一次），或看看窗外的绿色。",
        BreakKind::Free => "自由安排。避免打开会产生新待办的东西：消息、邮件、信息流、短视频。",
    }
}

/// 次日加块修正：最近两天心流均值 < 2.5 时，下调次日额外块数。
pub fn next_day_extra_reduction(last_two_days_flow_mean: Option<f32>) -> u8 {
    match last_two_days_flow_mean {
        Some(m) if m < 2.5 => 1,
        _ => 0,
    }
}

/// 按小时统计心流均值，找出峰值时段（至少需要 `min_samples` 个样本）。
pub fn peak_hours(samples: &[(u8, u8)], min_samples: usize, top: usize) -> Vec<u8> {
    let mut sum = [0u32; 24];
    let mut cnt = [0u32; 24];
    for &(h, f) in samples {
        if h < 24 {
            sum[h as usize] += f as u32;
            cnt[h as usize] += 1;
        }
    }
    let mut v: Vec<(u8, f32)> = (0..24)
        .filter(|&h| cnt[h] as usize >= min_samples)
        .map(|h| (h as u8, sum[h] as f32 / cnt[h] as f32))
        .collect();
    v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then(a.0.cmp(&b.0)));
    v.into_iter().take(top).map(|x| x.0).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_tiers() {
        assert_eq!(daily_budget(7.5, 3).gold, 3);
        assert_eq!(daily_budget(7.0, 3).gold, 3);
        let b = daily_budget(6.5, 4);
        assert_eq!((b.gold, b.cap_delta), (2, -15));
        assert_eq!(daily_budget(8.0, 2).gold, 2);
        let low = daily_budget(5.0, 5);
        assert!(low.light_day && low.suggest_nap && low.gold == 1);
        assert_eq!(daily_budget(8.0, 1).gold, 1);
    }

    fn ob(focus: u32, cap: u32, flow: u8, fat: u8) -> BlockOutcome {
        BlockOutcome { focus_min: focus, cap_min: cap, flow: Some(flow), fatigue: Some(fat), criterion_met: Some(true) }
    }

    #[test]
    fn cap_decreases_on_fatigue() {
        assert_eq!(adjust_cap(90, &[ob(80, 90, 3, 4)]), 85);
    }

    #[test]
    fn cap_decreases_when_criterion_unmet_near_cap() {
        let mut b = ob(88, 90, 3, 2);
        b.criterion_met = Some(false);
        assert_eq!(adjust_cap(90, &[b]), 85);
        let mut far = ob(40, 90, 3, 2);
        far.criterion_met = Some(false);
        assert_eq!(adjust_cap(90, &[far]), 90);
    }

    #[test]
    fn cap_increases_after_three_good_blocks() {
        let r = [ob(90, 90, 4, 2), ob(90, 90, 5, 1), ob(91, 90, 4, 2)];
        assert_eq!(adjust_cap(90, &r), 95);
        assert_eq!(adjust_cap(90, &r[1..]), 90);
    }

    #[test]
    fn cap_is_clamped() {
        assert_eq!(adjust_cap(60, &[ob(60, 60, 3, 5)]), 60);
        let r = [ob(120, 120, 5, 1), ob(120, 120, 5, 1), ob(120, 120, 5, 1)];
        assert_eq!(adjust_cap(120, &r), 120);
        assert_eq!(adjust_cap(200, &[]), 120);
    }

    #[test]
    fn landing_levels() {
        use LandingLevel::*;
        assert_eq!(landing_level(60, 90, true, false), None);
        assert_eq!(landing_level(90, 90, false, false), None);
        assert_eq!(landing_level(90, 90, true, false), Light);
        assert_eq!(landing_level(100, 90, false, false), Direct);
        assert_eq!(landing_level(100, 90, false, true), Light);
        assert_eq!(landing_level(110, 90, false, true), Direct);
        assert_eq!(landing_level(120, 90, false, false), Strong);
    }

    #[test]
    fn break_length_scales() {
        assert_eq!(break_minutes(45), 10);
        assert_eq!(break_minutes(60), 10);
        assert_eq!(break_minutes(75), 15);
        assert_eq!(break_minutes(90), 15);
        assert_eq!(break_minutes(91), 20);
        assert_eq!(break_minutes(30), 5);
    }

    #[test]
    fn break_prescription_priority() {
        let s = BreakSignals { fatigue: Some(5), stuck_min: 30, sedentary_min: 90, tense: true };
        assert_eq!(recommend_break(s), BreakKind::Rest);
        let s = BreakSignals { fatigue: Some(2), stuck_min: 30, sedentary_min: 90, tense: true };
        assert_eq!(recommend_break(s), BreakKind::Walk);
        let s = BreakSignals { sedentary_min: 90, tense: true, ..Default::default() };
        assert_eq!(recommend_break(s), BreakKind::Move);
        let s = BreakSignals { tense: true, ..Default::default() };
        assert_eq!(recommend_break(s), BreakKind::Calm);
        assert_eq!(recommend_break(BreakSignals::default()), BreakKind::Free);
    }

    #[test]
    fn peak_hours_need_samples() {
        let s = [(9, 5), (9, 4), (14, 2), (14, 3), (20, 5)];
        assert_eq!(peak_hours(&s, 2, 3), vec![9, 14]);
    }

    #[test]
    fn extra_block_reduction() {
        assert_eq!(next_day_extra_reduction(Some(2.0)), 1);
        assert_eq!(next_day_extra_reduction(Some(3.0)), 0);
        assert_eq!(next_day_extra_reduction(None), 0);
    }
}
