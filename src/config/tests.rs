use super::{paths::Paths, roms::rom_name, *};
use crate::keys::Action;

fn games(text: &str) -> Result<Vec<Game>, String> {
    parse_games(text)
}

#[test]
fn the_example_config_loads() {
    assert_eq!(games(EXAMPLE).unwrap().len(), 1);
}

#[test]
fn config_next_to_exe_comes_first() {
    let exe_dir = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
    assert_eq!(candidates().first(), Some(&exe_dir.join("rsgl.toml")));
    assert!(!home_candidates().iter().any(|p| p.starts_with(&exe_dir)));
}

#[test]
fn menus_merge_shared_entries_workdir_and_env() {
    let g = games(
        r#"
[[right_click]]
title = "Open folder"
action = "open_folder"

[[game]]
name = "A"
exec = "a"
workdir = "/games/a"
env = { X = "1", Y = "2" }

[[game.right_click]]
title = "Tool"
exec = "tool"
workdir = "/elsewhere"
env = { Y = "3" }

[[game.right_click]]
title = "Windowed"
exec = "./a -windowed"
launch = true

[[game]]
name = "B"
exec = "b"
"#,
    )
    .unwrap();

    let a = &g[0];
    assert_eq!(a.menu.len(), 3, "shared entry first, then the game's own");
    assert_eq!(a.menu[0].action, MenuAction::OpenFolder);
    assert_eq!(a.menu[0].workdir, Some(PathBuf::from("/games/a")));

    let tool = &a.menu[1];
    assert_eq!(tool.action, MenuAction::Exec(vec!["tool".into()]));
    assert_eq!(tool.workdir, Some(PathBuf::from("/elsewhere")), "own workdir wins");
    assert_eq!(tool.env.all["X"], "1", "game env is kept");
    assert_eq!(tool.env.all["Y"], "3", "entry env overrides");
    assert!(!tool.launch);

    let windowed = &a.menu[2];
    assert_eq!(windowed.workdir, Some(PathBuf::from("/games/a")), "defaults to the game's");
    assert!(windowed.launch);

    let b = &g[1];
    assert_eq!(b.menu.len(), 1, "only the shared entry");
    assert_eq!(b.menu[0].workdir, None);
}

#[test]
fn games_without_entries_have_no_menu() {
    let g = games("[[game]]\nname = \"A\"\nexec = \"a\"\n").unwrap();
    assert!(g[0].menu.is_empty());
}

#[test]
fn bad_entries_are_errors() {
    let entry = |body: &str| format!("[[game]]\nname = \"A\"\nexec = \"a\"\n[[game.right_click]]\n{body}\n");
    for (body, want) in [
        ("title = \"T\"", "needs an exec"),
        ("title = \"T\"\nexec = \"x\"\naction = \"open_folder\"", "not both"),
        ("title = \"T\"\naction = \"nope\"", "unknown action"),
        ("title = \"T\"\naction = \"open_folder\"\nlaunch = true", "needs an exec"),
    ] {
        let e = games(&entry(body)).err().unwrap_or_else(|| panic!("{body:?} should fail"));
        assert!(e.contains(want), "{body:?} gave {e:?}");
    }
}

#[test]
fn a_key_below_a_menu_header_is_rejected() {
    // `tags` here belongs to the menu entry, not the game
    let e = games(
        "[[game]]\nname = \"A\"\nexec = \"a\"\n[[game.right_click]]\ntitle = \"T\"\nexec = \"x\"\ntags = [\"x\"]\n",
    )
    .err()
    .expect("should fail");
    assert!(e.contains("unknown field"), "{e}");
}

fn game_exec(text: &str) -> String {
    games(text).unwrap().remove(0).exec.remove(0)
}

fn game_err(text: &str) -> String {
    games(text).err().expect("should fail")
}

#[test]
fn vars_nest_and_game_overrides_reach_them() {
    let g = games(
        r#"
[vars]
games = "/g"
w = 3440
h = 1440
gs = "gs -w {w} -h {h} --"

[[game]]
name = "A"
exec = "{gs} wine a.exe"
workdir = "{games}/A"

[[game]]
name = "B"
vars = { w = 2560 }
exec = "{gs} wine b.exe"
workdir = "{games}/B"
"#,
    )
    .unwrap();
    assert_eq!(g[0].exec, ["gs -w 3440 -h 1440 -- wine a.exe"]);
    assert_eq!(g[0].workdir, Some(PathBuf::from("/g/A")));
    assert_eq!(g[1].exec, ["gs -w 2560 -h 1440 -- wine b.exe"], "the game's w flows through {{gs}}");
}

#[test]
fn templates_merge_under_the_game_in_order() {
    let g = games(
        r#"
[vars]
games = "/g"

[templates.default]
workdir = "{games}/{dir}"

[templates.wine]
exec = 'wine "{exe}"'
env = { WINEPREFIX = "{prefix}", WINEDEBUG = "-all" }
vars = { prefix = "/wine/{dir}" }
tags = ["pc", "wine"]

[[templates.wine.right_click]]
title = "winecfg"
exec = "winecfg"

[templates.coop]
tags = ["coop", "pc"]
playtime = false

[[game]]
name = "DRG"
template = ["wine", "coop"]
vars = { dir = "drg", exe = "FSD.exe", prefix = "/wine-drg" }
env = { DXVK_HUD = "fps" }
tags = ["fps"]

[[game.right_click]]
title = "Own"
exec = "own"

[[game]]
name = "Native"
exec = "./run"
vars = { dir = "native" }
workdir = ""
"#,
    )
    .unwrap();
    let drg = &g[0];
    assert_eq!(drg.exec, ["wine \"FSD.exe\""], "exec comes from the template, filled in with the game's vars");
    assert_eq!(drg.workdir, Some(PathBuf::from("/g/drg")), "default applies to every game");
    assert_eq!(drg.env.all["WINEPREFIX"], "/wine-drg", "the game's own var wins over the template's");
    assert_eq!((drg.env.all["WINEDEBUG"].as_str(), drg.env.all["DXVK_HUD"].as_str()), ("-all", "fps"));
    assert_eq!(drg.tags, ["pc", "wine", "coop", "fps"], "tags add up, each once");
    assert!(!drg.playtime);
    let titles: Vec<_> = drg.menu.iter().map(|m| m.title.as_str()).collect();
    assert_eq!(titles, ["winecfg", "Own"], "the template's entries come first");

    let native = &g[1];
    assert_eq!(native.workdir, None, "workdir = \"\" opts out of the template's");
    assert!(native.playtime && native.menu.is_empty() && native.tags.is_empty());
}

#[test]
fn template_errors_say_which_game_and_what_to_do() {
    let e = game_err("[[game]]\nname = \"A\"\ntemplate = \"wnie\"\nexec = \"a\"\n");
    assert!(e.contains("game \"A\": no template \"wnie\""), "{e}");
    let e = game_err("[templates.default]\ntags = [\"x\"]\n[[game]]\nname = \"A\"\ntemplate = \"default\"\nexec = \"a\"\n");
    assert!(e.contains("every [[game]] gets [templates.default] already"), "{e}");
    let e = game_err("[[game]]\nname = \"A\"\n");
    assert!(e.contains("game \"A\": needs an exec, here or in a template"), "{e}");
    for key in ["name = \"x\"", "steam_appid = 1", "igdb_id = 1", "template = \"y\"", "hidden = true"] {
        let e = game_err(&format!("[templates.t]\n{key}\n"));
        assert!(e.contains("unknown field"), "{key}: {e}");
    }
    // hidden games aren't checked on load, but --check reports them
    let text = "[[game]]\nname = \"A\"\ntemplate = \"nope\"\nexec = \"a\"\nhidden = true\n";
    assert!(games(text).unwrap().is_empty());
    let report = check_config(toml::from_str(text).unwrap());
    assert_eq!(report.errors, ["game \"A\": no template \"nope\" (hidden game)"]);
}

#[test]
fn removed_defaults_and_for_say_what_replaced_them() {
    let e = game_err("[defaults]\nworkdir = \"/g\"\n");
    assert!(e.contains("[defaults] was removed: rename it to [templates.default]"), "{e}");
    for at in ["[[right_click]]", "[[templates.t.right_click]]"] {
        let e = game_err(&format!("{at}\ntitle = \"T\"\nexec = \"t\"\nfor = \"game\"\n"));
        assert!(e.contains("`for` was removed: move this entry to [[templates.default.right_click]]"), "{at}: {e}");
    }
    let e = game_err("[[game]]\nname = \"A\"\nexec = \"a\"\n[[game.right_click]]\ntitle = \"T\"\nexec = \"t\"\nfor = \"rom\"\n");
    assert!(e.contains("`for` was removed"), "{e}");
}

#[test]
fn braces_can_be_escaped() {
    let e = game_exec("[[game]]\nname = \"A\"\nexec = \"awk '{{print $1}}' {{x}}\"\n");
    assert_eq!(e, "awk '{print $1}' {x}");
}

#[test]
fn numbers_and_booleans_are_their_text() {
    let e = game_exec(
        "[vars]\nn = 5\nf = 1.5\nb = true\n[[game]]\nname = \"A\"\nexec = \"{n} {f} {b}\"\n",
    );
    assert_eq!(e, "5 1.5 true");
}

#[test]
fn tilde_is_expanded_after_substitution() {
    let g = games("[vars]\nbase = \"~/g\"\n[[game]]\nname = \"A\"\nexec = \"x\"\nworkdir = \"{base}/A\"\n")
        .unwrap();
    let wd = g[0].workdir.clone().unwrap();
    assert_eq!(wd, PathBuf::from(expand("~/g/A")));
    assert!(!wd.starts_with("~"));
}

#[test]
fn vars_work_in_cover_env_and_menu_entries() {
    let g = games(
        r#"
[vars]
dir = "Quake"

[[right_click]]
title = "Open"
exec = "xdg-open /g/{dir}"
env = { D = "{dir}" }

[[game]]
name = "A"
exec = "x"
env = { WINEPREFIX = "/p/{dir}" }
cover = "/c/{dir}.png"

[[game.right_click]]
title = "Tool"
exec = "tool {dir}"
workdir = "/w/{dir}"
"#,
    )
    .unwrap();
    let a = &g[0];
    assert_eq!(a.env.all["WINEPREFIX"], "/p/Quake");
    assert_eq!(a.menu[0].action, MenuAction::Exec(vec!["xdg-open /g/Quake".into()]));
    assert_eq!(a.menu[0].env.all["D"], "Quake");
    assert_eq!(a.menu[1].action, MenuAction::Exec(vec!["tool Quake".into()]));
    assert_eq!(a.menu[1].workdir, Some(PathBuf::from("/w/Quake")));
}

#[test]
fn a_menu_entry_reruns_the_games_exec_with_its_own_vars() {
    let g = games(
        r#"
[vars]
gs = "gamescope -w {w} --"
w = 3440

[[right_click]]
title = "No gamescope"
exec = "{exec}"
vars = { gs = "" }
launch = true

[[game]]
name = "A"
exec = "{gs} wine a.exe"
vars = { gs = "gs-a" }

[[game.right_click]]
title = "Narrow"
exec = "{exec} -windowed"
vars = { w = 1920 }

[[game]]
name = "B"
exec = "{gs} wine b.exe"
"#,
    )
    .unwrap();
    let (a, b) = (&g[0], &g[1]);
    assert_eq!(a.exec, ["gs-a wine a.exe"]);
    // the entry's vars win over the game's own
    assert_eq!(a.menu[0].action, MenuAction::Exec(vec![" wine a.exe".into()]));
    assert_eq!(a.menu[1].action, MenuAction::Exec(vec!["gs-a wine a.exe -windowed".into()]));
    assert_eq!(b.exec, ["gamescope -w 3440 -- wine b.exe"]);
    assert_eq!(b.menu[0].action, MenuAction::Exec(vec![" wine b.exe".into()]));

    // only menu entries have {exec}
    let e = game_err("[[game]]\nname = \"A\"\nexec = \"{exec}\"\n");
    assert!(e.contains("{exec} isn't defined"), "{e}");
    let e = game_err("[[right_click]]\ntitle = \"T\"\nexec = \"t\"\nvars = { v = [1] }\n[[game]]\nname = \"A\"\nexec = \"a\"\n");
    assert!(e.contains("entry \"T\": vars: v must be"), "{e}");
}

#[test]
fn exec_accepts_a_list_of_commands_and_expands_each_item() {
    let g = games(
        r#"
[vars]
fix = "sed -i 's/res=[0-9]+/res={res}/' {cfg}"
gs = "gamescope -w {w} --"
pull = "rsync remote/{path}/ saves/"
push = "rsync saves/ remote/{path}/"

[[right_click]]
title = "Launch without Gamescope"
exec = "{exec}"
vars = { gs = "" }
launch = true

[[game]]
name = "Delta Force"
exec = ["{pull}", "{fix}", "{gs} wine Df.exe", "{push}"]
vars = { path = "DeltaForce", w = 1024, cfg = "df.cfg", res = 5 }

[[game.right_click]]
title = "Wrap"
exec = ["pre", "{exec}", "post"]
vars = { w = 800 }
"#,
    )
    .unwrap();
    let df = &g[0];
    assert_eq!(
        df.exec,
        [
            "rsync remote/DeltaForce/ saves/",
            "sed -i 's/res=[0-9]+/res=5/' df.cfg",
            "gamescope -w 1024 -- wine Df.exe",
            "rsync saves/ remote/DeltaForce/",
        ]
    );
    assert_eq!(
        df.menu[0].action,
        MenuAction::Exec(vec![
            "rsync remote/DeltaForce/ saves/".into(),
            "sed -i 's/res=[0-9]+/res=5/' df.cfg".into(),
            " wine Df.exe".into(),
            "rsync saves/ remote/DeltaForce/".into(),
        ]),
        "{{exec}} stays a list with the entry's vars applied to each step"
    );
    assert_eq!(
        df.menu[1].action,
        MenuAction::Exec(vec![
            "pre".into(),
            "rsync remote/DeltaForce/ saves/".into(),
            "sed -i 's/res=[0-9]+/res=5/' df.cfg".into(),
            "gamescope -w 800 -- wine Df.exe".into(),
            "rsync saves/ remote/DeltaForce/".into(),
            "post".into(),
        ]),
    );

    let shown = describe(df);
    assert!(
        shown.contains(
            "  exec:    rsync remote/DeltaForce/ saves/\n           sed -i 's/res=[0-9]+/res=5/' df.cfg\n           gamescope -w 1024 -- wine Df.exe\n           rsync saves/ remote/DeltaForce/\n"
        ),
        "{shown}"
    );

    assert!(game_err("[[game]]\nname = \"A\"\nexec = []\n").contains("game \"A\": exec is empty"));
    assert!(
        game_err("[[game]]\nname = \"A\"\nexec = \"a\"\n[[game.right_click]]\ntitle = \"T\"\nexec = []\n")
            .contains("entry \"T\": exec is empty")
    );
    assert!(
        game_err("[[roms]]\npath = \"/r\"\nextensions = [\"nes\"]\nexec = []\n")
            .contains("[[roms]] \"/r\": exec is empty")
    );
}

#[test]
fn menu_entries_get_the_games_name_art_and_workdir() {
    let cache = std::env::temp_dir().join(format!("rsgl-menu-vars-test-{}", std::process::id()));
    let (cover, hero) = (cache.join("covers/rsgl_menu_test.png"), cache.join("heroes/rsgl_menu_test.jpg"));
    for f in [&cover, &hero] {
        fs::create_dir_all(f.parent().unwrap()).unwrap();
        fs::write(f, "").unwrap();
    }
    // only this test's game has art there, so the other tests don't notice
    PATHS.write().unwrap().cache = Some(cache.clone());
    let g = games(
        r#"
[[right_click]]
title = "T"
exec = "t '{name}' {grid} {hero} {workdir}"

[[game]]
name = "Rsgl {x} Test {year}"
exec = "a"
workdir = "/g/{x}"
vars = { x = "Menu" }

[[game]]
name = "B"
exec = "b"
vars = { hero = "mine" }
"#,
    );
    PATHS.write().unwrap().cache = None;
    fs::remove_dir_all(&cache).unwrap();
    let g = g.unwrap();
    let (b, test) = (&g[0], &g[1]);

    // the art is found by the name without {year}, which is also what downloads are saved as
    assert_eq!(test.cover.as_ref(), Some(&cover));
    let want = format!("t 'Rsgl Menu Test' {} {} /g/Menu", cover.display(), hero.display());
    assert_eq!(test.menu[0].action, MenuAction::Exec(vec![want]));
    // no art or workdir yet is blank, and the game's own variable wins
    assert_eq!(b.menu[0].action, MenuAction::Exec(vec!["t 'B'  mine ".into()]));
    // they're only for menu entries, like {exec}
    assert!(game_err("[[game]]\nname = \"A\"\nexec = \"{grid}\"\n").contains("{grid} isn't defined"));
}

#[test]
fn a_shared_menu_entry_sees_each_games_own_vars() {
    let text = |second: &str| {
        format!(
            "[[right_click]]\ntitle = \"T\"\nexec = \"tool {{dir}}\"\n\
                 [[game]]\nname = \"A\"\nexec = \"a\"\nvars = {{ dir = \"one\" }}\n\
                 [[game]]\nname = \"B\"\nexec = \"b\"\n{second}\n"
        )
    };
    let g = games(&text("vars = { dir = \"two\" }")).unwrap();
    assert_eq!(g[0].menu[0].action, MenuAction::Exec(vec!["tool one".into()]));
    assert_eq!(g[1].menu[0].action, MenuAction::Exec(vec!["tool two".into()]));

    // a game that doesn't define it is reported, naming both the entry and the game
    let e = game_err(&text(""));
    assert!(e.contains("[[right_click]]") && e.contains("\"B\"") && e.contains("{dir} isn't defined"), "{e}");
}

#[test]
fn errors_name_the_game_and_the_field() {
    let e = game_err("[[game]]\nname = \"A\"\nexec = \"run {nope}\"\n");
    assert!(e.contains("game \"A\"") && e.contains("exec") && e.contains("{nope} isn't defined"), "{e}");

    let e = game_err("[[game]]\nname = \"A\"\nexec = \"x\"\nworkdir = \"{nope}\"\n");
    assert!(e.contains("workdir"), "{e}");

    let e = game_err("[[game]]\nname = \"A\"\nexec = \"x\"\nenv = { K = \"{nope}\" }\n");
    assert!(e.contains("env K"), "{e}");

    // an undefined name inside a variable says which variable
    let e = game_err("[vars]\ngs = \"gamescope {w}\"\n[[game]]\nname = \"A\"\nexec = \"{gs}\"\n");
    assert!(e.contains("{w} isn't defined") && e.contains("in variable gs"), "{e}");
}

#[test]
fn loops_are_errors() {
    let e = game_err("[vars]\na = \"{b}\"\nb = \"{a}\"\n[[game]]\nname = \"A\"\nexec = \"{a}\"\n");
    assert!(e.contains("circle") && e.contains("a -> b -> a"), "{e}");
    let e = game_err("[vars]\na = \"x{a}\"\n[[game]]\nname = \"A\"\nexec = \"{a}\"\n");
    assert!(e.contains("a -> a"), "{e}");
}

#[test]
fn stray_and_odd_braces_are_errors() {
    for (exec, want) in [
        ("run {", "unclosed {"),
        ("run }", "stray }"),
        ("run {}", "isn't a variable name"),
        ("run {a b}", "isn't a variable name"),
        ("awk '{print $1}'", "isn't a variable name"),
    ] {
        let e = game_err(&format!("[[game]]\nname = \"A\"\nexec = \"{}\"\n", exec.replace('"', "\\\"")));
        assert!(e.contains(want), "{exec:?} gave {e:?}");
    }
}

#[test]
fn only_strings_numbers_and_booleans_are_vars() {
    let e = game_err("[vars]\nx = [1, 2]\n[[game]]\nname = \"A\"\nexec = \"a\"\n");
    assert!(e.contains("[vars]") && e.contains("x must be"), "{e}");
    let e = game_err("[[game]]\nname = \"A\"\nexec = \"a\"\nvars = { x = { y = 1 } }\n");
    assert!(e.contains("game \"A\", vars"), "{e}");
}

#[test]
fn hidden_games_are_not_checked() {
    let g = games("[[game]]\nname = \"A\"\nexec = \"{undefined}\"\nhidden = true\n[[game]]\nname = \"B\"\nexec = \"b\"\n")
        .unwrap();
    assert_eq!(g.len(), 1);
}

#[test]
fn show_finds_games_and_describes_them() {
    let g = games(
        r#"
[[game]]
name = "Left 4 Dead 2"
exec = "run l4d2"
workdir = "/g/l4d2"
env = { A = "1" }

[[game.right_click]]
title = "Tool"
exec = "tool"
env = { B = "2" }

[[game]]
name = "Left Alive"
exec = "x"
"#,
    )
    .unwrap();
    assert_eq!(find(&g, "left 4 dead 2").unwrap().name, "Left 4 Dead 2");
    assert_eq!(find(&g, "alive").unwrap().name, "Left Alive");
    assert!(find(&g, "left").err().unwrap().contains("matches 2 games"));
    assert!(find(&g, "zzz").err().unwrap().contains("no game matches"));

    let text = describe(&g[0]);
    assert!(text.contains("exec:    run l4d2") && text.contains("workdir: /g/l4d2"), "{text}");
    assert!(text.contains("A=1") && text.contains("1. Tool") && text.contains("B=2"), "{text}");
}

#[test]
fn global_env_sits_under_each_games_own() {
    let g = games(
        r#"
[vars]
prefix = "/p"

[env]
WINEDEBUG = "-all"
WINEPREFIX = "{prefix}/{dir}"
SHARED = "global"

[[right_click]]
title = "Tool"
exec = "tool"

[[game]]
name = "A"
exec = "a"
vars = { dir = "a" }
env = { SHARED = "mine", ONLY_A = "1" }
"#,
    )
    .unwrap();
    let a = &g[0];
    assert_eq!(a.env.all["WINEDEBUG"], "-all", "global values apply");
    assert_eq!(a.env.all["WINEPREFIX"], "/p/a", "and see the game's variables");
    assert_eq!(a.env.all["SHARED"], "mine", "the game's own value wins");
    assert_eq!(a.env.all["ONLY_A"], "1");
    assert_eq!(a.menu[0].env.all["WINEDEBUG"], "-all", "menu entries get it too");
}

#[test]
fn toggles_replace_global_env_but_not_a_games_or_entrys_own() {
    let g = games(
        r#"
[env]
MANGOHUD = "0"
DXVK_HUD = "0"
WINEDEBUG = "-all"

[[game]]
name = "A"
exec = "a"
env = { DXVK_HUD = "fps" }
right_click = [{ title = "T", exec = "t", env = { MANGOHUD = "0" } }]
"#,
    )
    .unwrap();
    let a = &g[0];
    let toggles = Toggles::from([
        ("MANGOHUD".to_string(), Some("1".to_string())),
        ("DXVK_HUD".to_string(), Some("full".to_string())),
        ("WINEDEBUG".to_string(), None),
        ("GAMEMODE".to_string(), Some("1".to_string())),
    ]);
    assert_eq!(a.env.get("MANGOHUD", &toggles), Some(Some("1")), "replaces [env]");
    assert_eq!(a.env.get("DXVK_HUD", &toggles), Some(Some("fps")), "the game's own wins");
    assert_eq!(a.env.get("WINEDEBUG", &toggles), Some(None), "off without a value unsets");
    assert_eq!(a.env.get("PATH", &toggles), None, "left to rsgl's own");
    assert_eq!(
        a.env.with(&toggles),
        [("DXVK_HUD", Some("fps")), ("GAMEMODE", Some("1")), ("MANGOHUD", Some("1")), ("WINEDEBUG", None)],
        "also one nothing else sets"
    );
    assert_eq!(a.menu[0].env.get("MANGOHUD", &toggles), Some(Some("0")), "the entry's own wins");
    assert_eq!(a.menu[0].env.get("DXVK_HUD", &toggles), Some(Some("fps")), "so does the game's");
    assert_eq!(a.env.get("MANGOHUD", &Toggles::new()), Some(Some("0")), "no toggles, just [env]");
}

#[test]
fn global_env_errors_say_where_they_came_from() {
    let e = game_err("[env]\nK = \"{dir}\"\n[[game]]\nname = \"A\"\nexec = \"a\"\n");
    assert!(e.contains("[env]") && e.contains("game \"A\"") && e.contains("{dir} isn't defined"), "{e}");
    // values must be strings, like a game's env
    assert!(games("[env]\nK = 1\n[[game]]\nname = \"A\"\nexec = \"a\"\n").is_err());
}

#[test]
fn keyboard_and_gamepad_tables_are_read_and_checked() {
    let (cfg, unknown) = parse("[keyboard]\nlaunch = [\"F2\"]\n[gamepad]\nlaunch = [\"X\"]\n").unwrap();
    assert!(unknown.is_empty(), "{unknown:?}");
    assert_eq!(cfg.keyboard.label(Action::Launch).as_deref(), Some("F2"));
    assert_eq!(cfg.gamepad.label(Action::Launch).as_deref(), Some("X"));

    let e = parse("[gamepad]\nback = [\"Circle\"]\n").err().unwrap().to_string();
    assert!(e.contains("gamepad") && e.contains("unknown button \"Circle\""), "{e}");
}

#[test]
fn unknown_keys_are_warnings_not_errors() {
    let (cfg, unknown) = parse(
        "[evn]\nK = \"1\"\n[ui]\nblurr = 3\n[[game]]\nname = \"A\"\nexec = \"a\"\ncovr = \"x\"\n",
    )
    .unwrap();
    assert_eq!(cfg.games.len(), 1);
    assert_eq!(unknown.len(), 3, "{unknown:?}");
    for key in ["`evn`", "`ui.blurr`", "`game.0.covr`"] {
        assert!(unknown.iter().any(|w| w.contains(key)), "{key} in {unknown:?}");
    }
    // menu entries still reject unknown keys, see MenuCfg
    let menu = "[[game]]\nname = \"A\"\nexec = \"a\"\n[[game.right_click]]\ntitle = \"T\"\nexec = \"t\"\nhidden = true\n";
    assert!(parse(menu).is_err());
}

#[test]
fn a_game_is_installed_when_its_workdir_exists_or_isnt_set() {
    let dir = std::env::temp_dir();
    let g = games(&format!(
        r#"
[[game]]
name = "Here"
exec = "a"
workdir = {dir:?}

[[game]]
name = "Gone"
exec = "a"
workdir = "/rsgl-test/no/such/dir"

[[game]]
name = "No workdir"
exec = "a"
"#
    ))
    .unwrap();
    let installed = |name: &str| g.iter().find(|g| g.name == name).unwrap().installed();
    assert!(installed("Here"));
    assert!(!installed("Gone"));
    assert!(installed("No workdir"));
}

fn checked(text: &str) -> Report {
    check_config(toml::from_str(text).unwrap())
}

#[test]
fn paths_are_taken_from_the_config_folder() {
    let paths = |text: &str| parse(text).unwrap().0.paths.resolve(Path::new("/usb/rsgl"));
    assert_eq!(paths("").unwrap(), Paths { cache: None, playtime: None }, "unset keeps the defaults");
    let p = paths("[paths]\ncache = \"./cache\"\nplaytime = \"/data/playtime.toml\"").unwrap();
    assert_eq!(p.cache, Some(PathBuf::from("/usb/rsgl/./cache")));
    assert_eq!(p.playtime, Some(PathBuf::from("/data/playtime.toml")), "an absolute path is kept");
    let home = directories::BaseDirs::new().unwrap().home_dir().to_path_buf();
    assert_eq!(paths("[paths]\ncache = \"~/rsgl\"").unwrap().cache, Some(home.join("rsgl")));
    assert_eq!(paths("[paths]\nplaytime = \"\"").unwrap_err(), "[paths]: playtime is empty");
}

#[test]
fn config_folder_of_a_relative_path_is_absolute() {
    let folder = config_folder(Path::new("rsgl.toml")).unwrap();
    assert_eq!(folder, std::env::current_dir().unwrap());
}

#[test]
fn check_reports_every_problem_hidden_games_included() {
    let dir = std::env::temp_dir();
    let r = checked(&format!(
        r#"
[[game]]
name = "Fine"
exec = "a"
workdir = {dir:?}

[[game]]
name = "Bad one"
exec = "{{nope}}"

[[game]]
name = "Bad hidden"
exec = "{{nope2}}"
hidden = true

[[game]]
name = "Gone"
exec = "a"
workdir = "/definitely/not/here"

[[game.right_click]]
title = "Tool"
exec = "t"
workdir = "/also/not/here"
"#
    ));
    assert_eq!((r.games, r.hidden), (4, 1));
    assert_eq!(r.errors.len(), 2, "both bad games are reported, not just the first: {:?}", r.errors);
    assert!(r.errors[0].contains("Bad one") && r.errors[0].contains("{nope}"));
    assert!(r.errors[1].contains("Bad hidden") && r.errors[1].contains("hidden game"));
    assert_eq!(r.warnings.len(), 2, "{:?}", r.warnings);
    assert!(r.warnings[0].contains("Gone") && r.warnings[0].contains("/definitely/not/here"));
    assert!(r.warnings[1].contains("entry \"Tool\"") && r.warnings[1].contains("/also/not/here"));
}

#[test]
fn check_warns_about_duplicate_names_and_stops_on_bad_vars() {
    let r = checked("[[game]]\nname = \"Quake\"\nexec = \"a\"\n[[game]]\nname = \"quake\"\nexec = \"b\"\n");
    assert!(r.errors.is_empty());
    assert!(r.warnings.iter().any(|w| w.contains("2 games are named")), "{:?}", r.warnings);

    let r = checked("[vars]\nx = [1]\n[[game]]\nname = \"A\"\nexec = \"a\"\n");
    assert_eq!(r.errors.len(), 1);
    assert!(r.errors[0].contains("[vars]"));
}

#[test]
fn check_warns_about_filter_tags_no_game_has() {
    let r = checked(
        r#"
[topbar]
left = [{ type = "filter", options = [
  { title = "All", filter = "" },
  { title = "LAN", filter = "t:pc t:LAN quake g:shooter" },
  { title = "NES", filter = "t:nes" },
  { title = "Old", filter = "t:old" },
] }]

[[game]]
name = "Quake"
exec = "a"
tags = ["pc", "lan-party"]

[[game]]
name = "Hidden"
exec = "a"
tags = ["old"]
hidden = true
"#,
    );
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(
        r.warnings,
        [
            "[topbar]: filter \"NES\": no game has a tag t:nes matches",
            "[topbar]: filter \"Old\": no game has a tag t:old matches",
        ],
        "tags match from their start, any case; a hidden game isn't shown"
    );
}

#[test]
fn grid_and_hero_ids_are_read() {
    let g = games("[[game]]\nname = \"A\"\nexec = \"a\"\ngrid_id = 664940\nhero_id = 15208\n").unwrap();
    assert_eq!((g[0].grid_id, g[0].hero_id), (Some(664940), Some(15208)));
    let g = games("[[game]]\nname = \"A\"\nexec = \"a\"\n").unwrap();
    assert_eq!((g[0].grid_id, g[0].hero_id), (None, None));
}

fn games_with_meta(text: &str, years: &[(&str, i32)]) -> Vec<Game> {
    let cfg: Config = toml::from_str(text).unwrap();
    let meta = years.iter().map(|(n, y)| (n.to_string(), Meta { year: Some(*y), ..Meta::default() })).collect();
    build(cfg, meta).unwrap().games
}

#[test]
fn names_take_variables_and_the_year_stays_out_of_the_name() {
    let text = r#"
[vars]
series = "Command & Conquer"
[[game]]
name = "{series} 3: Kane's Wrath {year}"
exec = "a"
[[game]]
name = "Quake ({year})"
exec = "b"
[[game]]
name = "Doom"
exec = "c"
"#;
    let g = games_with_meta(text, &[]);
    let (cnc, doom, quake) = (&g[0], &g[1], &g[2]);
    assert_eq!(cnc.name, "Command & Conquer 3: Kane's Wrath");
    assert_eq!(cnc.title, cnc.name, "no year known yet");
    assert!(cnc.needs_meta(false));
    assert_eq!(quake.name, "Quake", "empty brackets go too");
    assert_eq!((doom.title.as_str(), doom.needs_meta(false)), ("Doom", false));
    assert!(doom.needs_meta(true), "the status bar wants it");

    // cached metadata goes by the name, and the name stays the same
    let g = games_with_meta(text, &[("Command & Conquer 3: Kane's Wrath", 2008), ("Doom", 1993)]);
    assert_eq!(g[0].name, "Command & Conquer 3: Kane's Wrath");
    assert_eq!(g[0].title, "Command & Conquer 3: Kane's Wrath 2008");
    assert!(!g[0].needs_meta(true));
    assert_eq!(g[1].title, "Doom", "a name without {{year}} ignores the cache");
    assert_eq!(g[1].meta.as_ref().and_then(|m| m.year), Some(1993), "but keeps it for the status bar");

    // metadata found later
    let mut quake = g.into_iter().nth(2).unwrap();
    quake.set_meta(Meta { year: Some(1996), ..Meta::default() });
    assert_eq!((quake.title.as_str(), quake.needs_meta(true)), ("Quake (1996)", false));
}

#[test]
fn igdb_data_cached_under_another_igdb_id_is_stale() {
    let text = |id: &str| format!("[[game]]\nname = \"Doom {{year}}\"\nexec = \"a\"\n{id}");
    let cached = |igdb_id| {
        let meta = Meta { year: Some(1993), igdb_id, ..Meta::default() };
        BTreeMap::from([("Doom".to_string(), meta)])
    };
    let doom = |id: &str, igdb_id| build(toml::from_str(&text(id)).unwrap(), cached(igdb_id)).unwrap().games.remove(0);

    assert_eq!(doom("igdb_id = 673", None).igdb_id, Some(673));
    assert!(doom("igdb_id = 673", None).needs_meta(false), "found by name before the id was set");
    assert!(doom("igdb_id = 673", Some(1)).needs_meta(false), "found by a different id");
    assert_eq!(doom("igdb_id = 673", Some(673)).title, "Doom 1993");
    assert!(doom("", Some(673)).needs_meta(false), "the id was removed, so look it up by name");
    assert_eq!(doom("", None).title, "Doom 1993");
}

#[test]
fn a_games_own_year_wins_and_year_is_only_for_names() {
    let g = games("[[game]]\nname = \"Quake {year}\"\nexec = \"a\"\nvars = { year = 1996 }\n").unwrap();
    assert_eq!((g[0].title.as_str(), g[0].needs_meta(false)), ("Quake 1996", false));
    let e = game_err("[[game]]\nname = \"A\"\nexec = \"run {year}\"\n");
    assert!(e.contains("{year} isn't defined"), "{e}");
    let e = game_err("[[game]]\nname = \"A {nope}\"\nexec = \"a\"\n");
    assert!(e.contains("game \"A {nope}\", name"), "{e}");
}

#[test]
fn status_parts_are_checked_against_all_vars() {
    let text = |part: &str| format!("[status]\nleft = [{{ text = \"{part}\" }}]\n[[game]]\nname = \"A\"\nexec = \"a\"\nvars = {{ dir = \"a\" }}\n");
    assert!(games(&text("{title} {dir}")).is_ok(), "a game's own variable is fine");
    let e = game_err(&text("{nope}"));
    assert!(e.contains("[status]") && e.contains("{nope} isn't defined"), "{e}");
    let r = checked(&text("{nope}"));
    assert!(r.errors.iter().any(|e| e.contains("[status]")), "{:?}", r.errors);
}

#[test]
fn topbar_labels_are_checked_like_status_parts() {
    let text = |label: &str| {
        format!("[topbar]\nright = [{{ type = \"label\", text = \"{label}\" }}]\n[[game]]\nname = \"A\"\nexec = \"a\"\nvars = {{ dir = \"a\" }}\n")
    };
    assert!(games(&text("{dir} {env:HOME}")).is_ok());
    let e = game_err(&text("{nope}"));
    assert!(e.contains("[topbar]: right widget 1") && e.contains("{nope} isn't defined"), "{e}");
    let r = checked(&text("{nope}"));
    assert!(r.errors.iter().any(|e| e.contains("[topbar]")), "{:?}", r.errors);
}

#[test]
fn env_is_only_for_the_bars() {
    // a variable used in exec can't sneak it in either
    let e = game_err("[vars]\nmh = \"{env:MANGOHUD}\"\n[[game]]\nname = \"A\"\nexec = \"run {mh}\"\n");
    assert!(e.contains("{env:MANGOHUD} only works in [status] and [topbar]") && e.contains("$MANGOHUD"), "{e}");
    // but it's fine in a status part, through the variable too
    assert!(games("[vars]\nmh = \"{env:MANGOHUD}\"\n[status]\nleft = [{ text = \"{mh}\" }]\n").is_ok());
}

#[test]
fn bar_env_is_what_the_game_launches_with() {
    let g = games("[env]\nA = \"global\"\n[[game]]\nname = \"G\"\nexec = \"g\"\nenv = { PATH = \"/mine\" }\n").unwrap();
    let no_toggles = Toggles::new();
    let env = crate::status::env(Some(&g[0]), &no_toggles);
    assert_eq!((env("A").as_str(), env("PATH").as_str()), ("global", "/mine"), "[env] and the game's own win over ours");
}

const VAR_TOGGLE: &str = r#"
        [vars]
        mango = ""
        gs = "gamescope{mango} -f --"
        [topbar]
        left = [
          { type = "toggle", title = "MangoHud", var = "mango", on = " --mangoapp" },
          { type = "button", title = "Steam", exec = "{gs} steam" },
        ]
        [[game]]
        name = "G"
        exec = "{gs} ./g"
        [[game]]
        name = "Own"
        exec = "{gs} ./own"
        vars = { mango = " --own" }
    "#;

fn steam(topbar: &Topbar) -> &str {
    let crate::topbar::Widget::Button(b) = topbar.widget((0, 1)) else { panic!("a button") };
    let MenuAction::Exec(exec) = &b.action else { panic!("an exec") };
    &exec[0]
}

#[test]
fn var_toggles_switch_vars_for_games_and_buttons_when_built_again() {
    let cfg: Config = toml::from_str(VAR_TOGGLE).unwrap();
    let s = build(cfg, BTreeMap::new()).unwrap();
    let exec = |games: &[Game]| games.iter().map(|g| g.exec[0].clone()).collect::<Vec<_>>();
    assert_eq!(exec(&s.games), ["gamescope -f -- ./g", "gamescope --own -f -- ./own"], "starts off, like [vars]");
    assert_eq!(steam(&s.topbar), "gamescope -f -- steam");

    let mut on = s.topbar;
    on.flip((0, 0));
    let (games, topbar) = s.library.build(Some(&on)).unwrap();
    assert_eq!(exec(&games), ["gamescope --mangoapp -f -- ./g", "gamescope --own -f -- ./own"], "a game's own wins");
    assert_eq!(steam(&topbar), "gamescope --mangoapp -f -- steam");
    assert_eq!(topbar.var_values(), [("mango", " --mangoapp")], "still on");
}

#[test]
fn building_again_for_a_var_toggle_keeps_the_sort() {
    let text = VAR_TOGGLE.replace("left = [", r#"left = [{ type = "sort", options = ["name", "year"] },"#);
    let s = build(toml::from_str(&text).unwrap(), BTreeMap::new()).unwrap();
    let mut on = s.topbar;
    on.set_sort((0, 0), 1);
    on.flip((0, 1));
    let (_, topbar) = s.library.build(Some(&on)).unwrap();
    assert_eq!(topbar.sort(), crate::topbar::SortBy::Year);
}

#[test]
fn a_var_toggle_starts_from_vars_and_its_off_value_applies_from_the_start() {
    let text = VAR_TOGGLE.replace("mango = \"\"", "mango = \" --mangoapp\"");
    let s = build(toml::from_str(&text).unwrap(), BTreeMap::new()).unwrap();
    assert_eq!(s.topbar.var_values(), [("mango", " --mangoapp")]);
    assert_eq!(s.games[0].exec, ["gamescope --mangoapp -f -- ./g"]);

    // [vars] has neither value: it starts off, and off is what games get
    let text = VAR_TOGGLE.replace("mango = \"\"", "mango = \"x\"");
    let s = build(toml::from_str(&text).unwrap(), BTreeMap::new()).unwrap();
    assert_eq!(s.games[0].exec, ["gamescope -f -- ./g"]);
}

#[test]
fn a_var_toggle_cant_change_a_games_name() {
    let text = VAR_TOGGLE.replace("name = \"G\"", "name = \"G{mango}\"");
    let e = build(toml::from_str(&text).unwrap(), BTreeMap::new()).err().unwrap();
    assert!(e.contains("game \"G{mango}\", name: can't use {mango}, a top bar toggle switches it"), "{e}");
    let text = VAR_TOGGLE.replace("name = \"Own\"", "name = \"Own{mango}\"");
    assert!(build(toml::from_str(&text).unwrap(), BTreeMap::new()).is_ok(), "its own mango, the toggle doesn't reach it");
    let report = check_config(toml::from_str(&VAR_TOGGLE.replace("name = \"G\"", "name = \"G{mango}\"")).unwrap());
    assert!(report.errors.iter().any(|e| e.contains("can't use {mango}")), "{:?}", report.errors);
}
#[test]
fn rom_names_drop_region_and_dump_groups() {
    assert_eq!(rom_name("Super Mario World (USA) [!]"), "Super Mario World");
    assert_eq!(rom_name("Zelda (Europe) (Rev 1) - Extra"), "Zelda - Extra");
    assert_eq!(rom_name("Doom (unclosed"), "Doom (unclosed");
    assert_eq!(rom_name("(USA)"), "(USA)", "nothing but groups is kept");
}

#[test]
fn roms_become_games_with_rom_workdir_vars_and_env() {
    let dir = std::env::temp_dir().join(format!("rsgl-roms-test-{}", std::process::id()));
    fs::create_dir_all(dir.join("sub.sfc")).unwrap();
    for f in ["Super Mario World (USA).sfc", "F-Zero {x} [!].SMC", "readme.txt", "noext"] {
        fs::write(dir.join(f), "").unwrap();
    }
    let text = format!(
        r#"
[vars]
cores = "/cores"
roms = {:?}
[env]
G = "global"

[[right_click]]
title = "Shared"
exec = "{{exec}} --menu"

[[roms]]
path = "{{roms}}"
extensions = ["sfc", ".smc"]
exec = 'retroarch -L {{cores}}/snes9x.so "{{rom}}"'
tags = ["snes"]
env = {{ R = "rom" }}
"#,
        dir.to_str().unwrap()
    );
    let g = games(&text).unwrap();
    let names: Vec<_> = g.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["F-Zero {x}", "Super Mario World"], "other extensions and folders are skipped");

    let smw = &g[1];
    let rom = dir.join("Super Mario World (USA).sfc");
    assert_eq!(smw.exec, [format!("retroarch -L /cores/snes9x.so \"{}\"", rom.display())]);
    assert_eq!(smw.workdir.as_deref(), Some(dir.as_path()), "workdir defaults to the folder");
    assert_eq!(smw.tags, ["snes"]);
    assert_eq!((smw.env.all["G"].as_str(), smw.env.all["R"].as_str()), ("global", "rom"));
    assert_eq!(smw.menu[0].action, MenuAction::Exec(vec![format!("{} --menu", smw.exec[0])]), "[[right_click]] applies");
    assert!(g[0].exec[0].contains("F-Zero {x} [!].SMC"), "braces in file names are literal");

    let with_workdir = text.replace("tags = ", "workdir = \"/run\"\ntags = ");
    assert_eq!(games(&with_workdir).unwrap()[0].workdir, Some(PathBuf::from("/run")));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn roms_use_their_templates_but_not_default() {
    let dir = std::env::temp_dir().join(format!("rsgl-roms-templates-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Zelda.nes"), "").unwrap();
    let text = format!(
        r#"
[[right_click]]
title = "Open folder"
action = "open_folder"

[[templates.default.right_click]]
title = "Uninstall"
exec = "rm -rf {{path}}"

[templates.nes]
exec = 'nestopia "{{rom}}"'
tags = ["retro"]

[[templates.nes.right_click]]
title = "Delete ROM"
exec = 'rm "{{rom}}"'

[[game]]
name = "Quake"
exec = "quake"
vars = {{ path = "quake" }}

[[roms]]
path = {:?}
extensions = ["nes"]
template = "nes"
tags = ["nes"]
"#,
        dir.to_str().unwrap()
    );
    let g = games(&text).unwrap();
    let get = |name: &str| g.iter().find(|g| g.name == name).unwrap();
    let titles = |name: &str| -> Vec<String> { get(name).menu.iter().map(|m| m.title.clone()).collect() };
    assert_eq!(titles("Quake"), ["Open folder", "Uninstall"]);
    assert_eq!(titles("Zelda"), ["Open folder", "Delete ROM"], "and {{path}} isn't needed for ROMs");
    let zelda = get("Zelda");
    assert_eq!(zelda.exec, [format!("nestopia \"{}\"", dir.join("Zelda.nes").display())]);
    assert_eq!(zelda.tags, ["retro", "nes"]);
    assert_eq!(zelda.workdir.as_deref(), Some(dir.as_path()), "the folder, without a workdir anywhere");
    let report = check_config(toml::from_str(&text).unwrap());
    assert!(report.errors.is_empty(), "{:?}", report.errors);

    let with_workdir = text.replace("tags = [\"retro\"]", "tags = [\"retro\"]\nworkdir = \"/emu\"");
    let g = games(&with_workdir).unwrap();
    assert_eq!(g.iter().find(|g| g.rom).unwrap().workdir, Some(PathBuf::from("/emu")), "a template's beats the folder");

    let e = game_err(&text.replace("template = \"nes\"", "template = \"snes\""));
    assert!(e.contains("[[roms]]") && e.contains("no template \"snes\""), "{e}");
    let e = game_err(&text.replace("template = \"nes\"", ""));
    assert!(e.contains("[[roms]]") && e.contains("needs an exec, here or in a template"), "{e}");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn hasheous_url_takes_the_server_with_or_without_the_api_path() {
    let api = |text: &str| parse(&format!("[api]\n{text}")).map(|(c, _)| c.api.hasheous_api());
    assert_eq!(api("").unwrap(), "https://hasheous.org/api/v1");
    assert_eq!(api("hasheous_url = \"http://nas:5000\"").unwrap(), "http://nas:5000/api/v1");
    assert_eq!(api("hasheous_url = \"http://nas:5000/\"").unwrap(), "http://nas:5000/api/v1");
    assert_eq!(api("hasheous_url = \"https://h.lan/x/api/v1/\"").unwrap(), "https://h.lan/x/api/v1");
    for bad in ["nas:5000", "ftp://nas", "https://", ""] {
        let e = api(&format!("hasheous_url = \"{bad}\"")).unwrap_err().to_string();
        assert!(e.contains("isn't an http:// or https:// URL"), "{bad}: {e}");
    }
}

#[test]
fn hasheous_names_the_roms_it_identified() {
    let dir = std::env::temp_dir().join(format!("rsgl-roms-hasheous-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let [smw, homebrew, new] = ["smw.sfc", "Homebrew (PD).sfc", "New {x} (USA).sfc"].map(|f| dir.join(f));
    for f in [&smw, &homebrew, &new] {
        fs::write(f, "").unwrap();
    }
    let text = format!("[[roms]]\npath = {:?}\nextensions = [\"sfc\"]\nexec = \"x\"\n", dir.to_str().unwrap());
    let found = api_hasheous::Found { name: "Super Mario World {1}".into(), igdb_id: Some(1070), steamgriddb_id: Some(36886) };
    let known = api_hasheous::Known::of(&[(&smw, Some(found)), (&homebrew, None)]);
    let scan_with = |known| {
        let mut cfg: Config = toml::from_str(&text).unwrap();
        let scan = add_roms(&mut cfg, known).unwrap();
        let games = build(cfg, BTreeMap::new()).unwrap().games;
        (scan.unidentified, games)
    };

    let (unidentified, g) = scan_with(Some(&known));
    assert_eq!(unidentified, [new], "only the one never looked up is left to ask about");
    let names: Vec<_> = g.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["Homebrew", "New {x}", "Super Mario World {1}"], "braces in Hasheous's names are literal");
    assert_eq!((g[2].igdb_id, g[2].art_id.map(|a| a.to_string())), (Some(1070), Some("steamgriddb:36886".into())));

    let (unidentified, g) = scan_with(None);
    assert!(unidentified.is_empty(), "nothing is asked with Hasheous off");
    assert_eq!(g.iter().find(|g| g.name == "smw").map(|g| g.igdb_id), Some(None), "and its names aren't used");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn roms_files_pin_ids_and_hide_files() {
    let dir = std::env::temp_dir().join(format!("rsgl-roms-files-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let [tgm2, neogeo, smw] = ["tgm2.zip", "neogeo.zip", "smw.zip"].map(|f| dir.join(f));
    for f in [&tgm2, &neogeo, &smw] {
        fs::write(f, "").unwrap();
    }
    let text = format!(
        "[[roms]]\npath = {:?}\nextensions = [\"zip\"]\nexec = \"x\"\n\
             files.\"tgm2.zip\" = {{ igdb_id = 1, steamgriddb_id = 2, grid_id = 3, hero_id = 4 }}\n\
             files.\"neogeo.zip\" = {{ hidden = true }}\n\
             files.\"smw.zip\" = {{ igdb_id = 5 }}\n\
             files.\"gone.zip\" = {{ igdb_id = 6 }}\n",
        dir.to_str().unwrap()
    );
    let found = api_hasheous::Found { name: "Super Mario World".into(), igdb_id: Some(1070), steamgriddb_id: Some(36886) };
    let known = api_hasheous::Known::of(&[(&smw, Some(found))]);
    let mut cfg: Config = toml::from_str(&text).unwrap();
    let scan = add_roms(&mut cfg, Some(&known)).unwrap();
    assert_eq!(scan.unidentified, [tgm2], "a hidden file isn't sent to Hasheous");
    assert!(
        matches!(&scan.warnings[..], [w] if w.contains("files") && w.contains("\"gone.zip\"")),
        "a key that matches no file is a warning: {:?}",
        scan.warnings
    );
    let g = build(cfg, BTreeMap::new()).unwrap().games;
    let names: Vec<_> = g.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["Super Mario World", "tgm2"], "neogeo is hidden");
    let ids = |g: &Game| (g.igdb_id, g.art_id.map(|a| a.to_string()), g.grid_id, g.hero_id);
    assert_eq!(ids(&g[1]), (Some(1), Some("steamgriddb:2".into()), Some(3), Some(4)));
    assert_eq!(ids(&g[0]), (Some(5), Some("steamgriddb:36886".into()), None, None), "a pin beats Hasheous's id, key by key");

    let r = checked(&text);
    assert_eq!((r.games, r.hidden), (3, 1));
    assert!(r.warnings.iter().any(|w| w.contains("\"gone.zip\"")), "{:?}", r.warnings);
    let bad = text.replace("hidden = true", "hiden = true");
    assert!(toml::from_str::<Config>(&bad).is_err(), "typos in a file's keys are caught");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn igdb_platform_narrows_the_search_and_changing_it_looks_roms_up_again() {
    let dir = std::env::temp_dir().join(format!("rsgl-roms-platform-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    for f in ["1080 Snowboarding (USA).z64", "pinned.z64"] {
        fs::write(dir.join(f), "").unwrap();
    }
    let games_with = |keys: &str, cached: Option<&str>| {
        let text = format!(
            "[[roms]]\npath = {:?}\nextensions = [\"z64\"]\nexec = \"x\"\nfiles.\"pinned.z64\" = {{ igdb_id = 7 }}\n{keys}",
            dir.to_str().unwrap()
        );
        let mut cfg: Config = toml::from_str(&text).map_err(|e| e.to_string())?;
        add_roms(&mut cfg, None)?;
        let found = |igdb_id| Meta { name: Some("1080° Snowboarding".into()), igdb_id, igdb_platform: cached.map(str::to_string), ..Meta::default() };
        let meta = BTreeMap::from([("1080 Snowboarding".to_string(), found(None)), ("pinned".to_string(), found(Some(7)))]);
        build(cfg, meta).map(|s| s.games)
    };

    let g = games_with("igdb_platform = \"n64\"", Some("n64")).unwrap();
    assert_eq!(g[0].igdb_platform.as_deref(), Some("n64"));
    assert!(g[0].meta.is_some(), "found on the same platform");
    assert_eq!(g[1].igdb_platform, None, "a pinned igdb_id skips the search, so the platform doesn't apply");
    assert!(g[1].meta.is_none(), "and data cached with one is stale");

    let g = games_with("igdb_platform = \"n64\"", None).unwrap();
    assert!(g[0].meta.is_none(), "found on any platform, before igdb_platform was set");
    assert!(g[1].meta.is_some(), "a pinned game's data still holds");
    assert!(games_with("igdb_platform = \"snes\"", Some("n64")).unwrap()[0].meta.is_none(), "found on another platform");
    assert!(games_with("", Some("n64")).unwrap()[0].meta.is_none(), "igdb_platform was taken out");

    let e = games_with("igdb_platform = \" \"", None).err().unwrap();
    assert!(e.contains("igdb_platform is empty"), "{e}");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_roms_name_sets_the_title_with_igdb_name_and_year() {
    let dir = std::env::temp_dir().join(format!("rsgl-roms-name-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    for f in ["tgm2.zip", "1080 Snowboarding (USA).z64"] {
        fs::write(dir.join(f), "").unwrap();
    }
    let text = |name: &str| {
        format!("[[roms]]\npath = {:?}\nextensions = [\"zip\", \"z64\"]\nexec = \"x\"\n{name}", dir.to_str().unwrap())
    };
    let found = |name: &str| Meta { name: Some(name.into()), year: Some(2000), ..Meta::default() };
    let meta = BTreeMap::from([
        ("tgm2".to_string(), found("Tetris: The Grand Master 2 {x}")),
        ("1080 Snowboarding".to_string(), Meta { year: Some(1998), ..Meta::default() }),
    ]);
    let games_with = |name: &str, meta: &BTreeMap<String, Meta>| {
        let mut cfg: Config = toml::from_str(&text(name)).unwrap();
        add_roms(&mut cfg, None).unwrap();
        build(cfg, meta.clone()).unwrap().games
    };

    let g = games_with("name = \"{igdb_name} ({year})\"", &meta);
    let names: Vec<_> = g.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["1080 Snowboarding", "tgm2"], "who a ROM is stays its file's name");
    assert_eq!(g[1].title, "Tetris: The Grand Master 2 {x} (2000)", "braces in IGDB's names are literal");
    assert_eq!(g[1].art_search(), Some("Tetris: The Grand Master 2 {x}"));
    assert!(g[0].meta.is_some(), "IGDB doesn't know it, which is cached too");
    assert_eq!(g[0].title, "1080 Snowboarding (1998)", "{{igdb_name}} is the file's name when IGDB has none");
    assert!(!g[0].needs_meta(false), "so it isn't looked up on every start");

    let mut snowboarding = games_with("name = \"{igdb_name} (N64)\"", &BTreeMap::new()).remove(0);
    assert_eq!(snowboarding.title, "1080 Snowboarding (N64)");
    snowboarding.set_meta(found("1080° Snowboarding"));
    assert_eq!(snowboarding.title, "1080° Snowboarding (N64)");
    assert_eq!(snowboarding.name, "1080 Snowboarding");
    assert_eq!(snowboarding.art_search(), Some("1080° Snowboarding"));
    snowboarding.set_meta(found("1080 SNOWBOARDING"));
    assert_eq!(snowboarding.art_search(), None, "the same name needs no other search");

    let g = games_with("", &meta);
    assert_eq!(g[1].title, "tgm2", "without a name, the title is the file's");
    assert_eq!(g[1].art_search(), Some("Tetris: The Grand Master 2 {x}"), "art still comes by IGDB's name");
    assert!(!g[1].needs_meta(false));

    let mut cfg: Config = toml::from_str(&text("name = \"{nope}\"")).unwrap();
    add_roms(&mut cfg, None).unwrap();
    let e = build(cfg, BTreeMap::new()).err().unwrap();
    assert!(e.contains("name") && e.contains("nope"), "{e}");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_games_name_cant_use_igdb_name_and_isnt_searched_by_it() {
    let e = game_err("[[game]]\nname = \"{igdb_name}\"\nexec = \"a\"\n");
    assert!(e.contains("{igdb_name} only works in a [[roms]] name"), "{e}");
    let text = "[[game]]\nname = \"Quake {year}\"\nexec = \"a\"\n";
    let meta = Meta { name: Some("Quake (1996)".into()), year: Some(1996), ..Meta::default() };
    let g = build(toml::from_str(text).unwrap(), BTreeMap::from([("Quake".to_string(), meta)])).unwrap().games;
    assert_eq!(g[0].title, "Quake 1996");
    assert_eq!(g[0].art_search(), None, "a [[game]] is searched for by the name written");
}

#[test]
fn a_missing_rom_folder_is_a_warning_and_bad_roms_tables_are_errors() {
    let roms = |extra: &str| format!("[[roms]]\npath = \"/definitely/not/here\"\nexec = \"x\"\n{extra}");
    assert!(games(&roms("extensions = [\"sfc\"]")).unwrap().is_empty());
    let r = checked(&roms("extensions = [\"sfc\"]"));
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(r.warnings[0].contains("/definitely/not/here"), "{:?}", r.warnings);

    assert!(game_err(&roms("extensions = []")).contains("extensions is empty"));
    assert!(toml::from_str::<Config>(&roms("extensions = [\"sfc\"]\nextension = 1")).is_err(), "typos are caught");
    let r = checked("[[roms]]\npath = \"{nope}\"\nextensions = [\"sfc\"]\nexec = \"x\"\n");
    assert!(r.errors[0].contains("[[roms]]") && r.errors[0].contains("{nope}"), "{:?}", r.errors);
}

fn load_file(path: &Path) -> Result<Settings, String> {
    let (cfg, _) = read(path)?;
    let included = cfg.included.clone();
    load_cfg(cfg, &|e| at_file(path, &included, e))
}

#[test]
fn include_shares_templates_but_not_their_names() {
    let dir = std::env::temp_dir().join(format!("rsgl-include-templates-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let (root, a, b) = (dir.join("rsgl.toml"), dir.join("a.toml"), dir.join("b.toml"));
    fs::write(&a, "[templates.wine]\nexec = \"wine {exe}\"\n").unwrap();
    fs::write(&b, "[templates.coop]\ntags = [\"coop\"]\n").unwrap();
    fs::write(
        &root,
        "include = [\"a.toml\", \"b.toml\"]\n[[game]]\nname = \"A\"\ntemplate = [\"wine\", \"coop\"]\nvars = { exe = \"a.exe\" }\n",
    )
    .unwrap();
    let s = load_file(&root).unwrap();
    assert_eq!((s.games[0].exec.as_slice(), s.games[0].tags.as_slice()), (&["wine a.exe".to_string()][..], &["coop".to_string()][..]));

    fs::write(&b, "[templates.wine]\ntags = [\"x\"]\n").unwrap();
    let e = read(&root).err().unwrap();
    assert!(e.contains("[templates.wine] is defined in both") && e.contains("a.toml") && e.contains("b.toml"), "{e}");
    fs::write(&b, "").unwrap();
    fs::write(&root, "include = \"a.toml\"\n[templates.wine]\ntags = [\"x\"]\n").unwrap();
    let e = read(&root).err().unwrap();
    assert!(e.contains("[templates.wine] is defined in both") && e.contains("rsgl.toml"), "{e}");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn include_merges_files_with_parent_winning_and_accumulates_games_and_menus() {
    let dir = std::env::temp_dir().join(format!("rsgl-include-test-{}", std::process::id()));
    let sub = dir.join("sub");
    fs::create_dir_all(&sub).unwrap();

    let secrets = dir.join("secrets.toml");
    fs::write(
        &secrets,
        r#"
[api]
steamgriddb_key = "secret-key"

[ui]
card_width = 200
fullscreen = false

[vars]
w = 1920
gs = "gamescope -w {w} --"
"#,
    )
    .unwrap();

    let nested = sub.join("nested.toml");
    fs::write(
        &nested,
        r#"
[[right_click]]
title = "Open folder"
action = "open_folder"

[[game]]
name = "Quake"
exec = "{gs} quake"
"#,
    )
    .unwrap();

    let library = sub.join("library.toml");
    fs::write(
        &library,
        r#"
include = "nested.toml"

[vars]
w = 2560

[[game]]
name = "Doom"
exec = "{gs} doom"
"#,
    )
    .unwrap();

    let main_cfg = dir.join("rsgl.toml");
    fs::write(
        &main_cfg,
        r#"
include = ["secrets.toml", "sub/library.toml"]

[ui]
fullscreen = true

[api]
hasheous = true

[vars]
w = 3440

[[right_click]]
title = "Extra"
exec = "extra"

[[game]]
name = "Unreal"
exec = "{gs} unreal"
"#,
    )
    .unwrap();

    let s = load_file(&main_cfg).unwrap();
    assert_eq!(s.ui.card_width, 200.0, "kept from secrets.toml");
    assert!(s.ui.fullscreen, "main file wins over secrets.toml");
    assert_eq!(s.api.steamgriddb_key.as_deref(), Some("secret-key"));
    assert!(s.api.hasheous, "[api] tables merge key by key");
    let names: Vec<_> = s.games.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["Doom", "Quake", "Unreal"]);
    assert_eq!(s.games[0].exec, ["gamescope -w 3440 -- doom"], "main file's w = 3440 wins over library.toml and secrets.toml");
    let menu_titles: Vec<_> = s.games[0].menu.iter().map(|m| m.title.as_str()).collect();
    assert_eq!(menu_titles, ["Open folder", "Extra"], "included right_click first, then main file's");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn include_catches_cycles_and_reports_the_included_file_on_errors_and_warnings() {
    let dir = std::env::temp_dir().join(format!("rsgl-include-err-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();

    let (a, b) = (dir.join("a.toml"), dir.join("b.toml"));
    fs::write(&a, "include = [\"b.toml\"]\n").unwrap();
    fs::write(&b, "include = [\"./a.toml\"]\n").unwrap();
    let e = read(&a).err().unwrap();
    assert!(
        e.contains("files include each other in a circle") && e.contains("a.toml") && e.contains("b.toml"),
        "{e}"
    );

    let bad_games = dir.join("games.toml");
    fs::write(
        &bad_games,
        r#"
[evn]
K = "1"

[[game]]
name = "Broken"
exec = "{undefined}"

[[game]]
name = "MissingDir"
exec = "x"
workdir = "/rsgl-test/no/such/dir"
"#,
    )
    .unwrap();
    let root = dir.join("rsgl.toml");
    fs::write(&root, "include = [\"games.toml\"]\n").unwrap();

    // loading names games.toml, not rsgl.toml
    let e = load_file(&root).err().unwrap();
    assert!(
        e.starts_with(&format!("{}: game \"Broken\"", bad_games.display())),
        "got: {e}"
    );

    // check() attributes both the error, the workdir warning and the unknown key to games.toml
    let (cfg, unknown) = read(&root).unwrap();
    let mut report = check_config(cfg);
    report.warnings.extend(unknown);
    let prefix = format!("{}: ", bad_games.display());
    assert_eq!(report.errors.len(), 1);
    assert!(report.errors[0].starts_with(&prefix), "{:?}", report.errors);
    assert_eq!(report.warnings.len(), 2, "{:?}", report.warnings);
    assert!(report.warnings.iter().all(|w| w.starts_with(&prefix)), "{:?}", report.warnings);

    fs::remove_dir_all(&dir).unwrap();
}
