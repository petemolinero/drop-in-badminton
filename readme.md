# Drop-in Badminton

Court rotation board for pickup badminton nights, written in Rust (egui/eframe).
Built for a laptop mirrored to a big TV: courts big at the top, the "up next"
groups underneath, and the waiting queue on the right.

## Run

```sh
cargo run --release
```

Everything is saved automatically to a SQLite database in a
`Drop-in Badminton data` folder next to the app, so copying the app's folder to
another computer takes everything with it. (When that folder can't be written,
or when running from cargo's `target` folder, it's saved to
`~/.local/share/drop-in-badminton/` instead; on Windows,
`%APPDATA%\drop-in-badminton\drop-in-badminton\data\`. Data from earlier
versions is moved from there next to the app on first start, and the old folder
is renamed `... (moved next to the app)`.)

- **Roster**: everyone who has ever come. It carries over from week to week.
  Removing someone archives them, so their past games stay in History.
- **History**: each game day, who was there (check-in/out times), and every
  game played (court, round, start/end time, teams).
- **Live session**: courts, Up next, and the timer, so a restart picks up where you left off.

Press **New game day** at the bottom of the roster to close out today and start
fresh: everyone starts checked out, and you toggle players in as they arrive.
If the app has been idle for 8 hours, it starts a new game day on its own.
The **History** button (clock icon) shows past game days, attendance, and games.

The tabs in the title bar switch between four views:

- **Dashboard**: the courts, Up next, the timer and the waiting list.
- **Players**: the player database: everyone, archived players included, as a
  table, separate from any session. Edit names, skill levels, where they're from
  and how they found us in place; click the status to archive a player or make
  them active again. Turn on **Auto check-in** for regulars: they're checked in
  at the start of every new session. Columns sort when clicked, and show
  sessions, games, games per session, court time, usual partner, and first and
  latest session. **Export CSV** saves the rows shown as a spreadsheet.
- **Games**: every game ever played, one row each: date, start time, length,
  round, court, and both teams. It's read-only until **Unlock editing** (after a
  warning that it rewrites history); then every cell can be corrected and games
  deleted. Edits to past sessions can't be undone. **Export CSV** here too.
- **Analytics**: totals (players, sessions, games, average turnout, how many came
  back, court time), turnout per session split into returning players and
  first-timers, the ten players with the most games, and the roster's skill mix.

An older `state.json` is imported automatically on first run and renamed to
`state.json.imported`.

## Password

**Settings → Security → Set a password…** protects the app on a shared computer.
From then on the app opens on a sign-in screen, and its data (players, sessions,
games, and the backups) is only ever written to disk encrypted: Argon2id
stretches the password into a key, and XChaCha20-Poly1305 seals the database
(`drop-in-badminton.db.enc` in the data folder). The readable database files are
removed when a password is set.

There's **no recovery**: a forgotten password means the data can't be opened by
anyone. *Forgot the password?* on the sign-in screen can start over with an empty
app, keeping the locked data in `backups/` in case the password turns up.
**Change…** and **Remove…** need the current password.

## Playing in groups

- **Plays together** (right-click a player → *Plays together with…*, or the
  Players view's *Plays with* column): link 2–4 players, like a parent and
  their kids. Whenever they're here and free at the same time they go on the
  same court (teams are still balanced by skill), sharing one place in line. The link lasts across sessions
  until you unlink them; anyone not here or on a break doesn't hold the others up.
  Linked players show a link icon in the lists.
- **Practice group** (the *Practice* button, or right-click → *Join the
  practice group*): for a night when some players are preparing for a
  tournament. They only play each other, but the courts are shared fairly: each
  Up next group goes to whichever side has played less tonight (games per hour,
  per person), so everyone gets about the same time on court. Their places in
  the waiting list, and their courts and Up next cards, are marked in orange.
  It lasts for the session.

## How a night works

1. Open the **roster** (people icon). On a new week, press **New game day** first if it didn't start on its own. Toggle players in as they arrive. New
   players go in the sidebar's **Add player** tab: name, skill level, where they're
   from, how they found us (one tap for the usual answers), and whether to check
   them in automatically at every new session.
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
- Up next always takes the **next players in line**: 4 for one open court, 8 for
  two, and so on. Skill never lets anyone jump the queue; it only arranges the
  chosen players: similar levels share a court (Beginner = yellow, Intermediate =
  red, Advanced = green, Expert = blue), and teams are balanced: strongest +
  weakest vs. the middle two.
- Linked players share a place in line and a wait (bracketed together in the
  waiting list). If the cut-off would split them, they wait for the next round
  and the next player in line goes instead.
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
desktop shortcut. The app keeps its data in a `Drop-in Badminton data` folder beside
it: to move to another computer, copy the whole `Drop-in Badminton` folder. Windows will warn once that the app is unrecognised (it isn't
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
