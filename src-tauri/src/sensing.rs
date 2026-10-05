//! 传感层：只读取“活动/空闲”与前台应用，绝不记录按键内容、截图或剪贴板。
//! 任一能力在当前平台/会话不可用（如 Wayland、权限被拒）时返回 None，上层自动降级为手动断点。

use flowarc_core::{ContextItem, Settings};
use std::process::Command;

/// 距上次键鼠输入的秒数
pub fn idle_secs() -> Option<u64> {
    user_idle::UserIdle::get_time().ok().map(|t| t.as_seconds())
}

/// (应用名, 窗口标题)
pub fn foreground() -> Option<(String, String)> {
    active_win_pos_rs::get_active_window()
        .ok()
        .map(|w| (w.app_name, w.title))
}

fn git(repo: &str, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(repo).args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

/// 抓取上下文候选项（每一项都可在快照卡中删除）。
pub fn capture_context(s: &Settings) -> Vec<ContextItem> {
    let mut v = Vec::new();
    if !s.capture_context || !s.sensing_enabled {
        return v;
    }
    if let Some((app, title)) = foreground() {
        if !app.is_empty() {
            v.push(ContextItem { kind: "app".into(), value: app, keep: true });
        }
        if !title.is_empty() {
            v.push(ContextItem { kind: "window".into(), value: title, keep: true });
        }
    }
    if !s.repo_path.trim().is_empty() {
        if let Some(b) = git(&s.repo_path, &["rev-parse", "--abbrev-ref", "HEAD"]) {
            v.push(ContextItem { kind: "git_branch".into(), value: b, keep: true });
        }
        if let Some(d) = git(&s.repo_path, &["diff", "--shortstat"]) {
            v.push(ContextItem { kind: "git_diff".into(), value: d, keep: true });
        }
    }
    v
}
