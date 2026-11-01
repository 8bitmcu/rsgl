mod api;
mod blur;
mod config;
mod ui;

use config::{ApiCfg, Game, UiCfg};
use std::{
    os::unix::process::CommandExt,
    process::{Command, Stdio},
};

fn launch(g: &Game) {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(format!("exec {}", g.exec));
    if let Some(dir) = &g.workdir {
        cmd.current_dir(dir);
    }
    let _ = cmd
        .envs(&g.env)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0) // detach so the game outlives the launcher
        .spawn();
}

fn main() -> eframe::Result {
    let (games, ui_cfg, api_cfg, error) = match config::load() {
        Ok((g, u, a)) => (g, u, a, None),
        Err(e) => (vec![], UiCfg::default(), ApiCfg::default(), Some(e)),
    };
    ui::run(games, ui_cfg, api_cfg, error)
}
