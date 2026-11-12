//! End to end against a real haunt core. Copy into haunt's `cli/tests/` and run
//! `cargo test -p haunt-cli --features plugins --test checklist_meter` (see README).
#![cfg(feature = "plugins")]

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use haunt_core::plugin_settings::PluginConfig;
use haunt_core::plugin_store::{Store, attach};
use haunt_core::plugins::PluginHost;
use haunt_core::{Core, SurfaceEnv};

const ID: &str = "dev.haunt.checklist-meter";

fn repo() -> PathBuf {
    std::env::var_os("HAUNT_PLUGINS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../haunt-plugins"))
        .join("haunt-checklist-meter")
}

fn until(what: &str, f: impl Fn() -> bool) {
    let t = Instant::now();
    while !f() {
        // Generous: events reach the plugin on another thread, slow on a busy machine.
        assert!(t.elapsed() < Duration::from_secs(20), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn send(sock: &mut UnixStream, env: &SurfaceEnv, nonce: bool, cwd: &str, rest: &str) {
    let nonce = if nonce { format!(r#","nonce":"{}""#, env.nonce) } else { String::new() };
    let line = format!(r#"{{"v":1,"agent":"claude","surface_id":"{}"{nonce},"session_id":"s1","cwd":"{cwd}",{rest}}}"#, env.surface_id);
    sock.write_all(format!("{line}\n").as_bytes()).unwrap();
}

#[test]
fn the_row_follows_the_spec_checklist() {
    let dir = repo();
    if !dir.join("haunt-plugin.toml").is_file() {
        eprintln!("skipped: haunt-checklist-meter not found; set HAUNT_PLUGINS_DIR");
        return;
    }
    let tmp = PathBuf::from(format!("/tmp/haunt-fp-checklist-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let (home, user) = (tmp.join("home"), tmp.join("user"));
    let project = user.join("haunt");
    std::fs::create_dir_all(project.join("specs")).unwrap();
    let project = project.canonicalize().unwrap();
    // ${cwd} is the tab's git root.
    assert!(Command::new("git").args(["init", "-q"]).current_dir(&project).status().unwrap().success());
    let plan = project.join("specs/plan.md");
    std::fs::write(&plan, "- [x] one\n- [ ] two\n- [ ] three\n```\n- [ ] not counted\n```\n").unwrap();
    std::fs::write(project.join("specs/notes.txt"), "- [ ] not a spec\n").unwrap();
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["build", "--release", "--target", "wasm32-wasip2"])
        .current_dir(&dir)
        .env_remove("CARGO_TARGET_DIR")
        .status()
        .unwrap();
    assert!(status.success());
    let o = Command::new(env!("CARGO_BIN_EXE_haunt")).args(["plugin", "install", dir.to_str().unwrap(), "--yes"]).env("HAUNT_HOME", &home).output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));

    // ${cwd} is only ever a tab's folder strictly inside $HOME.
    unsafe { std::env::set_var("HOME", &user) };
    let core = Core::new(&home);
    core.start().unwrap();
    let (env, other) = (core.register_surface(project.to_str().unwrap()).unwrap(), core.register_surface("/tmp").unwrap());
    let set = Store::open(&home).unwrap().load_all(&PluginHost::new().unwrap(), Some(&core.handle())).unwrap();
    assert!(set.errors().is_empty(), "{:?}", set.errors());
    let set = Arc::new(Mutex::new(set));
    attach(&core, &set);
    let decoration = |s: &str| core.snapshot().groups.iter().flat_map(|g| &g.rows).find(|r| r.surface_id == s).and_then(|r| r.decoration.clone());
    let p = project.display().to_string();

    let mut sock = UnixStream::connect(&env.socket_path).unwrap();
    send(&mut sock, &other, false, &p, r#""event":"session_start""#);
    send(&mut sock, &env, true, &p, r#""event":"session_start""#);
    until("the meter", || decoration(&env.surface_id).is_some());
    let d = decoration(&env.surface_id).unwrap();
    assert_eq!((d.label.as_deref(), d.suffix.as_deref(), d.bar_permille), (Some("spec 1/3"), Some("specs/plan.md"), Some(333)));
    assert_eq!(decoration(&other.surface_id), None, "a forged event reads nothing");

    std::fs::write(&plan, "- [x] one\n- [x] two\n- [x] three\n").unwrap();
    let edit = format!(r#""event":"tool_ran","tool_name":"Edit","tool_use_id":"t1","tool_input":{{"file_path":"{}"}}"#, plan.display());
    send(&mut sock, &env, true, &p, &edit);
    until("the meter to fill", || decoration(&env.surface_id).and_then(|d| d.label).as_deref() == Some("spec 3/3 ✓"));
    assert_eq!(decoration(&env.surface_id).unwrap().bar_permille, None, "no bar once complete");

    set.lock().unwrap().set_config(&PluginConfig::parse(&format!("[plugins.\"{ID}\"]\npath = false\n")));
    assert_eq!(decoration(&env.surface_id).unwrap().suffix, None);

    drop(core);
    let _ = std::fs::remove_dir_all(&tmp);
}
