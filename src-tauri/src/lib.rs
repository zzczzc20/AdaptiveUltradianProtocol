mod commands;
mod sensing;

use flowarc_core::{rules::LandingLevel, Engine, Phase, Storage};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, WindowEvent,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub struct Inner {
    pub storage: Storage,
    pub engine: Engine,
}
pub struct AppState(pub Mutex<Inner>);

pub fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}
pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn show_main(app: &AppHandle, focus: bool) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        if focus {
            let _ = w.set_focus();
        }
    }
}

/// 若跨天且空闲，则开启新的一天；否则从库中恢复今日签到。
pub fn roll_day(inner: &mut Inner) {
    let today = today();
    let same_day = inner.engine.state.checkin.as_ref().map_or(false, |c| c.date == today);
    if same_day {
        return;
    }
    if inner.engine.state.phase == Phase::Idle {
        inner.engine.new_day();
        if let Ok(Some(c)) = inner.storage.get_checkin(&today) {
            inner.engine.state.checkin = Some(c);
        }
        if inner.engine.state.resume.is_none() {
            inner.engine.state.resume = inner.storage.latest_snapshot().ok().flatten();
        }
        let _ = inner.storage.save_state(&inner.engine.state);
    }
}

/// 后台循环：推进状态机、采样活动/空闲、检测断点、必要时唤起窗口。
fn spawn_background(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last_level = LandingLevel::None;
        let mut tick: u64 = 0;
        loop {
            std::thread::sleep(Duration::from_secs(5));
            tick += 1;
            let now = now_secs();
            let state = app.state::<AppState>();
            let (phase, focus_min, level, sensing, retention) = {
                let Ok(mut g) = state.0.lock() else { continue };
                roll_day(&mut g);
                let before = g.engine.state.phase;
                g.engine.tick(now);
                let settings = g.storage.settings().unwrap_or_default();
                if settings.sensing_enabled && g.engine.state.phase == Phase::Focus {
                    // 空闲 ≥20 秒视为自然断点；传感不可用时保持手动断点标记不变
                    if let Some(idle) = sensing::idle_secs() {
                        g.engine.set_breakpoint(idle >= 20);
                    }
                }
                if g.engine.state.phase != before {
                    let _ = g.storage.save_state(&g.engine.state);
                }
                (
                    g.engine.state.phase,
                    g.engine.focus_min(now),
                    g.engine.landing_level(now),
                    settings.sensing_enabled,
                    settings.context_retention_days,
                )
            };

            // 每分钟活动采样（仅 活跃/空闲 + 前台应用名）
            if tick % 12 == 0 && sensing && matches!(phase, Phase::Focus | Phase::Calibrating) {
                let idle = sensing::idle_secs();
                let app_name = sensing::foreground().map(|f| f.0);
                if let Ok(g) = state.0.lock() {
                    let _ = g.storage.record_activity(now, idle.map_or(true, |i| i < 60), app_name.as_deref());
                }
            }
            // 每小时清理一次过期上下文
            if tick % 720 == 0 {
                if let Ok(g) = state.0.lock() {
                    let _ = g.storage.prune_context(retention, now);
                }
            }

            // 托盘文字
            if let Some(tray) = app.tray_by_id("main") {
                let text = match phase {
                    Phase::Focus => format!("{} 分钟", focus_min),
                    Phase::Break => "休息".to_string(),
                    _ => String::new(),
                };
                let _ = tray.set_title(if text.is_empty() { None } else { Some(text.clone()) });
                let _ = tray.set_tooltip(Some(if text.is_empty() { "FlowArc".to_string() } else { format!("FlowArc · {}", text) }));
            }

            // 落地提示升级时唤起窗口
            if level != last_level {
                match level {
                    LandingLevel::None => {}
                    LandingLevel::Light => show_main(&app, false),
                    LandingLevel::Direct => show_main(&app, true),
                    LandingLevel::Strong => {
                        show_main(&app, true);
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.set_always_on_top(true);
                        }
                    }
                }
                if level != LandingLevel::Strong {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.set_always_on_top(false);
                    }
                }
                last_level = level;
            }
        }
    });
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app, true)))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _sc, ev| {
                    if ev.state() == ShortcutState::Pressed {
                        show_main(app, true);
                        let _ = app.emit("parking-hotkey", ());
                    }
                })
                .build(),
        )
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let storage = Storage::open(&dir.join("flowarc.db")).map_err(|e| e.to_string())?;
            let mut inner = Inner { engine: Engine::new(storage.load_state().unwrap_or_default()), storage };
            roll_day(&mut inner);
            let shortcut = inner.storage.settings().unwrap_or_default().parking_shortcut;
            app.manage(AppState(Mutex::new(inner)));

            // 托盘 / 菜单栏常驻
            let show = MenuItem::with_id(app, "show", "显示 FlowArc", true, None::<&str>)?;
            let park = MenuItem::with_id(app, "park", "停车场：记一笔", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &park, &quit])?;
            let mut tb = TrayIconBuilder::with_id("main")
                .tooltip("FlowArc")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, e| match e.id.as_ref() {
                    "show" => show_main(app, true),
                    "park" => {
                        show_main(app, true);
                        let _ = app.emit("parking-hotkey", ());
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, e| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                        show_main(tray.app_handle(), true);
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tb = tb.icon(icon.clone());
            }
            tb.build(app)?;

            // 全局快捷键（注册失败不致命：例如被其他程序占用）
            match shortcut.parse::<Shortcut>() {
                Ok(sc) => {
                    if let Err(e) = app.global_shortcut().register(sc) {
                        eprintln!("全局快捷键注册失败: {e}");
                    }
                }
                Err(e) => eprintln!("快捷键格式无效 ({shortcut}): {e}"),
            }

            spawn_background(app.handle().clone());
            Ok(())
        })
        // 关闭窗口 = 收进托盘，会话继续
        .on_window_event(|w, ev| {
            if let WindowEvent::CloseRequested { api, .. } = ev {
                api.prevent_close();
                let _ = w.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::submit_checkin,
            commands::start_block,
            commands::skip_calibration,
            commands::abort_block,
            commands::mark_breakpoint,
            commands::postpone,
            commands::request_landing,
            commands::submit_snapshot,
            commands::represcribe,
            commands::end_break,
            commands::add_parking,
            commands::list_parking,
            commands::complete_parking,
            commands::close_day,
            commands::get_summary,
            commands::get_settings,
            commands::save_settings,
            commands::get_insights,
            commands::export_data,
            commands::delete_all_data,
        ])
        .run(tauri::generate_context!())
        .expect("启动 FlowArc 失败");
}
