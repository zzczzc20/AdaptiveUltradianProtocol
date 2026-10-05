use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    Gold,
    Silver,
    Admin,
}
impl BlockKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            BlockKind::Gold => "gold",
            BlockKind::Silver => "silver",
            BlockKind::Admin => "admin",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "silver" => BlockKind::Silver,
            "admin" => BlockKind::Admin,
            _ => BlockKind::Gold,
        }
    }
}

/// 独立思考 / AI 协作 标签（用于日后自我实验对比）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkMode {
    Solo,
    AiCollab,
}
impl WorkMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkMode::Solo => "solo",
            WorkMode::AiCollab => "ai_collab",
        }
    }
    pub fn parse(s: &str) -> Self {
        if s == "ai_collab" {
            WorkMode::AiCollab
        } else {
            WorkMode::Solo
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndMode {
    /// 在自然断点落地
    Breakpoint,
    /// 到达硬上限强制落地
    Ceiling,
    /// 软下限前提前结束
    Early,
}
impl EndMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            EndMode::Breakpoint => "breakpoint",
            EndMode::Ceiling => "ceiling",
            EndMode::Early => "early",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "ceiling" => EndMode::Ceiling,
            "early" => EndMode::Early,
            _ => EndMode::Breakpoint,
        }
    }
}

/// 提前结束的原因
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EarlyReason {
    Fatigue,
    Interrupted,
    Done,
    Other,
}
impl EarlyReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            EarlyReason::Fatigue => "fatigue",
            EarlyReason::Interrupted => "interrupted",
            EarlyReason::Done => "done",
            EarlyReason::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Completion {
    Yes,
    Partial,
    No,
}
impl Completion {
    pub fn as_str(&self) -> &'static str {
        match self {
            Completion::Yes => "yes",
            Completion::Partial => "partial",
            Completion::No => "no",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "yes" => Some(Completion::Yes),
            "partial" => Some(Completion::Partial),
            "no" => Some(Completion::No),
            _ => None,
        }
    }
}

/// 休息类型处方
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakKind {
    /// 疲劳：闭眼 / 10 分钟小睡
    Rest,
    /// 卡壳：带着问题散步
    Walk,
    /// 久坐：轻度走动
    Move,
    /// 紧绷：5 分钟循环叹息 / 看绿色
    Calm,
    /// 无信号：自由选择
    Free,
}
impl BreakKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            BreakKind::Rest => "rest",
            BreakKind::Walk => "walk",
            BreakKind::Move => "move",
            BreakKind::Calm => "calm",
            BreakKind::Free => "free",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Block {
    pub id: i64,
    pub kind: Option<BlockKind>,
    pub mode: Option<WorkMode>,
    pub intent: String,
    pub criterion: String,
    pub start: i64,
    pub focus_start: i64,
    pub end: i64,
    pub soft_floor_min: u32,
    pub cap_min: u32,
    pub end_mode: Option<EndMode>,
    pub early_reason: Option<String>,
    pub postponed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Snapshot {
    pub block_id: i64,
    pub stopped_at: String,
    pub open_question: String,
    pub first_step: String,
    pub flow: Option<u8>,
    pub fatigue: Option<u8>,
    pub completion: Option<String>,
    pub context: Vec<ContextItem>,
    pub created: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextItem {
    /// app / window / git_branch / git_diff / url / command
    pub kind: String,
    pub value: String,
    pub keep: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BreakRecord {
    pub id: i64,
    pub block_id: i64,
    pub recommended: String,
    pub actual: Option<String>,
    pub planned_min: u32,
    pub actual_min: Option<u32>,
    pub occupied: bool,
    pub start: i64,
    pub end: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DayCheckin {
    pub date: String,
    pub sleep_hours: f32,
    pub energy: u8,
    pub gold_budget: u8,
    pub silver_budget: u8,
    pub cap_delta: i32,
    pub end_fatigue: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// 个人上限 U（分钟），默认 90，范围 60–120
    pub cap_min: u32,
    pub soft_floor_min: u32,
    pub ceiling_min: u32,
    /// 校准启动时长（分钟）1–5
    pub calibrate_min: u32,
    /// 窗口标题/URL 等上下文保留天数，0 = 不记录
    pub context_retention_days: u32,
    pub capture_context: bool,
    /// 用于抓取 git 分支/diff 的仓库目录（可选）
    pub repo_path: String,
    pub sensing_enabled: bool,
    pub parking_shortcut: String,
}
impl Default for Settings {
    fn default() -> Self {
        Settings {
            cap_min: 90,
            soft_floor_min: 45,
            ceiling_min: 120,
            calibrate_min: 3,
            context_retention_days: 30,
            capture_context: true,
            repo_path: String::new(),
            sensing_enabled: true,
            parking_shortcut: "CommandOrControl+Shift+Space".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParkingItem {
    pub id: i64,
    pub text: String,
    pub created: i64,
    pub block_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DaySummary {
    pub date: String,
    pub gold_done: u32,
    pub silver_done: u32,
    pub deep_minutes: u32,
    pub breakpoint_landings: u32,
    pub blocks: u32,
    pub mean_flow: Option<f32>,
    pub end_fatigue: Option<u8>,
    pub parking: Vec<String>,
}
