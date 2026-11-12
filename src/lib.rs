//! Checklist meter: ticked versus total checkboxes in a repo's spec or TODO file, on the agent's
//! row, so long jobs show progress past one turn.
// wit-bindgen's flattened `notify` import has 14 params.
#![allow(clippy::too_many_arguments)]

wit_bindgen::generate!({ path: "wit/haunt-plugin.wit", world: "plugin" });

mod checklist;

use std::collections::BTreeMap;
use std::sync::Mutex;

use checklist::{Meter, count, matches, pick, relative};
use haunt::plugin::log::log;
use haunt::plugin::rows::{self, Decoration, Tone};
use haunt::plugin::types::LogLevel;
use haunt::plugin::fs;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Files to read, relative to the agent's folder; `*` within the last part. Only those the
    /// manifest's fs grant covers can be read.
    pub files: Vec<String>,
    /// The file's path after the meta line.
    pub path: bool,
    /// The thin progress bar under the row.
    pub bar: bool,
    pub unverified: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { files: vec!["specs/*.md".into(), "TODO.md".into()], path: true, bar: true, unverified: false }
    }
}

#[derive(Debug, Clone, Default)]
struct Tab {
    cwd: String,
    touched: Option<String>,
    shown: Option<Meter>,
}

#[derive(Default)]
struct Live {
    settings: Settings,
    tabs: BTreeMap<String, Tab>,
}

static LIVE: Mutex<Option<Live>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Live) -> R) -> R {
    f(LIVE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Live::default))
}

/// Every candidate file under `cwd` with its counts, in the order of `patterns`.
fn candidates(cwd: &str, patterns: &[String]) -> Vec<(String, (u32, u32))> {
    let cwd = cwd.trim_end_matches('/');
    let mut out = Vec::new();
    for p in patterns {
        if p.is_empty() || p.starts_with('/') || p.split('/').any(|s| s.is_empty() || s == "." || s == "..") {
            continue;
        }
        let names: Vec<String> = match p.rsplit_once('/') {
            _ if !p.contains('*') => vec![p.clone()],
            Some((dir, _)) => fs::list(&format!("{cwd}/{dir}")).unwrap_or_default().into_iter().map(|n| format!("{dir}/{n}")).collect(),
            None => fs::list(cwd).unwrap_or_default(),
        };
        for rel in names.into_iter().filter(|r| matches(p, r)) {
            if out.iter().any(|(r, _)| *r == rel) {
                continue;
            }
            if let Ok(bytes) = fs::read(&format!("{cwd}/{rel}")) {
                out.push((rel, count(&String::from_utf8_lossy(&bytes))));
            }
        }
    }
    out
}

pub fn decoration(m: &Meter, s: &Settings) -> Decoration {
    Decoration {
        label: Some(m.label()),
        label_tone: Tone::Dim,
        suffix: s.path.then(|| m.rel.clone()),
        suffix_tone: Tone::Dim,
        scroll: false,
        bar: if s.bar { m.bar() } else { None },
        spark: None,
    }
}

fn refresh(surface: &str, l: &mut Live) {
    let Some(tab) = l.tabs.get_mut(surface) else { return };
    let meter = pick(&candidates(&tab.cwd, &l.settings.files), tab.touched.as_deref());
    if meter == tab.shown {
        return;
    }
    let d = meter.as_ref().map(|m| decoration(m, &l.settings));
    match rows::decorate(surface, d.as_ref()) {
        Ok(()) => tab.shown = meter,
        Err(e) => log(LogLevel::Debug, &format!("checklist-meter: {surface}: {e}")),
    }
}

const EDITS: [&str; 4] = ["Edit", "MultiEdit", "Write", "NotebookEdit"];

/// Whether an event may have changed a checklist: a new turn, its end, or an edit. Haunt sends file
/// paths only to plugins granted `event_details`; this one isn't, so an edit anywhere counts.
pub fn touched<'a>(kind: &str, tool: Option<&str>, cwd: &str, file: Option<&'a str>, patterns: &[String]) -> Option<Option<&'a str>> {
    let rel = file.and_then(|f| relative(cwd, f)).filter(|r| patterns.iter().any(|p| matches(p, r)));
    match kind {
        "session_start" | "prompt_submit" | "stop" => Some(rel),
        _ if rel.is_some() => Some(rel),
        "tool_ran" | "file_edited" if file.is_none() && tool.is_some_and(|t| EDITS.contains(&t)) => Some(None),
        _ => None,
    }
}

struct Plugin;

impl Guest for Plugin {
    fn init(json: String) {
        with(|l| l.settings = serde_json::from_str(&json).unwrap_or_default());
    }

    fn on_settings_changed(json: String) {
        with(|l| {
            l.settings = serde_json::from_str(&json).unwrap_or_default();
            let surfaces: Vec<String> = l.tabs.keys().cloned().collect();
            for s in surfaces {
                l.tabs.get_mut(&s).unwrap().shown = None;
                refresh(&s, l);
            }
        });
    }

    fn on_event(event: AgentEvent) {
        let (Some(surface), Some(cwd)) = (event.surface, event.cwd) else { return };
        with(|l| {
            if !event.verified && !l.settings.unverified {
                return;
            }
            let Some(rel) = touched(&event.kind, event.tool.as_deref(), &cwd, event.file_path.as_deref(), &l.settings.files) else { return };
            let tab = l.tabs.entry(surface.clone()).or_default();
            if tab.cwd != cwd {
                *tab = Tab { cwd, ..Tab::default() };
            }
            if let Some(r) = rel {
                tab.touched = Some(r.to_owned());
            }
            refresh(&surface, l);
        });
    }

    fn palette_items(_mode: String, _query: String) -> Vec<PaletteItem> {
        Vec::new()
    }

    fn on_action(_action_id: String, _payload: String) {}

    fn row_menu(_surface: String) -> Vec<MenuItem> {
        Vec::new()
    }

    fn on_row_moved(_surface: String, _group: String, _before: Option<String>) {}

    fn on_theme_changed() {}

    fn on_tick(_now_ms: u64) {}

    fn on_exec(_job: u64, _outcome: Result<haunt::plugin::exec::Output, String>) {}

    fn on_away_changed(_away: bool, _away_ms: u64) {}

    fn on_hover_hold(_held: bool, _surface: String) {}

    fn on_fs_tree(_job: u64, _outcome: Result<haunt::plugin::fs_list::TreeResult, String>) {}
}

export!(Plugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_row_reads_like_the_mockup() {
        let s = Settings::default();
        let d = decoration(&Meter { rel: "specs/plan.md".into(), done: 11, total: 18 }, &s);
        assert_eq!((d.label.as_deref(), d.suffix.as_deref()), (Some("spec 11/18"), Some("specs/plan.md")));
        assert!((d.bar.unwrap() - 0.611).abs() < 0.01);
        let done = decoration(&Meter { rel: "specs/plan.md".into(), done: 18, total: 18 }, &s);
        assert_eq!((done.label.as_deref(), done.bar), (Some("spec 18/18 ✓"), None));
        let bare = decoration(&Meter { rel: "TODO.md".into(), done: 1, total: 2 }, &Settings { path: false, bar: false, ..s });
        assert_eq!((bare.suffix, bare.bar), (None, None));
    }

    #[test]
    fn only_turn_edges_and_checklist_files_refresh() {
        let p = Settings::default().files;
        let e = Some("Edit");
        assert_eq!(touched("tool_ran", e, "/w/h", Some("/w/h/specs/plan.md"), &p), Some(Some("specs/plan.md")));
        assert_eq!(touched("file_edited", e, "/w/h", Some("/w/h/TODO.md"), &p), Some(Some("TODO.md")));
        assert_eq!(touched("tool_ran", e, "/w/h", Some("/w/h/src/main.rs"), &p), None);
        assert_eq!(touched("tool_ran", e, "/w/h", Some("/w/other/TODO.md"), &p), None);
        assert_eq!(touched("stop", None, "/w/h", None, &p), Some(None));
        assert_eq!(touched("tool_ran", e, "/w/h", None, &p), Some(None), "without paths any edit counts");
        assert_eq!(touched("tool_ran", Some("Bash"), "/w/h", None, &p), None);
        assert_eq!(touched("tool_start", e, "/w/h", None, &p), None, "only once it ran");
    }

    #[test]
    fn manifest_asks_for_what_it_uses_and_defaults_match() {
        let m = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/haunt-plugin.toml")).unwrap();
        let t: toml::Table = toml::from_str(&m).unwrap();
        let caps = t["capabilities"].as_table().unwrap();
        assert_eq!(caps.keys().collect::<Vec<_>>(), ["decorate", "events", "fs_read"]);
        let globs: Vec<_> = caps["fs_read"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().strip_prefix("${cwd}/").unwrap()).collect();
        assert_eq!(globs, Settings::default().files, "the default files are exactly the grant");
        let defaults: serde_json::Map<String, serde_json::Value> =
            t["settings"].as_table().unwrap().iter().map(|(k, v)| (k.clone(), serde_json::to_value(&v["default"]).unwrap())).collect();
        assert_eq!(serde_json::from_value::<Settings>(serde_json::Value::Object(defaults)).unwrap(), Settings::default());
    }
}
