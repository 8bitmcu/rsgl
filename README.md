# rsgl

A highly configurable game launcher for Linux and Windows that you can drive
with a keyboard, a gamepad or a mouse. You list your games in a TOML file, and
rsgl shows them as a grid of cover art. Type to filter, pick one with the arrow
keys, the D-pad or the mouse, launch it, and the launcher starts the game and
closes.

If you give it a [SteamGridDB](https://www.steamgriddb.com/) API key, it
downloads missing covers and hero images for you, then draws a blurred version
of the selected game's art behind the grid. With [IGDB](https://www.igdb.com/)
credentials, it also fills in release years and genres, which you can show in
titles and the status bar and search with `g:`.


![preview](assets/preview.gif)

## Features

- Cover-art grid with search-as-you-type filtering and optional autocomplete
- Variables (`{name}`), [templates](#templates) and a global `[env]`, so paths,
  launch commands and environment settings aren't repeated in the config
- Configurable [top bar](#top-bar) with buttons, dropdown menus, environment and
  variable toggles, sorting (`name`, `last_played`, `playtime`, `year`) and
  category filters
- Command-line `--launch`, `--random`, `--list`, `--show`, `--fetch`, `--check`, `--igdb` and `--igdb-matches`, for hotkeys and scripts
- Tags, with `t:tag` filtering in the search bar, and `g:genre` with IGDB genres
- ROM folders: every file in a folder becomes a game, with optional
  [Hasheous](#hasheous) hash identification (see [ROMs](#roms))
- Installed [Steam](#steam) and [Lutris](#lutris) games, imported from the
  launcher's own files, with your own tags, templates and settings per game
- Configurable right-click menu per game: tools, or alternate ways to launch
- Steam-like installs, updates and save syncing from your own server, all in
  config (see [Recipes](#recipes))
- Full keyboard and gamepad control with rebindable keys and buttons, mouse works too
- Automatic cover and hero downloads from SteamGridDB (optional)
- Blurred, crossfading background for the selected game
- Per-game working directory and environment variables
- Games are started detached, so they keep running after the launcher closes
- Playtime tracking per game, shown in the status bar
- A [status bar](#status-bar) you can rearrange with templates, including
  release year and genres from IGDB (optional)
- Single instance mode, so a second launcher window can't be opened
- Command-line flags for fullscreen, an alternate config file, a starting search
  and a starting filter (`--filter`)
- Games without a cover get a colored placeholder with their initial

## Building

You need a recent stable Rust toolchain.

```sh
cargo build --release
./target/release/rsgl
```

On Linux, gamepad support needs the libudev headers to build: `libudev-dev` on
Debian/Ubuntu, `systemd-devel` on Fedora.

rsgl uses `eframe`/`egui` with OpenGL. On Linux it supports both Wayland and
X11. For window manager rules, the window title is `Games` and its app id
(`WM_CLASS` on X11) is `rsgl`. On Windows it builds with the normal MSVC
toolchain (`x86_64-pc-windows-msvc`); see [Windows](#windows) for the
differences.

## Configuration

rsgl reads the first of these files that exists:

1. `rsgl.toml` in the same folder as the rsgl executable (see
   [Portable setup](#portable-setup))
2. `$XDG_CONFIG_HOME/rsgl/rsgl.toml` (usually `~/.config/rsgl/rsgl.toml`;
   on Windows `%APPDATA%\rsgl\rsgl.toml`)
3. `~/.rsgl.toml`

Pass `-c <file>` to use a different file instead (see [Command line](#command-line)).

If none exists, the first run writes an example config to
`~/.config/rsgl/rsgl.toml` and shows a message telling you to edit it and
press F5.

F5 reloads the config without restarting. Games, `card_width`, `blur`, the
`[api]` keys, `[keyboard]`, `[gamepad]`, `[status]` and `[topbar]` apply right away; `single_instance`,
`start_maximized`, `fullscreen`, `reopen_after_game` and `[playtime]` need a
restart. If the file has an error, the current list stays up and the
error is shown above it.

Unknown keys in the top-level config, `[ui]`, `[api]`, `[paths]`, `[playtime]`
and `[[game]]` are ignored with a warning in the terminal and in
[`--check`](#command-line), so a typo or a key from a newer version doesn't
stop the library loading. Menu entries, `[[roms]]`, `[steam]`, `[lutris]`, `[templates]` and
top bar widgets reject unknown keys as errors.

```toml
# Optional: merge in other config files (relative to this file's folder).
# Settings in this file win over included ones; [[game]], [[roms]] and [[right_click]] add up.
include = ["secrets.toml", "games.toml"]

[ui]
card_width = 170   # card width in pixels, clamped to 80-400
blur = 8           # background blur strength, 0 turns blur off (max 50)
single_instance = true   # refuse to start while another launcher window is open
start_maximized = false  # open the window maximized
fullscreen = false       # open the window fullscreen, like -f
reopen_after_game = false   # open the launcher again when a game launched from it exits

[playtime]
enabled = true           # false turns tracking and the status bar display off
checkpoint_minutes = 5   # save a running session every N minutes, 0 = only on exit

[api]
steamgriddb_key = "your-key-here"
igdb_client_id = "your-client-id"          # IGDB metadata, fills in {year} and {genres}
igdb_client_secret = "your-client-secret"
hasheous = false         # name [[roms]] files by their contents, see ROMs
hasheous_url = "http://nas:5000"           # a self-hosted Hasheous, default hasheous.org

[env]
WINEDEBUG = "-all"       # set for every game; a game's own env wins

[[game]]
name = "Example Game"
exec = "~/games/example/start.sh"            # or ["before", "game", "after"]
template = "wine"                            # optional, or ["wine", "coop"], see Templates
cover = "~/.cache/rsgl/covers/example.png"   # optional
workdir = "~/games/example"                  # optional
env = { SOME_VAR = "value" }                 # optional
playtime = false                             # optional, don't track this game
hidden = true                                # optional, keep the entry but leave it out of the grid
tags = ["coop", "lan", "fps"]                # optional, filter with t:coop in the search bar
steam_appid = 367520                         # optional, find art by Steam app id
steamgriddb_id = 1234                        # optional, find art by SteamGridDB game id
grid_id = 664940                             # optional, use this SteamGridDB grid as the cover
hero_id = 15208                              # optional, use this SteamGridDB hero for the background
igdb_id = 707                                # optional, get {year} and {genres} from this IGDB game
vars = { dir = "example" }                   # optional, this game's variables, see Variables
```

Notes on `[[game]]` entries:

- `exec` runs through `sh -c` (`cmd /C` on Windows), so arguments, quoting and
  pipes all work. It can also be a list of commands (`exec = ["...", "..."]`),
  which rsgl runs one after another in `workdir` with `env`, waiting for each
  step's process group (`cmd` process on Windows) to exit before starting the
  next. That lets you put pre-launch fixes or save pulls before the game command
  and post-exit tasks like pushing saves after it, without chaining them into
  one long command line; only the step currently running is live in the process
  list, and a step that exits non-zero (like a `sed` on a config file the game
  hasn't created yet) doesn't stop the next step from starting. `[[roms]]`,
  `[[right_click]]` and top bar buttons and menus accept a list too.
- If a game can't be started (for example its `workdir` doesn't exist), rsgl
  stays open and shows the reason in red above the grid until you press F5, and
  prints it to the terminal. A command the shell can't find is only noticed
  after the shell has already started, so that case still closes rsgl; start
  rsgl from a terminal to see the shell's message (see [Game output](#game-output)).
  Menu entries report failures the same way.
- A leading `~` is expanded in `exec`, `cover`, `workdir` and `env` values.
- Games are sorted by name (case-insensitive), not by their order in the file.
- If `cover` is missing or doesn't exist, rsgl looks in the covers cache (see
  below) for a file named after the game.
- Covers are normally found by searching SteamGridDB for the game's name, which
  sometimes picks the wrong game. Searches ask for static, non-NSFW, non-humor
  images, trying portrait grids (`600x900`, `342x482`, `660x930`) first for
  covers before falling back to any size. Set `steam_appid` or `steamgriddb_id`
  (the number in the game's SteamGridDB URL) to skip the search. If both are
  set, `steamgriddb_id` wins. Adding or changing an id re-downloads the cached
  cover and hero once, replacing what's in the cache.
- To pick the exact image instead of the best-scored one, set `grid_id` (cover)
  or `hero_id` (background) to the number in the image's SteamGridDB URL, e.g.
  `steamgriddb.com/grid/664940`. Each works on its own and wins over the game
  ids above for that image; the other one is still found as usual. Changing
  one re-downloads just that image. Only png, jpeg and webp images work; any
  other format (an animated gif, say) is reported in the terminal and skipped.
  A `cover = ...` file that exists still wins over `grid_id`.
- The background behind the grid is the game's blurred hero image, or its
  blurred cover when it has no hero (including local covers and heroes when no
  SteamGridDB key is set). With `blur = 0`, the original image is drawn at full
  resolution instead of caching a blurred copy.
- `igdb_id` does the same for IGDB: the year and genres come from that game
  instead of a search by name. See [IGDB credentials](#igdb-credentials).
- `hidden = true` hides a game without deleting its entry.
- A game whose `workdir` doesn't exist is drawn dimmed and marked "not
  installed" in the status bar. A game without a `workdir` is never dimmed.
  The grid updates when a right-click tool exits, so an install or uninstall
  entry shows up right away.
- `{name}` placeholders in `name`, `exec`, `workdir`, `cover` and `env` are filled in from
  `[vars]`, see [Variables](#variables).
- `exec` is required, unless a [template](#templates) the game uses sets it.
- `tags` is a list of labels for the game, see [Tags](#tags).
- `[[game.right_click]]` adds right-click menu entries, see
  [Right-click menu](#right-click-menu).

### Right-click menu

Right-click a game (or press Shift+F10 on the selected one) to get a menu you
define yourself. Each entry is either a **tool** for the game (open its config,
run a setup program) or an **alternate launch** (windowed, with mods, ...).

```toml
# shown for every game, before the game's own entries
[[right_click]]
title = "Open game folder"
action = "open_folder"

[[game]]
name = "Quake"
exec = "~/games/quake/start.sh"
workdir = "~/games/quake"
env = { SDL_VIDEODRIVER = "x11" }
tags = ["fps"]            # the game's own keys go above its menu entries

[[game.right_click]]
title = "Play (windowed)"
exec = "./start.sh -windowed"
launch = true             # an alternate launch, see below

[[game.right_click]]
title = "Edit config"
exec = "xdg-open ~/.config/quake/config.cfg"
workdir = "~"
env = { EDITOR = "kate" }
```

| Key       | Meaning                                                                          |
| --------- | -------------------------------------------------------------------------------- |
| `title`   | Text shown in the menu. Required.                                                |
| `exec`    | Command to run, like a game's `exec`. Needs `exec` or `action`, not both.        |
| `action`  | A built-in instead of `exec`. The only one is `"open_folder"`: open the workdir. |
| `workdir` | Optional. Defaults to the game's `workdir`.                                      |
| `env`     | Optional. Added to the game's `env`; the entry wins if both set a variable.      |
| `vars`    | Optional. [Variables](#variables) for this entry, on top of the game's.          |
| `launch`  | Optional, default `false`. See below.                                            |

- **Tools** (`launch = false`) run in the background and rsgl stays open. They
  don't count toward playtime.
- **Alternate launches** (`launch = true`) behave like pressing Enter on the
  game: rsgl closes and the playtime is tracked for that game (unless playtime
  is off, globally or for the game).
- Entries run exactly like a game's `exec`: through the shell, with `~`
  expanded, and printing to your terminal if you started rsgl from one.
- **`{exec}`** is the game's `exec` as written, with the entry's `vars` filled
  in. That makes a shared alternate launch a few lines, for every game:

  ```toml
  [vars]
  gs = "gamescope -w 3440 -h 1440 --"

  [[right_click]]
  title = "Launch without gamescope"
  exec = "{exec}"          # the game's "{gs} wine game.exe" becomes " wine game.exe"
  vars = { gs = "" }
  launch = true
  ```

  An entry's `vars` only reach its own `exec`, `workdir` and `env` (and `{exec}`);
  the `workdir` and `env` it gets from the game stay as the game has them. When
  the game's `exec` is a list, `exec = "{exec}"` (or `"{exec}"` as an item in
  the entry's own `exec` list) keeps it as a list and fills the entry's `vars`
  into each step.
- Entries also know about the game they're shown for:

  | Variable    | Value                                                                  |
  | ----------- | ---------------------------------------------------------------------- |
  | `{name}`    | The game's name, without [`{year}`](#variables)                         |
  | `{grid}`    | Full path of its cover (`cover`, or the downloaded one); blank if none  |
  | `{hero}`    | Full path of its downloaded hero; blank if none                         |
  | `{workdir}` | The game's `workdir`, even if the entry has its own; blank if none      |
  | `{exec}`    | The game's `exec`, see above                                            |

  `{grid}` and `{hero}` are what's on disk when the config is loaded, so art
  downloaded since shows up after F5. A variable of your own with one of these
  names wins. They're filled in as they are, so quote them for the shell: a
  name like "Kane's Wrath" breaks inside single quotes, so use double quotes:

  ```toml
  [[right_click]]
  title = "Uninstall"
  exec = '''test -n "{path}" && zenity --question --text="Delete {name}?" && rm -rf "{games}/{path}"'''
  workdir = "{games}"
  ```
- A top-level `[[right_click]]` applies to every game and every ROM, and comes
  first. Then come the entries from the game's [templates](#templates), then
  the game's own. For entries that only make sense for one kind, use a
  template: `[[templates.default.right_click]]` is shown for every `[[game]]`
  and never for ROMs, and a template your [`[[roms]]`](#roms) list holds the
  ROM-only ones. An entry isn't filled in for a game it isn't shown for, so a
  game-only entry can use `{path}` without ROMs defining it, and a ROM-only one
  can use `{rom}`:

  ```toml
  [[templates.default.right_click]]   # every [[game]], never ROMs
  title = "Uninstall"
  exec = '''test -n "{path}" && zenity --question --text="Delete {name}?" && rm -rf "{games}/{path}"'''
  workdir = "{games}"

  [[templates.snes.right_click]]      # the [[roms]] with template = "snes"
  title = "Delete ROM"
  exec = '''zenity --question --text="Delete {name}?" && rm "{rom}"'''
  ```

  `for = "game"` / `for = "rom"` from older versions is gone; an entry that
  still has it is an error that says where to move it.
 A game with no entries at all just opens its `workdir`
  when you right-click it. Otherwise the menu always opens, even for one entry.
- `action = "open_folder"` is greyed out when there's no `workdir` to open, and
  works the same on Windows, where `xdg-open` doesn't exist.
- Every key below a `[[game.right_click]]` header belongs to that entry, so put
  the game's own keys (`tags`, `hidden`, ...) above it. A key in the wrong place
  is reported as an error instead of being ignored.
- In the menu: up/down move, launch runs, back closes (arrows, Enter and Esc
  unless you [rebound them](#key-bindings)). Clicking outside closes it
  without launching the game under the pointer.

### Game output

If you start rsgl from a terminal, the game's stdout and stderr go to that
terminal too, even after the launcher window has closed. That makes it easy to
see why a game won't start. If rsgl was started some other way (a desktop
entry, a hotkey), the output is discarded as before.

- The game's stdin is never connected, since the game runs in the background
  and the terminal would stop it as soon as it tried to read.
- Ctrl-C in the terminal stops rsgl (and its playtime tracking), not the game,
  because the game runs in its own process group.
- With playtime tracking on, the shell prompt doesn't come back until the game
  exits, since rsgl stays running to time it.

### Variables

Put values you repeat in a `[vars]` table and use them as `{name}`:

```toml
[vars]
games = "~/.wine/drive_c/games"
w = 3440
h = 1440
gs = "gamescope -w {w} -h {h} -f --"

[[game]]
name = "FlatOut 2"
exec = "{gs} wine FlatOut2.exe"
workdir = "{games}/FlatOut2"

[[game]]
name = "Command & Conquer 3: Tiberium Wars"
vars = { w = 2560 }              # {gs} now means 2560x1440 for this game only
exec = "{gs} wine CNC3.exe"
workdir = "{games}/CommandConquer3TiberiumWars"
```

- **Where they apply:** `name`, `exec`, `workdir`, `cover` and `env` values, and
  the same fields of right-click menu entries. Not `tags` or a menu `title`.
- **`{year}`** is the game's release year from [IGDB](#igdb-credentials), and only
  works in `name` (and the [status bar](#status-bar)):
  `name = "Command & Conquer 3: Kane's Wrath {year}"`. It's blank
  until it's known (empty `()` or `[]` around it are dropped too). rsgl still knows
  the game by its name without the year, so playtime, cached art and `--launch`
  don't change when the year shows up. If IGDB picks the wrong game, set
  [`igdb_id`](#igdb-credentials); a game's own `vars = { year = 2008 }` also wins
  over IGDB, but only for the year.
- **Overrides:** a game's own `vars` win over `[vars]`. A variable can use other
  variables, and they're looked up after the two tables are merged, so the
  game's `w = 2560` above reaches the `{w}` inside `gs`.
- **Toggles:** a [top bar toggle](#top-bar) with `var` switches a `[vars]`
  value from the window, like `--mangoapp` in `gs` above.
- **Menu entries** see the variables of the game they're shown for. A shared
  `[[right_click]]` entry can use a per-game variable like `{dir}`, but then
  every game must define it. An entry's own `vars` win over the game's, and
  `{exec}`, `{name}`, `{grid}`, `{hero}` and `{workdir}` are the game's, see
  [Right-click menu](#right-click-menu).
- **Values** can be strings, numbers or booleans. Arrays and tables aren't
  allowed.
- **Literal braces:** write `{{` and `}}`, for example `awk '{{print $1}}'`.
- **Mistakes are errors**, shown in the red line on F5 with the game and field:
  an undefined `{name}`, variables that use each other in a circle, a stray or
  unclosed brace. Hidden games aren't checked.
- **Order:** variables are filled in first, then a leading `~` is expanded, so
  `games = "~/..."` works in `workdir`.
- Variables only exist inside the config. They aren't passed to the game as
  environment variables; use `env` for that.
- rsgl starts every game in its `workdir`, so `exec` can use paths relative to
  it (`wine FlatOut2.exe`, `./UT2004`) instead of repeating the full path.
- **Same workdir pattern for every game:** put it in
  [`[templates.default]`](#templates) once, and each game only needs its
  folder name.

To see what a game will actually run after all the substitution, use
[`--show`](#command-line).

### Environment

A top-level `[env]` table sets environment variables for every game and every
menu entry, so settings like wine's don't have to be repeated:

```toml
[env]
WINEDEBUG = "-all"
WINEPREFIX = "{prefix}/{dir}"

[[game]]
name = "Quake"
exec = "wine quake.exe"
vars = { dir = "quake" }
env = { WINEDEBUG = "err+all" }   # this game's own value wins
```

- A game's own `env` wins per variable, and a menu entry's `env` wins over both.
- Values can use `{variables}` (the game's own, so a global value that uses
  `{dir}` needs every game to define it) and a leading `~` is expanded.
- Values must be strings, and there's no way to *unset* a global variable for one
  game; set it to `""` to blank it. A [top bar toggle](#top-bar) can switch one
  for every game.
- `--show` prints the merged result.

### Templates

A template is a named set of game keys that the games listing it share, so 40
Wine games don't each repeat their `exec`, `env`, tags and menu entries:

```toml
[vars]
games = "~/games"
gs = "gamescope -w 2560 -h 1440 -f --"

[templates.default]              # every [[game]] gets this one first
workdir = "{games}/{dir}"

[templates.wine]
exec = '{gs} wine "{exe}"'
env = { WINEPREFIX = "{prefix}", WINEDEBUG = "-all" }
vars = { prefix = "~/.wine/{dir}" }
tags = ["pc", "wine"]

[[templates.wine.right_click]]
title = "winecfg"
exec = "winecfg"

[templates.coop]
tags = ["coop"]

[[game]]
name = "Hollow Knight"
template = "wine"
vars = { dir = "hollow-knight", exe = "hollow_knight.exe" }

[[game]]
name = "Deep Rock Galactic"
template = ["wine", "coop"]
vars = { dir = "drg", exe = "FSD.exe", prefix = "~/.wine-drg" }  # its own prefix wins
env = { DXVK_HUD = "fps" }                                       # added to the template's env
steam_appid = 548430
```

Deep Rock Galactic runs `{gs} wine "FSD.exe"` in `~/games/drg`, with
`WINEPREFIX=~/.wine-drg`, `WINEDEBUG=-all` and `DXVK_HUD=fps`, the tags
`pc wine coop`, and winecfg in its right-click menu.

- **Order:** `[templates.default]` first, then the game's `template` list left
  to right, then the game's own keys. Each wins over the ones before it.
- **Merging:** `exec`, `workdir`, `cover` and `playtime` are replaced whole.
  `env` and `vars` merge key by key. `tags` and `right_click` add up, in that
  order, with a tag only listed once.
- **Variables are filled in after merging,** so `{dir}` in a template is each
  game's own. Every game that uses a template must define the variables it
  needs; the error names the game.
- **`[templates.default]`** applies to every `[[game]]` and never to
  [`[[roms]]`](#roms) files or [Steam](#steam) and [Lutris](#lutris) games. A game can't opt out of it, but `workdir = ""`
  gives a game no workdir at all.
- **`[[roms]]`**, **`[steam]`** and **`[lutris]`** use templates the same way, with
  `template = "..."` on the block. That's where a ROM-only or Steam-only `exec`
  or menu entry goes.
- **Templates can set** `exec`, `workdir`, `cover`, `env`, `playtime`, `tags`,
  `vars` and `right_click`. Not `name`, `hidden` or the ids (`steam_appid`,
  `igdb_id`, ...), which belong to one game, and not `template`: a template
  can't use another one. Anything else is an error.
- **A template name can only be defined once**, even across
  [included](#configuration) files.
- `--show` prints a game with its templates applied.
- `[defaults]` from older versions is gone: rename it to `[templates.default]`.

### Tags

Give a game any number of `tags` and filter by them from the search bar with
the `t:` prefix:

```toml
[[game]]
name = "Left 4 Dead 2"
exec = "..."
tags = ["coop", "fps"]
```

| Search            | Shows                                                |
| ----------------- | ---------------------------------------------------- |
| `t:coop`          | games tagged `coop`                                  |
| `t:coop t:fps`    | games that have both tags                            |
| `t:coop left`     | `coop` games with "left" in the name                 |
| `t:co`            | tags that start with `co` (`coop`, `couch`, ...)     |

- Tag matching is case-insensitive and matches the start of a tag, so the grid
  narrows as you type. A bare `t:` doesn't filter anything yet.
- Anything in the search that isn't a `t:` or `g:` word is joined with spaces
  and matched as one phrase against the title shown on the card (including
  `{year}` and a ROM's [`name`](#roms)): `mario world` finds "Super Mario
  World", `world mario` doesn't. Extra spaces are ignored.
- Tags can't contain spaces, since the search splits on whitespace. Use
  something like `local-mp` or `local_mp`.
- The selected game's tags are shown in the status bar.

### ROMs

Instead of one `[[game]]` per ROM, point `[[roms]]` at a folder and every file
with one of the `extensions` becomes a game:

```toml
[vars]
cores = "~/.config/retroarch/cores"

[[roms]]
path = "~/roms/snes"
extensions = ["sfc", "smc", "zip"]
exec = 'retroarch -L {cores}/snes9x_libretro.so "{rom}"'
tags = ["snes"]                  # optional, so t:snes shows just these
name = "{igdb_name} (SNES)"      # optional, what the card shows, see below
igdb_platform = "snes"           # optional, only match IGDB games on this platform
workdir = "~/roms"               # optional, defaults to path
env = { SOME_VAR = "value" }     # optional
vars = { core = "snes9x" }       # optional, like a game's own vars
files."F-Zero (USA).sfc" = { igdb_id = 1234 }   # optional, one file's settings
files."bios.sfc" = { hidden = true }
```

- The name is the file name without its extension and without groups in
  parentheses or brackets, so `Super Mario World (USA) [!].sfc` is "Super
  Mario World". That's what IGDB is searched for, and what playtime and
  `--launch` go by.
- `name` sets what the card shows. It can use the ROM's vars, `{year}` and
  `{igdb_name}`, IGDB's name for the game: `"{igdb_name} ({year})"` shows
  `tgm2.zip` as "Tetris: The Grand Master 2 (2000)". Until IGDB has a name,
  `{igdb_name}` is the file's. It only changes what's shown: playtime, art and
  `--launch` still go by the file's name, so adding it later loses nothing.
  `{igdb_name}` needs [IGDB credentials](#igdb-credentials), and `rsgl
  --igdb-matches` shows what IGDB picked for each ROM.
- With IGDB credentials, a ROM's art is searched on SteamGridDB by IGDB's name
  rather than the file's, since file names often miss ("1080 Snowboarding" is
  "1080° Snowboarding"). The first time IGDB names a ROM, the games reload once
  the lookups are through, to fetch its art again.
- `igdb_platform` limits the IGDB search to one platform, so a remake or a
  same-named game on another system isn't picked. It takes IGDB's slug for the
  platform, the last part of its page URL (`igdb.com/platforms/n64` is `n64`):
  `nes`, `snes`, `n64`, `gb`, `gbc`, `gba`, `arcade` and so on. A slug IGDB
  doesn't have is reported once in the terminal, and those ROMs get no IGDB data;
  `rsgl --igdb-matches` stops on it. Adding or changing it looks the folder's
  ROMs up again on the next start. Files with a pinned `igdb_id` aren't searched,
  so it doesn't apply to them.
- `files` sets things for single files, keyed by the exact file name:
  `igdb_id`, `steamgriddb_id`, `grid_id` and `hero_id` work like on a
  `[[game]]`, and `hidden = true` leaves a file that isn't a game (a BIOS zip)
  out of the grid. Use them when IGDB or SteamGridDB picks the wrong game;
  `igdb_id` alone is usually enough, since it fixes `{igdb_name}`, `{year}` and
  the art search. An id here beats Hasheous's. A file name that matches no
  file is a warning, not an error. Keep the dotted `files."x" = {...}` form:
  a `[roms.files."x"]` header works too, but only after the block's other keys.
- `{rom}` is the file's full path. Quote it in `exec`, since ROM names have
  spaces.
- `template = "..."` (or a list) works like on a `[[game]]`, see
  [Templates](#templates), so one emulator setup can serve several folders;
  `exec` can come from it instead. `[templates.default]` isn't applied.
- `[vars]`, `[env]` and `[[right_click]]` apply like for any game, and `path`
  can use `[vars]` too. [`[templates.default]`](#templates) doesn't, so an
  Uninstall entry there never reaches a ROM; put ROM-only entries in a template
  the block lists. The workdir is the block's own, else its templates', else
  the folder (`workdir = ""` gives no workdir).
- Only the folder itself is scanned, not its subfolders. Extensions match in
  any case, with or without a leading `.`.
- The folder is scanned on start and on F5. One that can't be read (say, a
  drive that isn't plugged in) is a warning in the terminal and in `--check`,
  and the rest of the library still loads.
- Two ROMs that end up with the same name (`Game (USA)` and `Game (Europe)`)
  share playtime and art; `--check` warns about it.
- ROMs track [playtime](#playtime) unless one of the block's templates sets
  `playtime = false`; `[[roms]]` itself has no `playtime` key.

#### Hasheous

With `hasheous = true` under `[api]`, rsgl identifies ROMs by their contents on
[Hasheous](https://hasheous.org), which matches files against the No-Intro,
Redump and TOSEC sets. An identified ROM gets the game's proper name instead of
its file name, and the IGDB and SteamGridDB ids Hasheous has for it, so art and
`{year}` don't depend on a name search. It's off by default, since every ROM is
read once to hash it and the hashes are sent to hasheous.org. No key is needed.

- ROMs are hashed (SHA-1) and looked up in the background when the window opens.
  Once that's done, the games are loaded again with their new names, like F5.
  `--launch` and `--show` only use what's already been looked up; `--check`
  checks ROMs by their file names without reading `hasheous.toml`. Hidden files
  (`hidden = true`) aren't hashed or looked up.
- What Hasheous says is cached in `~/.cache/rsgl/hasheous.toml` by file path, so
  each ROM is only hashed once. A file that changes is looked up again, and so is
  one whose lookup failed (no network); a file Hasheous doesn't know is cached as
  unknown and keeps its file name.
- A `.zip` is matched by the one file in it. A zip with several files keeps its
  file name.
- A ROM's name is what its playtime and art go by, so turning Hasheous on (or a
  ROM getting identified) starts it over under the new name.
- `hasheous_url` points rsgl at your own Hasheous instead, e.g. the one Gaseous
  runs; it's the server's address, with or without `/api/v1`. A URL that isn't
  a Hasheous API stops the lookups with a warning instead of marking every ROM
  unknown. ROMs already cached aren't asked again after switching servers;
  delete `hasheous.toml` for that.

### Steam

`[steam]` makes every installed Steam game a game. It reads Steam's own files
(`libraryfolders.vdf` and each game's `appmanifest_<appid>.acf`), so there's no
API key or login, and only installed games show up.

```toml
[steam]
path = "~/.local/share/Steam"           # optional, see below
template = "steam"                      # optional, or a list, like on a [[game]]
exec = "steam steam://rungameid/{appid}"  # optional, this is the default
tags = ["steam"]                        # optional
workdir = "{installdir}"                # optional, this is the default
env = { SOME_VAR = "value" }            # optional, see below
vars = { x = "y" }                      # optional, like a game's own vars
games.1245620 = { tags = ["souls"], template = "coop" }   # optional, one game's settings
games.570 = { hidden = true }
games.620 = { name = "Portal 2 (co-op)", igdb_id = 72 }

[[steam.right_click]]                   # optional, like a game's menu entries
title = "Store page"
exec = "xdg-open https://store.steampowered.com/app/{appid}"
```

- Without `path`, Steam is looked for in `~/.local/share/Steam`, `~/.steam/steam`
  and the flatpak's `~/.var/app/com.valvesoftware.Steam/.local/share/Steam`; on
  Windows, in `Program Files (x86)\Steam`. Every library folder Steam knows
  about is read, not just the one in `path`.
- Each game gets `{appid}` and `{installdir}` (the full path of its folder) as
  variables, and `steam_appid`, so its art is found by app id rather than by name.
  The name is Steam's, without ™ and ®.
- The default `exec` is `steam steam://rungameid/{appid}`. For the flatpak it's
  `flatpak run com.valvesoftware.Steam steam://rungameid/{appid}`, and on Windows
  `start "" steam://rungameid/{appid}`.
- **`env` only reaches the `steam` command, not the game**: the Steam client
  that's already running starts the game. For things like `MANGOHUD=1 %command%`,
  use the game's launch options in Steam. (`exec` can also run the game itself
  from `{installdir}`, then `env` works as usual.)
- [Playtime](#playtime) is off by default, since `steam steam://...` returns
  right away. `playtime = true` turns it back on, say for an `exec` that runs the
  game directly.
- `games.<appid>` sets things for one game, by app id (the number in its store
  URL). It takes any `[[game]]` key except `steam_appid`: `name` renames the game
  (playtime, art and `--launch` go by the new name), `hidden = true` leaves it
  out, `igdb_id` fixes a wrong IGDB match and so on. An app id that isn't
  installed is a warning, not an error.
- Keys merge like [templates](#templates), each over the ones before:
  `[steam]`'s templates, `[steam]`'s own keys, the game's `games.<appid>`
  templates, then its `games.<appid>` keys. So tags add up: `tags = ["souls"]`
  above adds to `steam` rather than replacing it. `[templates.default]` isn't
  applied.
- Proton, the Steam Linux Runtime and the Steamworks redistributables are
  installed like games but skipped. A `games.<appid>` entry for one brings it
  back.
- Steam is read on start and on F5. Steam not being installed, or a library
  folder that can't be read (a drive that isn't plugged in), is a warning in the
  terminal and in `--check`, and the rest of the library still loads.
- A `[[game]]` with the `steam_appid` of an installed Steam game is kept, and
  so is the import, so it shows twice; `--check` and the terminal warn about it.
  Hide the one you don't want, the import with `games.<appid> = { hidden = true }`.

### Lutris

`[lutris]` makes every game installed through Lutris a game. It reads Lutris's
library, `pga.db`, without starting Lutris. Its keys work like
[`[steam]`](#steam)'s:

```toml
[lutris]
path = "~/.local/share/lutris"          # optional, the folder with pga.db, see below
template = "gs"                         # optional, or a list, like on a [[game]]
exec = "lutris lutris:rungameid/{id}"   # optional, this is the default
tags = ["lutris"]                       # optional
workdir = "{directory}"                 # optional, this is the default
games.the-witcher-3 = { tags = ["rpg"], template = "coop" }   # optional, one game's settings
games.epic-games-store = { hidden = true }
```

- Without `path`, Lutris is looked for in `~/.local/share/lutris`, then the
  flatpak's `~/.var/app/net.lutris.Lutris/data/lutris`. Lutris only runs on
  Linux, so on Windows `[lutris]` only warns.
- Each game gets `{id}`, `{slug}`, `{runner}`, `{platform}` and `{directory}`
  (the folder it's installed in) as variables. The name is Lutris's.
- The default `exec` is `lutris lutris:rungameid/{id}`, and
  `flatpak run net.lutris.Lutris lutris:rungameid/{id}` for the flatpak.
  Playtime is off by default, like for Steam: when Lutris is already running,
  the command returns right away. `env` only reaches the `lutris` command; use
  the game's settings in Lutris for the game's own.
- `games.<slug>` sets things for one game, by its Lutris slug (the last part of
  its lutris.net URL). It takes the same keys as
  `[steam]`'s `games.<appid>`. Lutris's own id changes on a reinstall, so it
  isn't used here; two installs of the same game share a slug and both get
  the settings. A slug that isn't installed is a warning.
- Lutris's cover for a game (`coverart/<slug>.jpg`) is its default cover, under
  a `cover` from the config. `cover = ""` drops it, so SteamGridDB's is used.
- A Steam game Lutris imported gets its `steam_appid`, so art is found by app
  id. If `[steam]` imports it too, both are kept with a warning, like for a
  `[[game]]`; hide one with `games.<slug> = { hidden = true }` or `[steam]`'s
  `games.<appid>`.
- Lutris's categories aren't read, so a game hidden in Lutris still shows;
  hide it here with `games.<slug>`.
- `pga.db` is read on start and on F5. Lutris not being installed or a `pga.db`
  that can't be read is a warning, and the rest of the library still loads.

### Genres

With [IGDB credentials](#igdb-credentials) set, `g:` filters by the genres IGDB
lists for a game, and works together with `t:` and the name:

| Search             | Shows                                                         |
| ------------------ | ------------------------------------------------------------- |
| `g:strategy`       | `Strategy`, `Real Time Strategy (RTS)`, `Turn-based strategy (TBS)` games |
| `g:rts`            | `Real Time Strategy (RTS)` games                              |
| `g:shooter t:coop` | `coop` games that are shooters                                |

- Genre matching is case-insensitive and matches anywhere in a genre's name,
  since IGDB's genres have spaces in them and the search splits on whitespace.
  A bare `g:` doesn't filter anything yet.
- A game shows up once its genres are known: games not looked up yet (the first
  start after adding them) and games IGDB doesn't know never match a `g:`.
- Wrong or missing genres usually mean IGDB matched the wrong game by name. Set
  the game's [`igdb_id`](#igdb-credentials).

### Status bar

The bar under the grid is a list of parts on the `left` and the `right`. Each
part is a text with variables, and `bold = true` makes it stand out. This is
the default:

```toml
[status]
separator = " · "   # goes between the parts that are shown
left = [
  { text = "{title}", bold = true },
  { text = "{playtime} · last played {last_played}" },
  { text = "{not_installed}" },
  { text = "{tags}" },
]
right = [{ text = "{shown} games" }]
```

| Variable          | Is                                                              |
| ----------------- | --------------------------------------------------------------- |
| `{title}`         | the name as shown on the card, `{year}` included                |
| `{name}`          | the name without `{year}` (what playtime and art go by)         |
| `{year}`          | the release year from [IGDB](#igdb-credentials)                 |
| `{genres}`        | the genres from IGDB, like `Real Time Strategy (RTS), Strategy` |
| `{playtime}`      | the total time played, like `42h 10m`                           |
| `{last_played}`   | when, like `3 days ago`                                         |
| `{not_installed}` | `not installed` if the game's `workdir` doesn't exist           |
| `{tags}`          | the game's tags, like `coop, fps`                               |
| `{shown}`         | how many games match the search                                 |
| `{total}`         | how many games there are                                        |
| `{env:NAME}`      | environment variable `NAME`, as the selected game would get it  |

- **Empty parts disappear**, separator and all: a part whose variables are all
  blank isn't shown, so a game that was never played has no stray
  `· last played`. Empty `()` or `[]` are dropped too, like in names, so
  `"{name} ({year})"` shows just the name until the year is known.
- **Your own variables** from `[vars]` and a game's `vars` work too, and win over
  the ones above. A part that uses a variable only some games define is left out
  for the others.
- **Leaving out** `left` or `right` keeps its default; `right = []` empties it.
- **Mistakes are errors**, like in the rest of the config: an unknown variable
  or a stray brace is shown in the red line, and `--check` reports it.
- `{year}` and `{genres}` need [IGDB credentials](#igdb-credentials).
- `{env:NAME}` is the game's own `env`, then [`[env]`](#environment), then
  rsgl's own environment; blank if none of them set it. It only works in the
  bars: commands run through the shell, so use `$NAME` there.
- When no game matches the search, the left side just says `No games`.

### Top bar

The bar above the grid holds widgets in three zones: `left`, `center` and
`right`. The default is just the search bar in the center:

```toml
[topbar]
left = [
  { type = "label", text = "MangoHud {env:MANGOHUD}", bold = true },
  { type = "button", title = "Steam", exec = "steam -bigpicture" },
  { type = "filter", options = [
      { title = "All", filter = "" },
      { title = "PC Games", filter = "t:pc" },
      { title = "LAN", filter = "t:pc t:lan" },
      { title = "Need for Speed", filter = "Need for Speed" },
  ] },
]
center = [
  { type = "search", placeholder = "Find something to play…", autocomplete = ["names", "tags", "genres"] },
]
right = [
  { type = "label", text = "{shown}/{total}" },
  { type = "sort", options = ["name", "last_played", "playtime", "year"], default = "last_played" },
  { type = "toggle", title = "MangoHud", env = "MANGOHUD", on = "1", off = "0" },
  { type = "menu", title = "Tools", entries = [
      { title = "Lutris", exec = "lutris" },
      { title = "Pavucontrol", exec = "pavucontrol", env = { PULSE_LATENCY_MSEC = "60" } },
  ] },
]
```

| Widget   | Keys                                                                      |
| -------- | ------------------------------------------------------------------------- |
| `label`  | `text` and `bold`, like a [status bar](#status-bar) part, same variables  |
| `search` | `placeholder`, shown while it's empty, instead of the key hint, and `autocomplete` |
| `button` | `title`, and `exec`, `workdir` and `env` like a menu entry                |
| `menu`   | `title`, and `entries`: a list of `title`, `exec`, `workdir` and `env`    |
| `toggle` | `title`, and `env` or `var`, a variable it sets to `on` or `off`         |
| `sort`   | `options`, orders for the grid to pick from, and the `default` one        |
| `filter` | `options`: a list of `title` and `filter`, a search; and the `default` title |

- The center zone stays centered. The left and right zones get the room beside
  it, and their text is cut off rather than running into it; with something
  beside it, the center takes at most half the width. The widgets nearest the
  window's edge are cut off last.
- A label is left out when its variables are all blank, like a status part.
- There's at most one search bar. **Leaving out** `center` puts it there,
  unless it's in `left` or `right`; `center = []` empties it. Without a search
  bar there's nothing to type into, but `--search` still filters.
- **Autocomplete** is off unless you list what it completes: `names`, `tags`
  (after `t:`) and `genres` (after `g:`, needs [IGDB credentials](#igdb-credentials)).
  While the cursor is at the end of the search box, the rest of the word you're
  typing shows grayed out after it, and Tab adds it. A name completes all of the
  card's title; a genre one word at a time, since the search splits on
  whitespace (`g:rt` gives `g:rtS`, for `(RTS)`). It's the first match in
  alphabetical order, and only from the games the rest of the search shows, so
  taking it never leaves the grid empty. Binding Tab in
  [`[keyboard]`](#key-bindings) takes it away from autocomplete. Without
  `placeholder`, the search bar's hint shows your bound `launch` and `reload`
  keys.
- An unknown widget `type` or key is an error.
- **Buttons and menu entries** run like a [right-click](#right-click-menu)
  tool: in the background, and rsgl stays open. No game is involved, so they
  get `[vars]` and [`[env]`](#environment) but not a game's own `vars` or
  `env`, and `workdir` defaults to none. A variable only some games define is
  an error, also when `[env]` uses it; give it a default in `[vars]`.
- **Toggles** replace the variable's [`[env]`](#environment) value for
  everything launched from then on: games, right-click entries, buttons and
  menus. A game's or entry's own `env` still wins. Without `off`, switching it
  off unsets the variable. A leading `~` in `on` and `off` is expanded, like in
  `[env]`. Labels with `{env:NAME}` follow it.
- **A toggle with `var`** switches one of the [`[vars]`](#variables) instead,
  for everything that uses it: games, right-click entries, buttons, menus and
  labels. A game's own `vars` still win. `on` and `off` are used as they are;
  to build on them, use the variable in another `[vars]` value. Without `off`,
  switching it off makes the variable blank. It has to be in `[vars]`, and a
  game's `name` can't use it. For example, MangoHud inside gamescope:

  ```toml
  [vars]
  mango = ""
  gs = "gamescope{mango} -w {w} -h {h} -f --"

  [topbar]
  right = [{ type = "toggle", title = "MangoHud", var = "mango", on = " --mangoapp" }]
  ```
- A toggle starts on when the variable would be `on` without it (its `[env]`
  value, else rsgl's own environment; for `var`, its `[vars]` value), every
  time rsgl starts or reloads; it isn't remembered, also not by
  `reopen_after_game`. There's one toggle per variable.
- **The sort** orders the grid, and shows the order it's on: `name`, `last_played` and `playtime` (most
  first; without [playtime](#playtime) on, they're by name), or `year` (IGDB's,
  newest first). Games without one go last, in name order. `options` defaults
  to all four and `default` to the first option. It starts from `default`
  every time rsgl starts, and there's at most one. Picking an order selects
  its first game; when IGDB data reorders the grid, the selection stays on its
  game.
- **The filter** narrows the grid to one of its searches, shown as
  `< PC Games >`. A `filter` is anything you could type in the search bar
  (`t:` tags, `g:` genres, name text), and the grid shows the games that match
  both it and what's typed, so you can pick `< NES >` and type `mario`. The
  search bar itself isn't touched, and Esc doesn't clear the filter.
  `filter = ""` shows every game, so put an "All" option first if you want
  one; there's no hidden one. The arrows (or `prev_filter`/`next_filter`,
  Ctrl+PageUp/PageDown and LB/RB by default) switch it, going around at the
  ends, and a click on the title opens the list. It starts on `default`, else
  the first option, every time rsgl starts (or on the one
  [`--filter`](#command-line) names); there's at most one, and titles
  can't repeat. Switching selects the grid's first game.
  [`--check`](#command-line) warns about a `t:` tag that no game has, which is
  likely a typo.
- The sort and filter stay on what you picked when the config is reloaded,
  as long as the order or filter title is still there.
- Multi-line inline tables, like the `menu` above, need TOML 1.1, which rsgl reads.
- **With the keyboard or a gamepad**, up on the top row of the grid (or with no
  games shown) moves to the bar's first button, menu, toggle, sort or filter. Left/right move
  along them, launch uses one, down or back go back to the grid. Only a fresh
  press of up goes into the bar, so holding it to scroll stops at the top row.
  The search box is skipped; typing still goes to it.
- A button, menu, toggle, sort or filter cut off by a narrow window is skipped too.

### Playtime

When you launch a game, the window closes but rsgl stays running in the
background until the game exits, then adds the session to the game's total.
The selected game's total and when you last played it are shown in the status
bar.

- The background process is a fresh, tiny copy of rsgl (a few MB), not the
  window, so it doesn't keep the launcher's memory around.
- It waits for the game's whole process group, so `start.sh` scripts that start
  the real game and exit right away are measured correctly.
- Running sessions are saved every `checkpoint_minutes`, so a crash or shutdown
  loses at most that much.
- Time while the machine is suspended isn't counted.
- Totals are stored by game name in `~/.local/share/rsgl/playtime.toml`
  (see [Portable setup](#portable-setup) to keep them elsewhere).
  Renaming a game in the config starts it from zero (or rename the entry in
  that file too).

Anything that hands off to another daemon won't be tracked properly:
`steam steam://...`, Lutris, Heroic, some flatpaks. The launch command returns
right away and the game runs outside our process group. Direct executables and
normal wrapper scripts are fine. Set `playtime = false` on those games so they
don't log a few seconds each launch; games from [`[steam]`](#steam) have it off
already.

### Reopening after a game

With `reopen_after_game = true`, the launcher comes back when a game started
from its window exits, for couch or kiosk setups where rsgl is the only thing
on screen. It uses the same background process as playtime tracking (and works
with playtime off), so it has the same limits: a game that hands off to Steam,
Lutris and the like "exits" right away and the launcher reopens immediately.

- It reopens with the same command-line flags it was started with (`-c`, `-f`, `-s`, `--filter`).
- Closing the launcher yourself (Esc, the window's close button) ends the loop.
- Games started with `--launch` don't reopen it.

### Windows

Everything works the same on Windows apart from the points below.

- **Paths in TOML:** use single quotes so backslashes aren't treated as escapes,
  for example `exec = '"C:\Games\My Game\game.exe" -windowed'`. Forward slashes
  in `cover` and `workdir` are fine.
- **`exec`:** it runs as `cmd /S /C "<exec>"`. Quote any path that contains
  spaces. A leading `~` (or `~\`) is expanded to your user folder, but without
  quotes, so use a full quoted path if your user name has a space in it.
- **Console window:** when rsgl is started from a terminal, games share it and
  print there (see [Game output](#game-output)). Otherwise they start with
  their console hidden, so windowed games are unaffected but a terminal
  program will run invisibly.
- **Playtime:** Windows has no process groups, so rsgl follows the one process
  it started (the `cmd` wrapper), which waits for the game to exit. Games that
  start something else and quit right away (launchers, `start`, Steam URLs) are
  not tracked properly, so set `playtime = false` on those. When you launch a
  game, rsgl starts a tiny hidden copy of itself to do the waiting and the
  window process exits.
- **Folders:** the config is in `%APPDATA%\rsgl` (or next to `rsgl.exe`), the cache in
  `%LOCALAPPDATA%\rsgl` and the playtime file in `%APPDATA%\rsgl\playtime.toml`.
  A menu's `open_folder` action opens the game's folder in Explorer.
- **Terminal output:** release builds are GUI programs. Run from `cmd` or
  PowerShell they print `--help`, errors and the game's output to that terminal,
  but the prompt comes back immediately instead of waiting for rsgl to close.

### Single instance

With `single_instance = true` (the default), starting rsgl while its window is
already open does nothing. The lock is released when the window closes, so you
can open the launcher again while a game is running.

### SteamGridDB key

Get an API key from your SteamGridDB account preferences. You can set it in the
config as shown above, or with the `STEAMGRIDDB_API_KEY` environment variable,
which takes priority over the config file unless it's blank. Without a key,
rsgl only uses images that are already on disk.

### IGDB credentials

IGDB fills in [`{year}`](#variables) in game names, `{year}` and `{genres}` in
the [status bar](#status-bar), and the genres that [`g:`](#genres) searches.
With credentials set, every game is looked up by name in the background when the
window opens, and the result is cached in `~/.cache/rsgl/igdb_meta.toml`, so
each game is only looked up once (a game IGDB doesn't know is cached as empty,
except a ROM, which is asked again on the next start). Delete a game's entry there
to look it up again.

The name search looks at IGDB's top 10 results and prefers ones with a release
date: it picks an exact name match with a release date, else the first result
with a release date, and only falls back to undated results when none has a
date. Set `igdb_id` on the game to skip the search. `rsgl --igdb NAME` prints
the raw top 5 search results, with each game's `id` (e.g.
`rsgl --igdb "Portal 2" > portal.json`). Adding, changing or removing an
`igdb_id` looks the game up again on the next start.

To see which IGDB game every game got, run `rsgl --igdb-matches`:

```
=  Quake  ->  Quake (1996), igdb_id 333
?  Quake 4  ->  Quake (1996), igdb_id 333
=  Doom  ->  DOOM (1993), igdb_id 673, pinned
?  Some Mod  ->  nothing on IGDB
```

`=` is an exact match by name or a game with `igdb_id` set; `?` is IGDB's best
guess, worth a look (`rsgl --igdb-matches | grep '^?'`). It searches IGDB again
for every game, the same way the window does, rather than reading the cache.
Lookups are spaced to IGDB's 4 requests a second; if IGDB still answers 429
Too Many Requests, rsgl waits 15 seconds and tries again (up to 4 times).

IGDB signs in through Twitch, so register an application at
[dev.twitch.tv/console](https://dev.twitch.tv/console) (any name, OAuth redirect
`http://localhost`, client type Confidential) and copy its client id and a new
secret into `igdb_client_id` and `igdb_client_secret`. The `IGDB_CLIENT_ID` and
`IGDB_CLIENT_SECRET` environment variables take priority over the config file
unless they're blank. The access token Twitch hands out is cached in
`~/.cache/rsgl/igdb_token.toml` (readable only by you) and reused until it's
within an hour of expiring (roughly 60 days), or renewed if IGDB rejects it or
`igdb_client_id` changes.

## Recipes

Steam-like features you can build from config alone.

### Installing games from a server

Keep your games on a server and install, update, check and uninstall them from
the right-click menu, with a top bar menu to update everything at once:

```toml
[vars]
games = "~/.wine/drive_c/games"
remote = "user@192.168.0.100:/home/user/data/games"
term = "xterm -hold -e"   # or "foot --hold", "kitty --hold", "alacritty --hold -e"
sync = "rsync -avP"

[templates.default]
workdir = "{games}/{path}/"

[topbar]
right = [
  { type = "menu", title = "Sync", entries = [
      { title = "Update installed", exec = '''{term} sh -c 'for d in {games}/*/; do {sync} {remote}/"$(basename "$d")"/ "$d"; done' ''' },
      { title = "Pull everything", exec = "{term} {sync} {remote}/ {games}/" },
  ] },
]

[[templates.default.right_click]]
title = "Install / update"
exec = "{term} {sync} {remote}/{path}/ {games}/{path}/"
workdir = "{games}"

[[templates.default.right_click]]
title = "Verify files"
exec = "{term} rsync -rc --dry-run --itemize-changes {remote}/{path}/ {games}/{path}/"
workdir = "{games}"

[[templates.default.right_click]]
title = "Uninstall"
exec = '''test -n "{path}" && zenity --question --title=Uninstall --text="Delete {name}?" && rm -rf "{games}/{path}"'''
workdir = "{games}"

[[game]]
name = "Command & Conquer 3: Kane's Wrath"
exec = "wine CNC3EP1.exe"
vars = { path = "CommandConquer3KaneWrath" }

[[game]]
name = "Unreal Tournament 2004"
exec = "./UT2004"
workdir = "{games}/{path}/System/"   # its own workdir wins over the template's
vars = { path = "UnrealTournament2004" }
```

**The server needs** an ssh server you can log in to, `rsync` installed (rsync
runs on both ends), and read access to the games folder. Each game is a folder
there named like its `path`. Set up ssh key auth (`ssh-copy-id user@192.168.0.100`):
"Update installed" logs in once per game, and anything run without `{term}`
has no terminal to type a password into (see [Game output](#game-output)).

- **Not installed games are dimmed**, since their `workdir` doesn't exist yet,
  and light up when the install finishes. That's why the menu entries use
  `workdir = "{games}"` instead of the game's own: rsgl won't start anything
  in a folder that doesn't exist. Create `{games}` once before the first install.
- **Every `[[game]]` needs `path`**, because `[templates.default]` and its
  entries apply to every game. [`[[roms]]`](#roms) don't get them, so ROMs
  don't need a `path` and Uninstall can't reach them.
- **Trailing slashes matter:** `rsync -a src/foo/ dst/foo/` copies the contents
  of `foo`; without the slashes you get `dst/foo/foo`.
- **`{term}`** opens a terminal so you can watch the transfer, and its hold
  option keeps it open afterwards to read the result. The option differs per
  terminal: xterm's is `-hold`, most others take `--hold`. To use your
  `$TERMINAL`, write `${{TERMINAL:-xterm}}` (the `{{` is a literal brace, see
  [Variables](#variables)), with a hold option it understands.
- **The `test -n` guard** stops an empty `path` from turning Uninstall into
  `rm -rf ~/.wine/drive_c/games/`. The zenity dialog needs a mouse or keyboard.
- **Verify files** lists the files that differ from the server or are missing
  locally, by checksum. It reads every file on both ends, so it's slow on big
  games. Run Install / update to fix what it finds.
- **Update installed** only updates folders you already have; Pull everything
  installs every game on the server.
- **No `-z` on a LAN:** game files are mostly compressed already, so
  compression costs CPU and gains little. Use `rsync -avzP` over the internet.
- **No `--delete` by default:** it would make an update remove files the server
  doesn't have, but plenty of games keep their config and saves in their own
  folder, and those would go too.

### Syncing saves

Pull a game's saves before it starts and push them after it exits by listing the
steps in `exec`. Because each item runs after the previous one finishes, a
failed pull when the server is unreachable still lets the game start. On top of
the config above:

```toml
[vars]
saves_remote = "user@192.168.0.100:/home/user/data/saves"
sync_saves = "rsync -a --update --ignore-missing-args --mkpath"
saves_dir = "."
saves = ""
pull = 'test -n "{saves}" && {sync_saves} {saves_remote}/{path}/ "{saves_dir}/"'
push = 'test -n "{saves}" && cd "{saves_dir}" && {sync_saves} -R {saves} {saves_remote}/{path}/'

[[game]]
name = "FlatOut 2"
exec = [
  "{pull}",
  "wine FlatOut2.exe",
  "{push}",
]
vars = { path = "FlatOut2", saves = "Savegame" }

[[game]]
name = "Delta Force"
exec = [
  "{pull}",
  "wine Df.exe",
  "{push}",
]
vars = { path = "DeltaForce", saves = "player0*.ply df.cfg" }

[[game]]
name = "Return to Castle Wolfenstein"
exec = [
  "{pull}",
  "./wolfsp.x86",
  "{push}",
]
vars = { path = "RtCW", saves_dir = "$HOME/.wolf", saves = "save main/wolfconfig.cfg" }
```

- This runs without a terminal, so it needs ssh key auth.
- `saves` is a space-separated list of files, folders or globs relative to
  `saves_dir` (which defaults to `.`, the game's `workdir`). Quote a path inside
  `saves` if it has spaces (`saves = "'My Saves'"`).
- `-R` (`--relative`) keeps each path relative to `saves_dir` under
  `{saves_remote}/{path}/`, so `pull` restores them all into `saves_dir` in one
  step. `--ignore-missing-args` skips files or globs that don't exist yet on
  first launch, and `--mkpath` (rsync 3.2.3+) creates `saves_dir` and the remote
  folder if they aren't there yet.
- For saves outside `workdir`, override `saves_dir` on the game. Because
  `"{saves_dir}"` is quoted in the shell to allow spaces, write `$HOME` or a
  full path in `saves_dir` rather than a leading `~`. Each game gets one
  `saves_dir` root; a game that keeps saves both in `workdir` and outside it
  needs a second `pull_home`/`push_home` pair.
- `--update` never overwrites a newer file, so playing on two machines doesn't
  lose the newest save (and there's no `--delete`, so a deleted save comes back
  from the server). It can't merge two sets of changes, though; for that,
  [Syncthing](https://syncthing.net/) on the saves folder is the better tool.
- The sync time counts toward playtime.

## Command line

```
Usage: rsgl [OPTIONS]

Options:
  -c, --config <FILE>  Read this config file instead of the default locations
  -f, --fullscreen     Start fullscreen
  -s, --search <TEXT>  Start with TEXT in the search bar (t:tag, g:genre filter)
      --filter <NAME>  Start on the top bar's filter with this title
      --installed      Only include installed games
  -h, --help           Show this help
  -V, --version        Show the version

Commands that run without opening the window (use one):
      --launch <GAME>  Start GAME
      --random         Start a random game
      --list           Print the names of all games
      --show <GAME>    Print how GAME will run (exec, workdir, env, menu)
      --fetch          Download missing art and metadata into the cache
      --check          Check the whole config, hidden games included
      --igdb <NAME>    Search IGDB for NAME and print the JSON it returns
      --igdb-matches   Print which IGDB game each game's year and genres come from
```

- `--config` replaces the default lookup rather than going first. The file has
  to exist: rsgl exits with an error if it doesn't, and never writes an example
  config to a path you gave it. F5 reloads the same file. `--flag=VALUE` works
  for every long option that takes a value, and a leading `~` in `--config` is
  expanded.
- `--fullscreen` does the same as `fullscreen = true` and wins over `start_maximized`.
- `--installed` narrows the window, `--list`, `--random` and `--fetch` to games
  whose `workdir` exists (or isn't set).
- The eight commands (`--launch`, `--random`, `--list`, `--show`, `--fetch`, `--check`, `--igdb`, `--igdb-matches`) run
  without a window, don't take the single-instance lock, and never write an
  example config: if there's no config they just say so. Use only one at a time, with
  `--config` if you like. They exit with 0 on success, 1 on failure, and 2 on bad
  command-line arguments.
- `--launch GAME` starts a game from a hotkey or a script, just like pressing
  Enter on it. With playtime on, rsgl stays running in the background until the
  game exits, to time it. If the game can't start, it says why and exits 1.
- `--random` starts a random game the same way. It respects `-s`/`--search`,
  `--filter` and `--installed`, for example `rsgl --random --installed -s t:coop`.
- `--list` prints the name of every matching game, one per line, in alphabetical
  order, respecting `-s`/`--search`, `--filter` and `--installed`. For example,
  with rofi:
  `rsgl --list --installed | rofi -dmenu | xargs -d '\n' -r rsgl --launch`.
- `--show GAME` prints the game's resolved `exec`, `workdir`, `env` and menu
  entries, with every variable filled in. For `--launch` and `--show`, GAME can
  be an exact name (any case) or part of a name if only one game contains it.
- `--fetch` runs the Hasheous, IGDB and SteamGridDB fetchers and background
  blur headlessly until they finish, respecting `-s`/`--search`, `--filter` and
  `--installed` if given.
- Hidden games aren't loaded, so `--list`, `--random`, `--show`, `--launch` and
  `--fetch` don't see them. `--check` does look at them.
- `--check` reads the whole config (including `include` files) and reports every problem instead of stopping
  at the first (printing up to 20 errors, then all warnings): an **error** is
  something that stops a game loading (an undefined `{variable}`, a bad menu
  entry, a key under the wrong header), and a **warning** is something that
  loads but looks wrong (a game or menu entry `workdir` that doesn't exist, an
  unknown key, a filter `t:` tag no game has, two games with the same name).
  Only errors make the exit code 1. It checks files and folders on this machine,
  so warnings about a drive that isn't mounted are normal.
- `--search` fills in the search bar and applies it right away, for example
  `rsgl -s "t:coop"`. Esc clears it like any other search. It also filters
  `--list`, `--random` and `--fetch`.
- `--filter NAME` starts on the top bar's [filter](#top-bar) with that title
  instead of its `default`, for example `rsgl --filter NES` for a couch
  shortcut. It finds the title like `--launch` finds a game, so `--filter nes`
  works too. A title that isn't there (or a top bar without a filter) shows as
  an error in the window, which then starts on the default. It goes with
  `--search` (`rsgl --filter NES -s mario`) and also filters `--list`,
  `--random` and `--fetch`.

## Controls

| Key                     | Action                                                                           |
| ----------------------- | -------------------------------------------------------------------------------- |
| Type                    | Filter games by name, `t:tag` by tag, or `g:genre` by genre                      |
| Tab                     | Take the search's [autocomplete](#top-bar) suggestion, if it's on                |
| Arrow keys              | Move the selection                                                               |
| PageUp/Down             | Move the selection a screen up or down                                           |
| Ctrl+PageUp/Down        | Switch the top bar's [filter](#top-bar)                                          |
| Enter / click           | Launch the selected game and close rsgl                                          |
| Mouse over a card       | Select it (only when the mouse moves, so it doesn't fight the keyboard)          |
| Mouse wheel             | Scroll the grid                                                                  |
| Right-click / Shift+F10 | Open the game's [menu](#right-click-menu), or its `workdir` if it has no entries |
| F5                      | Reload the config                                                                |
| Esc                     | Clear the search, or quit if it's empty (closes an open menu, or leaves the top bar, first) |

These are the defaults; see [Key bindings](#key-bindings) to change them. Mouse
buttons can't be rebound. A gamepad works too, see [Gamepad](#gamepad).

### Key bindings

`[keyboard]` binds each action to a list of keys. An action you leave out keeps
its default, and an empty list unbinds it:

```toml
[keyboard]
launch = ["Enter"]
menu = ["Shift+F10", "Ctrl+M"]
reload = []               # F5 does nothing now
```

| Action      | Default     | Does                                                         |
| ----------- | ----------- | ------------------------------------------------------------ |
| `up`        | `Up`        | Move up a row, from the top row into the [top bar](#top-bar) (up an entry in the menu) |
| `down`      | `Down`      | Move down a row, or out of the top bar (down an entry in the menu) |
| `left`      | `Left`      | Move left                                                    |
| `right`     | `Right`     | Move right                                                   |
| `page_up`   | `PageUp`    | Move a screen up                                             |
| `page_down` | `PageDown`  | Move a screen down                                           |
| `launch`    | `Enter`     | Launch the selected game, run the highlighted menu entry, or use the top bar's button, menu, toggle, sort or filter |
| `menu`      | `Shift+F10` | Open the selected game's right-click menu                    |
| `back`      | `Escape`    | Close the menu, else leave the top bar, else clear the search, else quit |
| `reload`    | `F5`        | Reload the config                                            |
| `prev_filter` | `Ctrl+PageUp` | Switch the top bar's [filter](#top-bar) to the previous one |
| `next_filter` | `Ctrl+PageDown` | Switch it to the next one                                |

- Keys and modifiers are case-insensitive and use egui's names: `Enter`,
  `Escape` (or `Esc`), `Tab`, `Home`, `End`, `PageUp`, `F1`-`F35`,
  `Up`/`Down`/`Left`/`Right` (or `ArrowUp` etc.), letters, digits, and
  punctuation either by symbol (`/`) or name (`Slash`).
- Modifiers go in front with `+`: `Ctrl`, `Alt` and `Shift`, like
  `Ctrl+Alt+M` or `Ctrl++`. They must match exactly, so `Enter` doesn't fire on
  `Alt+Enter`.
- Letters, digits, punctuation, `Space`, `Backspace` and `Delete` type into or
  edit the search box, so they need `Ctrl` or `Alt` (`Ctrl+L` is fine, `L` or
  `Shift+L` isn't).
- An unknown action, an unknown key, or a key bound to two actions is an
  error, shown like any other config error. Rebinding `back` to `Enter` means
  moving `launch` off it too.

### Gamepad

Any number of gamepads work alongside the keyboard, and can be plugged in while
rsgl is open. `[gamepad]` binds the same actions to buttons, with the same
rules as `[keyboard]`:

```toml
[gamepad]
menu = ["Menu", "Y"]
reload = []
```

| Action      | Default                       |
| ----------- | ----------------------------- |
| `up`        | `DPadUp`, `LeftStickUp`       |
| `down`      | `DPadDown`, `LeftStickDown`   |
| `left`      | `DPadLeft`, `LeftStickLeft`   |
| `right`     | `DPadRight`, `LeftStickRight` |
| `page_up`   | `LT`                          |
| `page_down` | `RT`                          |
| `launch`    | `A`                           |
| `menu`      | `Menu`                        |
| `back`      | `B`                           |
| `reload`    | `View`                        |
| `prev_filter` | `LB`                        |
| `next_filter` | `RB`                        |

- Buttons are named after the Xbox layout (in any case) and matched by
  position, so `A` is the bottom face button on any pad (Cross on a PlayStation
  pad): `A`, `B`, `X`, `Y`, `LB`, `RB`, `LT`, `RT`, `View`, `Menu`, `Guide`,
  `LS`, `RS` (stick clicks), `DPadUp`/`Down`/`Left`/`Right`, and
  `LeftStickUp`/`Down`/`Left`/`Right` for tilting the left stick.
- Holding a button bound to a move action (`up`, `down`, `left`, `right`,
  `page_up`, `page_down`) repeats it; other actions don't, whichever button
  they're on.
- rsgl ignores the gamepad while its window isn't focused, so a tool started
  from the menu can be used with the pad without rsgl reacting too.

## Cache

Downloaded and generated files live under `~/.cache/rsgl` (`$XDG_CACHE_HOME/rsgl`
if set, `%LOCALAPPDATA%\rsgl` on Windows). Playtime likewise follows
`$XDG_DATA_HOME`, and the single-instance lock is `$XDG_RUNTIME_DIR/rsgl.lock`
(or in the cache folder when `$XDG_RUNTIME_DIR` isn't set). The cache holds:

- `covers/` — cover art, named `<slug>.png|jpg|webp`
- `heroes/` — wide hero images used for backgrounds
- `backgrounds/` — blurred backgrounds, regenerated automatically when `blur` changes
- `igdb_token.toml` — the IGDB access token, readable only by you; delete it to force a new one
- `igdb_meta.toml` — release years and genres from IGDB, by game name; delete a game's entry to look it up again
- `hasheous.toml` — what [Hasheous](#hasheous) identified each ROM as, by file path; delete an entry to look it up again

The slug is the game name lowercased, with every non-alphanumeric character
replaced by `_` (so "Hollow Knight: Silksong" becomes `hollow_knight__silksong`).
Each `<slug>.id` file in `covers/` and `heroes/` records what the cached image
was downloaded with (`steam_appid`, `steamgriddb_id`, `grid_id`/`hero_id`, or
the IGDB name a ROM was searched by), so changing one of those fetches it again.
You can drop your own images into `covers/` or `heroes/` with that name and rsgl
will use them instead of downloading. Deleting a file makes rsgl fetch it again
on the next start.

### Portable setup

`[paths]` moves the cache and the playtime file. A relative path is taken from
the config file's folder, not the folder rsgl was started from, so the program,
its config and everything it saves can share one folder (a USB stick, say):

```toml
[paths]
cache = "./cache"             # default ~/.cache/rsgl
playtime = "./playtime.toml"  # default ~/.local/share/rsgl/playtime.toml
```

Name the config `rsgl.toml` and put it next to the executable, and rsgl picks
it up on its own, so double-clicking `rsgl.exe` is enough. A config anywhere
else works with `rsgl -c path/to/rsgl.toml`. `~` works too. A change takes
effect on F5 (except the playtime totals shown in the open window, which are
loaded at start), and nothing is moved over from the old place.
