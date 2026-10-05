use crate::{now_secs, roll_day, sensing, today, AppState};
use flowarc_core::{
    rules::{self, LandingLevel},
    session::{StartParams, StateView},
    BlockKind, BreakKind, ContextItem, DaySummary, EarlyReason, Settings, Snapshot, WorkMode,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

type Res<T> = Result<T, String>;
fn e<E: std::fmt::Display>(x: E) -> String {
    x.to_string()
}

#[tauri::command]
pub fn get_state(state: State<AppState>) -> Res<StateView> {
    let mut g = state.0.lock().map_err(e)?;
    roll_day(&mut g);
    let now = now_secs();
    g.engine.tick(now);
    Ok(g.engine.view(now))
}

#[tauri::command]
pub fn submit_checkin(state: State<AppState>, sleep_hours: f32, energy: u8) -> Res<StateView> {
    let mut g = state.0.lock().map_err(e)?;
    let c = g.engine.checkin(&today(), sleep_hours.clamp(0.0, 14.0), energy.clamp(1, 5)).clone();
    g.storage.save_checkin(&c).map_err(e)?;
    g.storage.save_state(&g.engine.state).map_err(e)?;
    Ok(g.engine.view(now_secs()))
}

#[tauri::command]
pub fn start_block(
    state: State<AppState>,
    kind: BlockKind,
    mode: WorkMode,
    intent: String,
    criterion: String,
) -> Res<StateView> {
    let mut g = state.0.lock().map_err(e)?;
    let settings = g.storage.settings().map_err(e)?;
    let cap = g.storage.personal_cap(settings.cap_min).map_err(e)?;
    let id = g.storage.insert_block(kind, mode, &intent, &criterion).map_err(e)?;
    let now = now_secs();
    let params = StartParams { id, kind, mode, intent, criterion, personal_cap: cap };
    if let Err(err) = g.engine.start_block(now, params, &settings) {
        let _ = g.storage.delete_block(id);
        return Err(err.to_string());
    }
    let b = g.engine.state.block.clone().ok_or("内部错误")?;
    g.storage.update_block(&b).map_err(e)?;
    g.storage.save_state(&g.engine.state).map_err(e)?;
    Ok(g.engine.view(now))
}

#[tauri::command]
pub fn skip_calibration(state: State<AppState>) -> Res<StateView> {
    let mut g = state.0.lock().map_err(e)?;
    let now = now_secs();
    g.engine.skip_calibration(now).map_err(e)?;
    if let Some(b) = g.engine.state.block.clone() {
        g.storage.update_block(&b).map_err(e)?;
    }
    g.storage.save_state(&g.engine.state).map_err(e)?;
    Ok(g.engine.view(now))
}

#[tauri::command]
pub fn abort_block(state: State<AppState>) -> Res<StateView> {
    let mut g = state.0.lock().map_err(e)?;
    let id = g.engine.abort_block().map_err(e)?;
    g.storage.delete_block(id).map_err(e)?;
    g.storage.save_state(&g.engine.state).map_err(e)?;
    Ok(g.engine.view(now_secs()))
}

/// 手动标记“当前是自然断点”（传感不可用时的降级路径）
#[tauri::command]
pub fn mark_breakpoint(state: State<AppState>, value: bool) -> Res<StateView> {
    let mut g = state.0.lock().map_err(e)?;
    g.engine.set_breakpoint(value);
    Ok(g.engine.view(now_secs()))
}

#[tauri::command]
pub fn postpone(state: State<AppState>) -> Res<StateView> {
    let mut g = state.0.lock().map_err(e)?;
    let now = now_secs();
    g.engine.postpone(now).map_err(e)?;
    if let Some(b) = g.engine.state.block.clone() {
        g.storage.update_block(&b).map_err(e)?;
    }
    g.storage.save_state(&g.engine.state).map_err(e)?;
    Ok(g.engine.view(now))
}

#[derive(Serialize)]
pub struct LandingResult {
    pub view: StateView,
    pub context: Vec<ContextItem>,
}

/// 落地：进入快照阶段，并抓取可删除的上下文候选。
#[tauri::command]
pub fn request_landing(state: State<AppState>, reason: Option<EarlyReason>) -> Res<LandingResult> {
    let mut g = state.0.lock().map_err(e)?;
    let now = now_secs();
    g.engine.request_landing(now, reason).map_err(e)?;
    if let Some(b) = g.engine.state.block.clone() {
        g.storage.update_block(&b).map_err(e)?;
    }
    g.storage.save_state(&g.engine.state).map_err(e)?;
    let settings = g.storage.settings().map_err(e)?;
    let view = g.engine.view(now);
    drop(g);
    let context = sensing::capture_context(&settings);
    Ok(LandingResult { view, context })
}

#[derive(Deserialize)]
pub struct SnapshotInput {
    pub stopped_at: String,
    pub open_question: String,
    pub first_step: String,
    pub flow: Option<u8>,
    pub fatigue: Option<u8>,
    pub completion: Option<String>,
    pub context: Vec<ContextItem>,
}

#[tauri::command]
pub fn submit_snapshot(state: State<AppState>, input: SnapshotInput) -> Res<StateView> {
    let mut g = state.0.lock().map_err(e)?;
    let now = now_secs();
    let snap = Snapshot {
        stopped_at: input.stopped_at,
        open_question: input.open_question,
        first_step: input.first_step,
        flow: input.flow.map(|v| v.clamp(1, 5)),
        fatigue: input.fatigue.map(|v| v.clamp(1, 5)),
        completion: input.completion,
        // 仅保留用户确认保留的条目
        context: input.context.into_iter().filter(|c| c.keep).collect(),
        ..Default::default()
    };
    let (_block, _kind, _mins) = g.engine.submit_snapshot(now, snap).map_err(e)?;
    if let Some(s) = g.engine.state.resume.clone() {
        g.storage.save_snapshot(&s).map_err(e)?;
    }
    // 规则引擎：根据最近 6 个块调整个人上限 U
    let settings = g.storage.settings().map_err(e)?;
    let cur = g.storage.personal_cap(settings.cap_min).map_err(e)?;
    let recent = g.storage.recent_outcomes(6).map_err(e)?;
    let new_cap = rules::adjust_cap(cur, &recent);
    if new_cap != cur {
        g.storage.set_personal_cap(new_cap).map_err(e)?;
    }
    g.storage.save_state(&g.engine.state).map_err(e)?;
    Ok(g.engine.view(now))
}

#[tauri::command]
pub fn represcribe(state: State<AppState>, stuck: bool, tense: bool) -> Res<StateView> {
    let mut g = state.0.lock().map_err(e)?;
    g.engine.represcribe(stuck, tense).map_err(e)?;
    g.storage.save_state(&g.engine.state).map_err(e)?;
    Ok(g.engine.view(now_secs()))
}

#[tauri::command]
pub fn end_break(state: State<AppState>, actual: Option<BreakKind>, occupied: bool) -> Res<StateView> {
    let mut g = state.0.lock().map_err(e)?;
    let now = now_secs();
    let rec = g.engine.end_break(now, actual, occupied).map_err(e)?;
    g.storage.insert_break(&rec).map_err(e)?;
    g.storage.save_state(&g.engine.state).map_err(e)?;
    Ok(g.engine.view(now))
}

#[tauri::command]
pub fn add_parking(state: State<AppState>, text: String) -> Res<()> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(());
    }
    let g = state.0.lock().map_err(e)?;
    let bid = g.engine.state.block.as_ref().map(|b| b.id);
    g.storage.add_parking(text, now_secs(), bid).map_err(e)?;
    Ok(())
}

#[tauri::command]
pub fn list_parking(state: State<AppState>) -> Res<Vec<flowarc_core::ParkingItem>> {
    state.0.lock().map_err(e)?.storage.list_parking(false).map_err(e)
}

#[tauri::command]
pub fn complete_parking(state: State<AppState>, id: i64) -> Res<()> {
    state.0.lock().map_err(e)?.storage.complete_parking(id).map_err(e)
}

#[tauri::command]
pub fn close_day(state: State<AppState>, end_fatigue: Option<u8>) -> Res<DaySummary> {
    let mut g = state.0.lock().map_err(e)?;
    if let Some(c) = g.engine.close_day(end_fatigue.map(|v| v.clamp(1, 5))).map_err(e)? {
        g.storage.save_checkin(&c).map_err(e)?;
    }
    g.storage.save_state(&g.engine.state).map_err(e)?;
    g.storage.day_summary(&today()).map_err(e)
}

#[tauri::command]
pub fn get_summary(state: State<AppState>) -> Res<DaySummary> {
    state.0.lock().map_err(e)?.storage.day_summary(&today()).map_err(e)
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Res<Settings> {
    state.0.lock().map_err(e)?.storage.settings().map_err(e)
}

#[tauri::command]
pub fn save_settings(state: State<AppState>, mut settings: Settings) -> Res<Settings> {
    settings.cap_min = settings.cap_min.clamp(rules::CAP_MIN, rules::CAP_MAX);
    settings.soft_floor_min = settings.soft_floor_min.clamp(30, 60);
    settings.calibrate_min = settings.calibrate_min.clamp(1, 5);
    settings.ceiling_min = rules::HARD_CEILING;
    let g = state.0.lock().map_err(e)?;
    g.storage.save_settings(&settings).map_err(e)?;
    if !settings.capture_context || settings.context_retention_days == 0 {
        g.storage.prune_context(0, now_secs()).map_err(e)?;
    }
    Ok(settings)
}

#[derive(Serialize)]
pub struct Insights {
    pub personal_cap: u32,
    pub peak_hours: Vec<u8>,
    pub flow_mean_2d: Option<f32>,
    pub extra_block_reduction: u8,
    pub landing_note: Option<&'static str>,
}

#[tauri::command]
pub fn get_insights(state: State<AppState>) -> Res<Insights> {
    let g = state.0.lock().map_err(e)?;
    let s = g.storage.settings().map_err(e)?;
    let flow = g.storage.recent_flow_mean(2).map_err(e)?;
    let samples = g.storage.hourly_flow_samples().map_err(e)?;
    let level = g.engine.landing_level(now_secs());
    Ok(Insights {
        personal_cap: g.storage.personal_cap(s.cap_min).map_err(e)?,
        // 至少 3 个样本的小时才参与（约 2–3 周数据后才会出现）
        peak_hours: rules::peak_hours(&samples, 3, 3),
        flow_mean_2d: flow,
        extra_block_reduction: rules::next_day_extra_reduction(flow),
        landing_note: (level != LandingLevel::None).then_some("已到落地区间"),
    })
}

/// 导出到“下载”目录，返回文件路径。format: json | csv
#[tauri::command]
pub fn export_data(app: AppHandle, state: State<AppState>, format: String) -> Res<String> {
    let g = state.0.lock().map_err(e)?;
    let (body, ext) = match format.as_str() {
        "csv" => (g.storage.export_csv().map_err(e)?, "csv"),
        _ => (g.storage.export_json().map_err(e)?, "json"),
    };
    drop(g);
    let dir = app
        .path()
        .download_dir()
        .or_else(|_| app.path().app_data_dir())
        .map_err(e)?;
    std::fs::create_dir_all(&dir).map_err(e)?;
    let path = dir.join(format!("flowarc-export-{}.{}", today(), ext));
    std::fs::write(&path, body).map_err(e)?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn delete_all_data(state: State<AppState>) -> Res<StateView> {
    let mut g = state.0.lock().map_err(e)?;
    g.storage.delete_all().map_err(e)?;
    g.engine.state = Default::default();
    g.storage.save_state(&g.engine.state).map_err(e)?;
    Ok(g.engine.view(now_secs()))
}
