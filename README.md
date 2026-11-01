# rsgl

A small, keyboard-driven game launcher for Linux. You list your games in a TOML
file, and rsgl shows them as a grid of cover art. Type to filter, pick one with
the arrow keys, hit Enter, and the launcher starts the game and closes.

If you give it a [SteamGridDB](https://www.steamgriddb.com/) API key, it
downloads missing covers and hero images for you, then draws a blurred version
of the selected game's art behind the grid.

## Features

- Cover-art grid with search-as-you-type filtering
- Full keyboard control, mouse works too
- Automatic cover and hero downloads from SteamGridDB (optional)
- Blurred, crossfading background for the selected game
- Per-game working directory and environment variables
- Games are started detached, so they keep running after the launcher closes
- Playtime tracking per game, shown in the status bar
- Single instance mode, so a second launcher window can't be opened
- Games without a cover get a colored placeholder with their initial

## Building

You need a recent stable Rust toolchain.

```sh
cargo build --release
./target/release/rsgl
```

rsgl uses `eframe`/`egui` with OpenGL and supports both Wayland and X11. It
only builds on Unix, since it detaches games with Unix process groups.

## Configuration

rsgl reads the first of these files that exists:

1. `$XDG_CONFIG_HOME/rsgl/rsgl.toml` (usually `~/.config/rsgl/rsgl.toml`)
2. `~/.rsgl.toml`

If neither exists, the first run writes an example config to
`~/.config/rsgl/rsgl.toml` and shows a message telling you to edit it and
press F5.

F5 reloads the config without restarting. Games, `card_width`, `blur` and the
API key apply right away; `single_instance`, `start_maximized` and `[playtime]`
need a restart. If the file has an error, the current list stays up and the
error is shown above it.

```toml
[ui]
card_width = 170   # card width in pixels, clamped to 80-400
blur = 8           # background blur strength, 0 turns blur off (max 50)
single_instance = true   # refuse to start while another launcher window is open
start_maximized = false  # open the window maximized

[playtime]
enabled = true           # false turns tracking and the status bar display off
checkpoint_minutes = 5   # save a running session every N minutes, 0 = only on exit

[api]
steamgriddb_key = "your-key-here"

[[game]]
name = "Example Game"
exec = "~/games/example/start.sh"
cover = "~/.cache/rsgl/covers/example.png"   # optional
workdir = "~/games/example"                  # optional
env = { SOME_VAR = "value" }                 # optional
playtime = false                             # optional, don't track this game
hidden = true                                # optional, keep the entry but leave it out of the grid
steam_appid = 367520                         # optional, find art by Steam app id
steamgriddb_id = 1234                        # optional, find art by SteamGridDB game id
```

Notes on `[[game]]` entries:

- `exec` runs through `sh -c`, so arguments, quoting and pipes all work.
- A leading `~` is expanded in `exec`, `cover`, `workdir` and `env` values.
- Games are sorted by name (case-insensitive), not by their order in the file.
- If `cover` is missing or doesn't exist, rsgl looks in the covers cache (see
  below) for a file named after the game.
- Covers are normally found by searching SteamGridDB for the game's name, which
  sometimes picks the wrong game. Set `steam_appid` or `steamgriddb_id` (the
  number in the game's SteamGridDB URL) to skip the search. If both are set,
  `steamgriddb_id` wins. Adding or changing an id re-downloads the cached cover
  and hero once, replacing what's in the cache.
- `hidden = true` hides a game without deleting its entry.

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

### Single instance

With `single_instance = true` (the default), starting rsgl while its window is
already open does nothing. The lock is released when the window closes, so you
can open the launcher again while a game is running.

### SteamGridDB key

Get an API key from your SteamGridDB account preferences. You can set it in the
config as shown above, or with the `STEAMGRIDDB_API_KEY` environment variable,
which takes priority over the config file. Without a key, rsgl only uses
images that are already on disk.

## Controls

| Key           | Action                                   |
| ------------- | ---------------------------------------- |
| Type          | Filter games by name                     |
| Arrow keys    | Move the selection                       |
| PageUp/Down   | Move the selection a screen up or down   |
| Enter / click | Launch the selected game and close rsgl  |
| Right-click   | Menu with "Open game folder" (`workdir`) |
| F5            | Reload the config                        |
| Esc           | Clear the search, or quit if it's empty  |

## Cache

Downloaded and generated images live under `~/.cache/rsgl`:

- `covers/` — cover art, named `<slug>.png|jpg|webp`
- `heroes/` — wide hero images used for backgrounds
- `backgrounds/` — blurred backgrounds, regenerated automatically when `blur` changes

The slug is the game name lowercased, with every non-alphanumeric character
replaced by `_` (so "Hollow Knight: Silksong" becomes `hollow_knight__silksong`).
Each `<slug>.id` file in `covers/` records which `steam_appid`/`steamgriddb_id`
the cached art was downloaded with. You can drop your own images into `covers/` or `heroes/` with that name and rsgl
will use them instead of downloading. Deleting a file makes rsgl fetch it again
on the next start.

Older versions saved images to `~/.local/share/game-covers` and
`~/.local/share/game-heroes`. Those folders are still checked, so existing
downloads keep working.
