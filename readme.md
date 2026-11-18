# Drop-in Badminton

Court rotation board for pickup badminton nights, written in Rust (egui/eframe).
Built for a laptop mirrored to a big TV: courts big at the top, the "up next"
groups underneath, and the waiting queue on the right.

## Run

```sh
cargo run --release
```

Everything is saved automatically to a SQLite database at
`~/.local/share/drop-in-badminton/drop-in-badminton.db` (on Windows,
`%APPDATA%\drop-in-badminton\drop-in-badminton\data\drop-in-badminton.db`):

- **Roster**: everyone who has ever come. It carries over from week to week.
  Removing someone archives them, so their past games stay in History.
- **History**: each game day, who was there (check-in/out times), and every
  game played (court, round, start/end time, teams).
- **Live session**: courts, Up next, and the timer, so a restart picks up where you left off.

Press **New game day** at the bottom of the roster to close out today and start
fresh: everyone starts checked out, and you toggle players in as they arrive.
If the app has been idle for 8 hours, it starts a new game day on its own.
The **History** button (clock icon) shows past game days, attendance, and games.

The tabs in the title bar switch between three views:

- **Dashboard**: the courts, Up next, the timer and the waiting list.
- **Players**: the player database: everyone, archived players included, as a
  table, separate from any session. Edit names and skill levels in place; click
  the status to archive a player or make them active again. Columns sort
  when clicked, and show sessions, games, games per session, court time, usual
  partner, and first and latest session.
- **Analytics**: totals (players, sessions, games, average turnout, how many came
  back, court time), turnout per session split into returning players and
  first-timers, the ten players with the most games, and the roster's skill mix.

An older `state.json` is imported automatically on first run and renamed to
`state.json.imported`.

## How a night works

1. Open the **roster** (people icon). On a new week, press **New game day** first if it didn't start on its own. Toggle players in as they arrive, or add new
   ones with a name and skill level.
2. The **Up next** cards fill automatically from the waiting pool.
3. Press **Next round** (or `N`): each Up next group goes onto its court, and the
   cards refill from the pool.
4. Press **play** on the timer (or `Space`). When it hits zero the timer and the
   Next round button pulse. Press Next round again, and repeat.

Drag any player chip to rearrange: between courts and Up next slots (dropping on
someone swaps you), from the waiting list or roster into a slot, or back onto
the waiting list to send them back. Anyone you pull off Up next by hand sits out
until the next round, so they don't get placed straight back.

## Rotation rules

- The waiting pool is ordered by wall-clock time since each player last came off
  a court (or checked in). Waits within the same minute are broken by fewest games tonight.
- Each Up next group starts with the longest-waiting player and is completed with the
  closest skill matches (Beginner = yellow, Intermediate = red, Advanced = green, Expert = blue).
- Anyone past the **maximum wait** (default 30 min) goes in next regardless of skill.
- Teams are balanced: strongest + weakest vs. the middle two.
- With too few players to fill every court, courts take turns: the one that has
  been playing longest gets the next group.

## Shortcuts

`Space` start/pause timer · `N` next round (both on the Dashboard) · `F1` help · `F11` full screen · `Ctrl+Z` undo · `Esc` close panels

**Right-click any player** — on a court, up next, waiting, or in the roster — to send them to waiting, give them a break, check them out, change their skill level, rename them, or delete them (with a confirmation).

**Help** (top left of the ribbon, or `F1`) adds a short explanation to the top of every section — courts, Up next, waiting list, roster, settings and history.

## Installing and updating

Download `drop-in-badminton-windows-x86_64.exe` from the
[latest release](https://github.com/petemolinero/drop-in-badminton/releases/latest),
put it somewhere permanent (e.g. a `Drop-in Badminton` folder in Documents) and make a
desktop shortcut. Windows will warn once that the app is unrecognised (it isn't
code-signed): click **More info → Run anyway**.

After that, updates install from inside the app. It checks GitHub on start-up
(and every few hours); when a newer release exists, an **Update** button appears
in the ribbon. Clicking it shows what's new, downloads the new version, backs up
the database to `backups/` next to it, and restarts. **Settings** shows the
current version and has a **Check for updates** button.

### Publishing an update

```sh
tools/release.sh 0.2.0 "Courts can be renamed" "Fixed the timer sound"
```

This bumps the version in `Cargo.toml`, runs the tests, commits, tags and
pushes. GitHub Actions (`.github/workflows/release.yml`) builds Windows and
Linux binaries and publishes the release; the notes become "What's new" in the
app's update dialog. Each copy of the app picks it up the next time it checks.

Updates are off in debug builds and when `DROP_IN_BADMINTON_NO_UPDATE` is set.

## Development

`cargo test` runs the rotation engine and storage tests.

## Look and feel

The UI follows Fluent 2, the design language of Microsoft 365: a title bar and
command ribbon over floating cards on a neutral canvas, in light or dark.

- **Font:** Segoe UI is used when it's installed (it can't be bundled); otherwise the embedded Roboto.
- **Icons:** a small subset of [Fluent System Icons](https://github.com/microsoft/fluentui-system-icons)
  (MIT) in `assets/fonts/FluentIcons-DropIn.ttf`. To add an icon, edit and rerun
  `tools/build_icons.py` (needs `fonttools`), then copy the printed constants into `src/theme.rs`.
  Then run `tools/shuttle_glyph.py assets/fonts/FluentIcons-DropIn.ttf`: Fluent has no
  shuttlecock, so it draws one over the `SHUTTLE`/`SHUTTLE_F` slots.
- **Windows graphics:** some PCs have graphics drivers that crash the moment they're
  loaded (an old Intel OpenGL driver did). So on Windows the .exe starts the app as a
  child process with one graphics setup at a time (Direct3D 12, Direct3D 12 in
  software, Vulkan, OpenGL) until one opens a window, and remembers it in
  `graphics.txt` in the data folder (`src/launch.rs`). Fatal errors are written to
  `crash.log` there too, and shown in a message box. Every release is started on a
  clean Windows machine first (`tools/smoke.ps1`); the "Windows smoke test" workflow
  also fakes a driver crash to check the fallback.
- **App icon:** a Liquid Glass shuttlecock on a blue tile. `tools/glass_icon.py` draws it
  as `assets/icon/app.svg` and renders the 1024 px master `app-1024.png` with headless
  Chrome; `tools/build_app_icon.py` then makes the .exe icon (`app.ico`) and the window
  icon (`app.png`, `app.rgba`) from the master. `build.rs` embeds the .ico in the Windows .exe.
