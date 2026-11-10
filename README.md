# rsgl

A small game launcher for Linux and Windows that you can drive with a keyboard,
a gamepad or a mouse. You list your games in a TOML file, and rsgl shows them as
a grid of cover art. Type to filter, pick one with the arrow keys, the D-pad or
the mouse, launch it, and the launcher starts the game and closes.

If you give it a [SteamGridDB](https://www.steamgriddb.com/) API key, it
downloads missing covers and hero images for you, then draws a blurred version
of the selected game's art behind the grid. With [IGDB](https://www.igdb.com/)
credentials, it also fills in release years and genres, which you can show in
titles and the status bar and search with `g:`.


![preview](assets/preview.gif)

## Features

- Cover-art grid with search-as-you-type filtering
- Variables (`{name}`) and a global `[env]`, so paths, launch commands and
  environment settings aren't repeated in the config
- Command-line `--launch`, `--list`, `--show`, `--check` and `--igdb`, for hotkeys and scripts
- Tags, with `t:tag` filtering in the search bar, and `g:genre` with IGDB genres
- Configurable right-click menu per game: tools, or alternate ways to launch
- Full keyboard and gamepad control with rebindable keys and buttons, mouse works too
- Automatic cover and hero downloads from SteamGridDB (optional)
- Blurred, crossfading background for the selected game
- Per-game working directory and environment variables
- Games are started detached, so they keep running after the launcher closes
- Playtime tracking per game, shown in the status bar
- A status bar you can rearrange with templates, including release year and
  genres from IGDB (optional)
- Single instance mode, so a second launcher window can't be opened
- Command-line flags for fullscreen, an alternate config file and a starting search
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
X11. On Windows it builds with the normal MSVC toolchain (`x86_64-pc-windows-msvc`);
see [Windows](#windows) for the differences.

## Configuration

rsgl reads the first of these files that exists:

1. `$XDG_CONFIG_HOME/rsgl/rsgl.toml` (usually `~/.config/rsgl/rsgl.toml`;
   on Windows `%APPDATA%\rsgl\rsgl.toml`)
2. `~/.rsgl.toml`

Pass `-c <file>` to use a different file instead (see [Command line](#command-line)).

If neither exists, the first run writes an example config to
`~/.config/rsgl/rsgl.toml` and shows a message telling you to edit it and
press F5.

F5 reloads the config without restarting. Games, `card_width`, `blur`, the
`[api]` keys, `[keyboard]`, `[gamepad]`, `[status]` and `[topbar]` apply right away; `single_instance`,
`start_maximized`, `fullscreen`, `reopen_after_game` and `[playtime]` need a
restart. If the file has an error, the current list stays up and the
error is shown above it.

```toml
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

[env]
WINEDEBUG = "-all"       # set for every game; a game's own env wins

[[game]]
name = "Example Game"
exec = "~/games/example/start.sh"
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
  pipes all work.
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
  sometimes picks the wrong game. Set `steam_appid` or `steamgriddb_id` (the
  number in the game's SteamGridDB URL) to skip the search. If both are set,
  `steamgriddb_id` wins. Adding or changing an id re-downloads the cached cover
  and hero once, replacing what's in the cache.
- To pick the exact image instead of the best-scored one, set `grid_id` (cover)
  or `hero_id` (background) to the number in the image's SteamGridDB URL, e.g.
  `steamgriddb.com/grid/664940`. Each works on its own and wins over the game
  ids above for that image; the other one is still found as usual. Changing
  one re-downloads just that image. Only png, jpeg and webp images work; an
  animated one is reported in the terminal and skipped. A `cover = ...` file
  that exists still wins over `grid_id`.
- `igdb_id` does the same for IGDB: the year and genres come from that game
  instead of a search by name. See [IGDB credentials](#igdb-credentials).
- `hidden = true` hides a game without deleting its entry.
- A game whose `workdir` doesn't exist is drawn dimmed and marked "not
  installed" in the status bar. A game without a `workdir` is never dimmed.
  The grid updates when a right-click tool exits, so an install or uninstall
  entry shows up right away.
- `{name}` placeholders in `name`, `exec`, `workdir`, `cover` and `env` are filled in from
  `[vars]`, see [Variables](#variables).
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
| `launch`  | Optional, default `false`. See below.                                            |

- **Tools** (`launch = false`) run in the background and rsgl stays open. They
  don't count toward playtime.
- **Alternate launches** (`launch = true`) behave like pressing Enter on the
  game: rsgl closes and the playtime is tracked for that game (unless playtime
  is off, globally or for the game).
- Entries run exactly like a game's `exec`: through the shell, with `~`
  expanded, and printing to your terminal if you started rsgl from one.
- A top-level `[[right_click]]` applies to every game and comes first, then the
  game's own entries. A game with no entries at all just opens its `workdir`
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
  every game must define it.
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
- **Same workdir pattern for every game:** put it in `[defaults]` once, and
  each game only needs its folder name:

  ```toml
  [defaults]
  workdir = "{games}/{path}/"

  [[game]]
  name = "FlatOut 2"
  exec = "{gs} wine FlatOut2.exe"
  vars = { path = "FlatOut2" }

  [[game]]
  name = "Unreal Tournament 2004"
  exec = "./UT2004"
  workdir = "{games}/UnrealTournament2004/System/"   # its own workdir wins
  ```

  The default is filled in with each game's variables, so every game that
  uses it must define `{path}`; the error says the workdir came from
  `[defaults]`. `workdir = ""` gives a game no workdir at all. `workdir` is the
  only key `[defaults]` has; anything else is an error. Top bar buttons and
  menus don't use it.

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
- Anything in the search that isn't a `t:` word is matched against the game name
  as before. Extra spaces are ignored.
- Tags can't contain spaces, since the search splits on whitespace. Use
  something like `local-mp` or `local_mp`.
- The selected game's tags are shown in the status bar.

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
  The rest of the word you're typing shows grayed out after it, and Tab adds it.
  A name completes all of the game's name; a genre one word at a time, since
  the search splits on whitespace (`g:rt` gives `g:rtS`, for `(RTS)`). It's the
  first match in alphabetical order, and only from the games the rest of the
  search shows, so taking it never leaves the grid empty. Binding Tab in
  [`[keyboard]`](#key-bindings) takes it away from autocomplete.
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
  every time, like a toggle, and there's at most one. Picking an order selects
  its first game; when IGDB data reorders the grid, the selection stays on its
  game.
- Multi-line inline tables, like the `menu` above, need TOML 1.1, which rsgl reads.
- **With the keyboard or a gamepad**, up on the top row of the grid (or with no
  games shown) moves to the bar's first button, menu, toggle or sort. Left/right move
  along them, launch uses one, down or back go back to the grid. Only a fresh
  press of up goes into the bar, so holding it to scroll stops at the top row.
  The search box is skipped; typing still goes to it.
- A button, menu, toggle or sort cut off by a narrow window is skipped too.

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
- Totals are stored by game name in `~/.local/share/rsgl/playtime.toml`.
  Renaming a game in the config starts it from zero (or rename the entry in
  that file too).

Anything that hands off to another daemon won't be tracked properly:
`steam steam://...`, Lutris, Heroic, some flatpaks. The launch command returns
right away and the game runs outside our process group. Direct executables and
normal wrapper scripts are fine. Set `playtime = false` on those games so they
don't log a few seconds each launch.

### Reopening after a game

With `reopen_after_game = true`, the launcher comes back when a game started
from its window exits, for couch or kiosk setups where rsgl is the only thing
on screen. It uses the same background process as playtime tracking (and works
with playtime off), so it has the same limits: a game that hands off to Steam,
Lutris and the like "exits" right away and the launcher reopens immediately.

- It reopens with the same command-line flags it was started with (`-c`, `-f`, `-s`).
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
- **Folders:** the config is in `%APPDATA%\rsgl`, the cache in
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
which takes priority over the config file. Without a key, rsgl only uses
images that are already on disk.

### IGDB credentials

IGDB fills in [`{year}`](#variables) in game names, `{year}` and `{genres}` in
the [status bar](#status-bar), and the genres that [`g:`](#genres) searches.
With credentials set, every game is looked up by name in the background when the
window opens, and the result is cached in `~/.cache/rsgl/igdb_meta.toml`, so
each game is only looked up once (a game IGDB doesn't know is cached as empty).
Delete a game's entry there to look it up again.

The name search picks the result with the exact same name, else the first one
with a release date, which is sometimes the wrong game. Set `igdb_id` on the
game to skip the search. `rsgl --igdb NAME` prints the raw search results, with
each game's `id` (e.g. `rsgl --igdb "Portal 2" > portal.json`). Adding, changing
or removing an `igdb_id` looks the game up again on the next start.

IGDB signs in through Twitch, so register an application at
[dev.twitch.tv/console](https://dev.twitch.tv/console) (any name, OAuth redirect
`http://localhost`, client type Confidential) and copy its client id and a new
secret into `igdb_client_id` and `igdb_client_secret`. The `IGDB_CLIENT_ID` and
`IGDB_CLIENT_SECRET` environment variables take priority over the config file.
The access token Twitch hands out is cached in `~/.cache/rsgl/igdb_token.toml`
and reused until it's about to expire (roughly 60 days).

## Command line

```
Usage: rsgl [OPTIONS]

Options:
  -c, --config <FILE>  Read this config file instead of the default locations
  -f, --fullscreen     Start fullscreen
  -s, --search <TEXT>  Start with TEXT in the search bar (t:tag, g:genre filter)
  -h, --help           Show this help

Commands that run without opening the window (use one):
      --launch <GAME>  Start GAME
      --list           Print the names of all games
      --show <GAME>    Print how GAME will run (exec, workdir, env, menu)
      --check          Check the whole config, hidden games included
      --igdb <NAME>    Search IGDB for NAME and print the JSON it returns
```

- `--config` replaces the default lookup rather than going first. The file has
  to exist: rsgl exits with an error if it doesn't, and never writes an example
  config to a path you gave it. F5 reloads the same file. `--config=FILE` works
  too, and a leading `~` is expanded.
- `--fullscreen` does the same as `fullscreen = true` and wins over `start_maximized`.
- The five commands (`--launch`, `--list`, `--show`, `--check`, `--igdb`) run
  without a window, don't take the single-instance lock, and never write an
  example config: if there's no config they just say so. Use only one at a time, with
  `--config` if you like. They exit with 0 on success and 1 on failure.
- `--launch GAME` starts a game from a hotkey or a script, just like pressing
  Enter on it. With playtime on, rsgl stays running in the background until the
  game exits, to time it. If the game can't start, it says why and exits 1.
- `--list` prints the name of every game, one per line, in the order the grid
  shows them. For example, with rofi:
  `rsgl --list | rofi -dmenu | xargs -d '\n' -r rsgl --launch`.
- `--show GAME` prints the game's resolved `exec`, `workdir`, `env` and menu
  entries, with every variable filled in. For `--launch` and `--show`, GAME can
  be an exact name (any case) or part of a name if only one game contains it.
- Hidden games aren't loaded, so `--list`, `--show` and `--launch` don't see them.
  `--check` does look at them.
- `--check` reads the whole config and reports every problem instead of stopping
  at the first: an **error** is something that stops a game loading (an undefined
  `{variable}`, a bad menu entry, a key under the wrong header), and a
  **warning** is something that loads but looks wrong (a `workdir` that doesn't
  exist, two games with the same name, which share a playtime total). Only
  errors make the exit code 1. It checks files and folders on this machine, so
  warnings about a drive that isn't mounted are normal.
- `--search` fills in the search bar and applies it right away, for example
  `rsgl -s "t:coop"`. Esc clears it like any other search.

## Controls

| Key                     | Action                                                                           |
| ----------------------- | -------------------------------------------------------------------------------- |
| Type                    | Filter games by name, `t:tag` by tag, or `g:genre` by genre                      |
| Tab                     | Take the search's [autocomplete](#top-bar) suggestion, if it's on                |
| Arrow keys              | Move the selection                                                               |
| PageUp/Down             | Move the selection a screen up or down                                           |
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
| `launch`    | `Enter`     | Launch the selected game, run the highlighted menu entry, or use the top bar's button, menu, toggle or sort |
| `menu`      | `Shift+F10` | Open the selected game's right-click menu                    |
| `back`      | `Escape`    | Close the menu, else leave the top bar, else clear the search, else quit |
| `reload`    | `F5`        | Reload the config                                            |

- Keys use egui's names: `Enter`, `Escape` (or `Esc`), `Tab`, `Home`, `End`,
  `PageUp`, `F1`-`F35`, `Up`/`Down`/`Left`/`Right`, letters, digits, and
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

Any gamepad works alongside the keyboard, and can be plugged in while rsgl is
open. `[gamepad]` binds the same actions to buttons, with the same rules as
`[keyboard]`:

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
| `page_up`   | `LB`                          |
| `page_down` | `RB`                          |
| `launch`    | `A`                           |
| `menu`      | `Menu`                        |
| `back`      | `B`                           |
| `reload`    | `View`                        |

- Buttons are named after the Xbox layout and matched by position, so `A` is
  the bottom face button on any pad (Cross on a PlayStation pad): `A`, `B`,
  `X`, `Y`, `LB`, `RB`, `LT`, `RT`, `View`, `Menu`, `Guide`, `LS`, `RS` (stick
  clicks), `DPadUp`/`Down`/`Left`/`Right`, and
  `LeftStickUp`/`Down`/`Left`/`Right` for tilting the left stick.
- Holding a direction or `LB`/`RB` repeats it; other buttons don't.
- rsgl ignores the gamepad while its window isn't focused, so a tool started
  from the menu can be used with the pad without rsgl reacting too.

## Cache

Downloaded and generated files live under `~/.cache/rsgl` (`%LOCALAPPDATA%\rsgl` on Windows):

- `covers/` — cover art, named `<slug>.png|jpg|webp`
- `heroes/` — wide hero images used for backgrounds
- `backgrounds/` — blurred backgrounds, regenerated automatically when `blur` changes
- `igdb_token.toml` — the IGDB access token, readable only by you; delete it to force a new one
- `igdb_meta.toml` — release years and genres from IGDB, by game name; delete a game's entry to look it up again

The slug is the game name lowercased, with every non-alphanumeric character
replaced by `_` (so "Hollow Knight: Silksong" becomes `hollow_knight__silksong`).
Each `<slug>.id` file in `covers/` records which `steam_appid`/`steamgriddb_id`
the cached art was downloaded with. You can drop your own images into `covers/` or `heroes/` with that name and rsgl
will use them instead of downloading. Deleting a file makes rsgl fetch it again
on the next start.

Older versions saved images to `~/.local/share/game-covers` and
`~/.local/share/game-heroes`. Those folders are still checked, so existing
downloads keep working.
