//! End-to-end check of the built BTC Ticker. Usage: `btc-ticker-e2e <btc-ticker.exe> <screenshot folder> [pid file]`.
//!
//! It covers the primary monitor with a stand-in game window (this executable copied to `Wow.exe`), launches the app
//! over it with throwaway settings, and drives it with real mouse input through `SendInput`. It clicks only its own
//! windows and the app's, puts the cursor back, and prints one PASS, FAIL or INCONCLUSIVE line per check.

mod desktop;
mod standin;
mod ticker;
mod verdict;

use std::cell::OnceCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde_json::{Value, json};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::WindowsAndMessaging::{WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST};

use desktop::{Mouse, Rect};
use standin::StandIn;
use ticker::{Snapshot, Ticker, wait_until};
use verdict::Verdicts;

const KRAKEN_TICKER: &str = "https://api.kraken.com/0/public/Ticker?pair=XBTUSD";
const MENU_CLASS: &str = "BTCTickerMenu";
const SETTINGS_CLASS: &str = "BTCTickerSettings";
const MENU_ROWS: f64 = 3.0;
const WHEEL_DELTA: i32 = 120;
/// `MOUSEWHEEL_ROUTING_FOCUS`: the wheel goes to the focused window, as with "Scroll inactive windows" off.
const FOCUS_ROUTING: u32 = 0;
/// Kraken answers with 721 candles, the newest still forming, and the app keeps 720.
const HELD_CANDLES: usize = 720;
/// The bar budget the app promises: one bar per 2 DIPs of chart width past a 60 DIP price axis.
const PRICE_AXIS_DIPS: f64 = 60.0;
const DIPS_PER_BAR: f64 = 2.0;

fn main() {
    desktop::per_monitor_dpi();
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("standin") {
        standin::run(&args[2..]);
        return;
    }
    let (Some(app), Some(shots)) = (args.get(1), args.get(2)) else {
        println!("usage: btc-ticker-e2e <btc-ticker.exe> <screenshot folder> [pid file]");
        std::process::exit(2);
    };
    let mut report = Report::default();
    run(&mut report, Path::new(app), Path::new(shots), args.get(3).map(PathBuf::from));
    std::process::exit(if report.verdicts.failures() == 0 { 0 } else { 1 });
}

pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

#[derive(Default)]
struct Report {
    verdicts: Verdicts,
    /// The first input the check saw but did not send. The mouse sets it, and every verdict after it goes through it.
    disturbance: Rc<OnceCell<String>>,
}

impl Report {
    fn line(&mut self, status: &str, name: &str, detail: impl AsRef<str>) {
        let disturbance = self.disturbance.get().map(String::as_str);
        println!("{}", self.verdicts.check(status, name, detail.as_ref(), disturbance));
    }

    fn check(&mut self, passed: bool, name: &str, detail: impl AsRef<str>) {
        self.line(if passed { "PASS" } else { "FAIL" }, name, detail);
    }

    fn teardown(&mut self, passed: bool, name: &str, detail: impl AsRef<str>) {
        println!("{}", self.verdicts.teardown(if passed { "PASS" } else { "FAIL" }, name, detail.as_ref()));
    }
}

/// Everything one run needs; dropping it closes the stand-in and the app and puts the cursor back.
struct Session {
    ticker: Ticker,
    game: StandIn,
    mouse: Mouse,
    /// For each gesture: its name, and whether the stand-in game was foreground before and 0.3 s after it.
    gestures: Vec<(String, bool, bool)>,
    /// Gestures whose expected effect showed up in the snapshot.
    landed: usize,
}

impl Session {
    fn overlay(&self) -> HWND {
        self.ticker.snapshot().map_or_else(HWND::default, |snapshot| desktop::hwnd(snapshot.hwnd))
    }

    fn allowed(&self) -> Vec<HWND> {
        let mut allowed = vec![self.overlay(), self.game.hwnd];
        allowed.extend(desktop::windows_of(self.ticker.pid, MENU_CLASS));
        allowed
    }

    /// Runs a gesture, waits up to `seconds` for a snapshot `wanted` accepts, and records the foreground around it.
    fn gesture(
        &mut self,
        name: &str,
        seconds: f64,
        act: impl FnOnce(&Mouse, &[HWND]) -> Result<(), String>,
        wanted: impl Fn(&Snapshot) -> bool,
    ) -> Result<Snapshot, String> {
        let before = desktop::foreground() == self.game.hwnd;
        let acted = act(&self.mouse, &self.allowed());
        let result = acted.and_then(|()| {
            self.ticker.wait_for(seconds, wanted).ok_or_else(|| describe(self.ticker.snapshot().as_ref()))
        });
        std::thread::sleep(std::time::Duration::from_millis(300));
        self.gestures.push((name.to_string(), before, desktop::foreground() == self.game.hwnd));
        if result.is_ok() {
            self.landed += 1;
        }
        result
    }

    fn click(&mut self, target: &str, seconds: f64, wanted: impl Fn(&Snapshot) -> bool) -> Result<Snapshot, String> {
        let point = self.target(target)?;
        self.gesture(&format!("click {target}"), seconds, |mouse, allowed| mouse.click(point, false, allowed), wanted)
    }

    fn target(&self, target: &str) -> Result<POINT, String> {
        let snapshot = self.ticker.snapshot().ok_or("no snapshot")?;
        snapshot.targets.get(target).map(Rect::center).ok_or_else(|| format!("no '{target}' target in the snapshot"))
    }

    /// Right-clicks the ticker and returns the menu window that opens.
    fn open_menu(&mut self) -> Result<HWND, String> {
        let point = self.target("ticker")?;
        let pid = self.ticker.pid;
        let before = desktop::foreground() == self.game.hwnd;
        self.mouse.click(point, true, &self.allowed())?;
        let menu = wait_until(3.0, || desktop::windows_of(pid, MENU_CLASS).first().copied());
        self.gestures.push(("right-click ticker".to_string(), before, desktop::foreground() == self.game.hwnd));
        self.landed += usize::from(menu.is_some());
        menu.ok_or_else(|| "no menu window appeared within 3 s".to_string())
    }

    fn menu_row(menu: HWND, row: f64) -> POINT {
        let frame = desktop::window_rect(menu);
        let height = (frame.height - 2.0) / MENU_ROWS;
        POINT { x: (frame.x + frame.width / 2.0) as i32, y: (frame.y + 1.0 + height * (row + 0.5)) as i32 }
    }
}

fn run(report: &mut Report, app: &Path, shots: &Path, pid_file: Option<PathBuf>) {
    let work = std::env::temp_dir().join(format!("btcticker-e2e-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&work);
    let user_settings = std::env::var("APPDATA").map(|appdata| PathBuf::from(appdata).join(r"BTCTicker\settings.json"));
    let user_settings_before = user_settings.as_ref().ok().map(std::fs::read);

    let tag = 0x4254_4345 ^ std::process::id() as usize;
    fresh_install(report, app, &work.join("fresh-install"), tag, pid_file.as_deref());

    let (monitor, work_area) = desktop::primary_monitor();
    let game_exe = work.join("Wow.exe");
    let this = std::env::current_exe().unwrap_or_default();
    if let Err(error) = std::fs::copy(&this, &game_exe) {
        report.line("FAIL", "launch", format!("could not copy the stand-in to {}: {error}", game_exe.display()));
        return;
    }
    let game = match StandIn::spawn(&game_exe, &monitor, "BTC Ticker end-to-end check: stand-in for World of Warcraft")
    {
        Ok(game) => game,
        Err(error) => return report.line("FAIL", "launch", error),
    };
    if !wait_until(3.0, || (desktop::foreground() == game.hwnd).then_some(())).is_some()
        && !desktop::bring_to_front(game.hwnd)
    {
        report.line("INCONCLUSIVE", "launch", "the stand-in game window could not take the foreground");
        return;
    }

    let seed = settings(json!({ "locked": false, "refreshSeconds": 300 }));
    let ticker = match Ticker::launch(app, &work, Some(&seed), tag, Rc::clone(&report.disturbance)) {
        Ok(ticker) => ticker,
        Err(error) => return report.line("FAIL", "launch", format!("could not start {}: {error}", app.display())),
    };
    if let Some(pid_file) = &pid_file {
        let _ = std::fs::write(pid_file, ticker.pid.to_string());
    }
    let mouse = Mouse::new(tag, Rc::clone(&report.disturbance));
    let mut session = Session { ticker, game, mouse, gestures: Vec::new(), landed: 0 };
    let launched_in_front = wait_until(1.0, || session.ticker.snapshot()).is_some();
    session.gestures.push(("launch".to_string(), true, desktop::foreground() == session.game.hwnd));
    if !launched_in_front && session.ticker.wait_for(10.0, |_| true).is_none() {
        report.line("FAIL", "launch", "the app wrote no state file within 10 s");
        return;
    }

    feed(report, &session);
    window(report, &session, &monitor);
    topmost_rival(report, &session);
    settings_file(report, &session);
    only_when_wow(report, &session, &this, &work_area);
    refused_focus_grab(report, &session, &this, &work_area);
    clamped(report, &session, &work_area);
    chart(report, &mut session, shots, &work_area);
    opacity_zero(report, &mut session);
    dragging(report, &mut session);
    chart_manipulation(report, &mut session, &work_area, shots);
    menu(report, &mut session);
    focus(report, &session);
    settings_window(report, &mut session, shots);
    second_launch(report, &session, &work);
    quit(report, &mut session);

    let killed = if session.ticker.stop() {
        format!("killed pid {}, which had not quit; ", session.ticker.pid)
    } else {
        String::new()
    };
    let leftovers = desktop::processes_running(&session.ticker.exe);
    report.teardown(
        leftovers.is_empty(),
        "no-leftovers",
        if leftovers.is_empty() {
            format!("{killed}no process runs {}", session.ticker.exe.display())
        } else {
            format!("{killed}still running {}: pids {leftovers:?}", session.ticker.exe.display())
        },
    );
    let untouched =
        user_settings.as_ref().ok().map(std::fs::read).map(|now| now.ok()) == user_settings_before.map(Result::ok);
    report.teardown(
        untouched,
        "user-settings-untouched",
        format!(
            "{} is byte for byte what it was before the run",
            user_settings.map_or_else(|_| "%APPDATA%".into(), |path| path.display().to_string())
        ),
    );
    drop(session);
    let _ = std::fs::remove_dir_all(&work);
}

/// The test's settings: defaults, then `overrides`. The app fills in every key a file leaves out.
fn settings(overrides: Value) -> Value {
    let mut settings = json!({
        "showLabel": true, "showChange": true, "decimals": 0, "fontSize": 15,
        "locked": true, "onlyWhenWoWActive": false,
    });
    if let (Some(base), Some(extra)) = (settings.as_object_mut(), overrides.as_object()) {
        base.extend(extra.clone());
    }
    settings
}

fn feed(report: &mut Report, session: &Session) {
    let live =
        session.ticker.wait_for(20.0, |snapshot| snapshot.feed == "live" && is_ticker_line(&snapshot.text, true, 0));
    let Some(live) = live else {
        let last = session.ticker.snapshot();
        report.line("FAIL", "live-quote", format!("no live quote within 20 s; last: {}", describe(last.as_ref())));
        report.line("FAIL", "refresh-restarts", "no live quote to start from");
        return;
    };
    let shown = shown_price(&live.text).map_or(0.0, |(price, _)| price);
    match kraken_price() {
        Some(reference) if (shown - reference).abs() / reference > 0.02 => {
            report.line("FAIL", "live-quote", format!("shows {shown} but Kraken says {reference}"));
        }
        reference => report.check(
            (1_000.0..10_000_000.0).contains(&shown),
            "live-quote",
            format!(
                "'{}', {}",
                live.text,
                reference.map_or("Kraken cross-check unavailable".into(), |price| format!("Kraken now {price}"))
            ),
        ),
    }

    let first = live.fetched_at.unwrap_or_default();
    session.ticker.write_settings(&settings(json!({ "locked": false, "refreshSeconds": 2 })));
    let edited = now();
    match session.ticker.wait_for(4.0, |snapshot| snapshot.fetched_at.unwrap_or_default() > first) {
        Some(again) => report.line(
            "PASS",
            "refresh-restarts",
            format!(
                "refresh 300 s -> 2 s in the settings file: a new quote landed {:.1} s after the edit",
                again.fetched_at.unwrap_or_default() - edited
            ),
        ),
        None => report.line("FAIL", "refresh-restarts", "no new quote within 4 s of setting refresh to 2 s"),
    }
}

fn window(report: &mut Report, session: &Session, monitor: &Rect) {
    let Some(snapshot) = session.ticker.snapshot() else { return };
    let overlay = desktop::hwnd(snapshot.hwnd);
    let style = desktop::ex_style(overlay);
    let wanted = (WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW).0;
    let rect = desktop::window_rect(overlay);
    report.check(
        desktop::owner_pid(overlay) == session.ticker.pid
            && desktop::is_visible(overlay)
            && style & wanted == wanted
            && rect.same(&snapshot.frame),
        "window-on-screen",
        format!(
            "window {:?} of pid {} at {rect}, snapshot {}, exstyle 0x{style:08x} (layered, topmost, noactivate, toolwindow wanted)",
            overlay.0, session.ticker.pid, snapshot.frame
        ),
    );

    let order = desktop::z_order();
    let position = |window: HWND| order.iter().position(|candidate| *candidate == window);
    let game_rect = desktop::window_rect(session.game.hwnd);
    match (position(overlay), position(session.game.hwnd)) {
        (Some(ours), Some(game)) => report.check(
            ours < game && rect.intersects(&game_rect) && desktop::is_topmost(overlay),
            "above-wow",
            format!(
                "overlay z-index {ours} (topmost {}) over the stand-in Wow.exe at z-index {game}, which covers the primary monitor {monitor}",
                desktop::is_topmost(overlay)
            ),
        ),
        found => report.line("FAIL", "above-wow", format!("overlay or stand-in missing from the z-order: {found:?}")),
    }

    let icon = desktop::tray_icon(session.ticker.pid, "BTCTickerMain");
    report.check(
        icon.is_some(),
        "tray-icon",
        icon.map_or("no notification-area icon registered for the app's main window".to_string(), |icon| {
            format!("registered; Windows shows it, or the overflow chevron holding it, at {icon}")
        }),
    );
}

/// Starts the app with no settings and no settings folder, then edits the file it should be watching.
fn fresh_install(report: &mut Report, app: &Path, work: &Path, tag: usize, pid_file: Option<&Path>) {
    let _ = std::fs::create_dir_all(work);
    let ticker = match Ticker::launch(app, work, None, tag, Rc::clone(&report.disturbance)) {
        Ok(ticker) => ticker,
        Err(error) => {
            return report.line("FAIL", "fresh-install", format!("could not start {}: {error}", app.display()));
        }
    };
    if let Some(pid_file) = pid_file {
        let _ = std::fs::write(pid_file, ticker.pid.to_string());
    }
    let started = ticker.wait_for(10.0, |snapshot| !snapshot.locked).is_some();
    ticker.write_settings(&json!({ "locked": true }));
    let picked_up = started && ticker.wait_for(5.0, |snapshot| snapshot.locked).is_some();
    report.check(
        picked_up,
        "fresh-install",
        format!("started with no settings folder {started}; a settings file written afterwards applied {picked_up}"),
    );
}

fn settings_file(report: &mut Report, session: &Session) {
    let before = session.ticker.snapshot();
    session.ticker.write_settings(&settings(json!({ "showLabel": false, "decimals": 2, "fontSize": 30 })));
    let height = before.as_ref().map_or(f64::INFINITY, |snapshot| snapshot.frame.height);
    let edited = session.ticker.wait_for(5.0, |snapshot| {
        snapshot.locked && is_ticker_line(&snapshot.text, false, 2) && snapshot.frame.height > height
    });
    match (edited, before) {
        (Some(edited), Some(before)) => report.line(
            "PASS",
            "settings-file-live",
            format!(
                "edited on disk: '{}' {} px tall -> '{}' {} px tall, locked",
                before.text, before.frame.height, edited.text, edited.frame.height
            ),
        ),
        _ => report.line(
            "FAIL",
            "settings-file-live",
            format!("no change within 5 s; last {}", describe(session.ticker.snapshot().as_ref())),
        ),
    }
}

/// Moves the real foreground between another app and the stand-in game, with onlyWhenWoWActive on.
fn only_when_wow(report: &mut Report, session: &Session, this: &Path, work_area: &Rect) {
    session.ticker.write_settings(&settings(json!({ "onlyWhenWoWActive": true })));
    let overlay = session.overlay();
    let shown = |visible: bool| {
        session.ticker.wait_for(5.0, |snapshot| snapshot.visible == visible && desktop::is_visible(overlay) == visible)
    };
    let other_rect = Rect { x: work_area.x + 60.0, y: work_area.y + 240.0, width: 420.0, height: 240.0 };
    let other = match StandIn::spawn(this, &other_rect, "BTC Ticker end-to-end check: some other app") {
        Ok(other) => other,
        Err(error) => return report.line("INCONCLUSIVE", "only-when-wow", error),
    };
    let other_in_front = desktop::foreground() == other.hwnd || desktop::bring_to_front(other.hwnd);
    let hidden = other_in_front && shown(false).is_some();
    let game_in_front = desktop::bring_to_front(session.game.hwnd);
    let back = game_in_front && shown(true).is_some();
    drop(other);
    let detail = format!(
        "another app foreground {other_in_front}: overlay hidden {hidden}; stand-in Wow.exe foreground again {game_in_front}: overlay shown {back}"
    );
    if !other_in_front || !game_in_front {
        report.line("INCONCLUSIVE", "only-when-wow", format!("Windows refused a foreground switch; {detail}"));
    } else {
        report.check(hidden && back, "only-when-wow", detail);
    }
}

/// An always-on-top window that raises itself sends no foreground event, so only the app's own timer can notice.
fn topmost_rival(report: &mut Report, session: &Session) {
    let (overlay, game) = (session.overlay(), session.game.hwnd);
    let raised = desktop::set_topmost(game, true);
    let buried = desktop::is_above(game, overlay);
    let started = std::time::Instant::now();
    let back = wait_until(3.0, || desktop::is_above(overlay, game).filter(|above| *above));
    let seconds = started.elapsed().as_secs_f64();
    let in_front = desktop::foreground() == game;
    desktop::set_topmost(game, false);
    let back = back.map_or("not within 3 s".to_string(), |_| format!("after {seconds:.1} s"));
    let detail = format!(
        "the stand-in Wow.exe made itself topmost {raised} and was then above the overlay {buried:?}; the overlay back on top {back}; stand-in still foreground {in_front}"
    );
    if !raised || buried != Some(true) {
        report.line("INCONCLUSIVE", "topmost-rival", format!("the rival never showed above the overlay; {detail}"));
    } else {
        report.check(back.starts_with("after") && in_front, "topmost-rival", detail);
    }
}

/// Windows refuses a background app's `SetForegroundWindow` yet still sends a foreground event naming that app's
/// window. With onlyWhenWoWActive on, the ticker must keep showing, because the game never lost the foreground.
fn refused_focus_grab(report: &mut Report, session: &Session, this: &Path, work_area: &Rect) {
    session.ticker.write_settings(&settings(json!({ "onlyWhenWoWActive": true })));
    let (overlay, game) = (session.overlay(), session.game.hwnd);
    let corner = Rect { x: work_area.max_x() - 340.0, y: work_area.max_y() - 200.0, width: 300.0, height: 160.0 };
    let other = match StandIn::spawn(this, &corner, "BTC Ticker end-to-end check: an app grabbing the foreground") {
        Ok(other) => other,
        Err(error) => return report.line("INCONCLUSIVE", "refused-focus-grab", error),
    };
    let spot = POINT { x: (work_area.x + 40.0) as i32, y: (work_area.max_y() - 40.0) as i32 };
    let mut tries = Vec::new();
    let mut refused = false;
    for _ in 0..5 {
        // Clicking the game makes it the process that received the last input, so Windows refuses the other app.
        let ready = session.mouse.click(spot, false, &session.allowed()).is_ok()
            && (desktop::foreground() == game || desktop::bring_to_front(game))
            && session.ticker.wait_for(3.0, |snapshot| snapshot.visible).is_some();
        if !ready {
            tries.push("the stand-in game could not be put in front with the ticker showing".to_string());
            continue;
        }
        let seen = other.foreground_events().len();
        let asked = other.try_take_foreground();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let named = other.foreground_events().get(seen..).is_some_and(|events| events.contains(&other.hwnd));
        let kept = desktop::foreground() == game;
        tries.push(format!("asked {asked}, a foreground event named it {named}, the game kept the foreground {kept}"));
        if asked && named && kept {
            refused = true;
            break;
        }
    }
    let hidden = refused
        && wait_until(1.5, || {
            let shown =
                session.ticker.snapshot().is_some_and(|snapshot| snapshot.visible) && desktop::is_visible(overlay);
            (!shown).then_some(())
        })
        .is_some();
    drop(other);
    let detail = format!("{}; then the ticker hid within 1.5 s {hidden}", tries.join("; "));
    if refused {
        report.check(!hidden, "refused-focus-grab", detail);
    } else {
        report.line(
            "INCONCLUSIVE",
            "refused-focus-grab",
            format!("Windows never refused the grab with an event; {detail}"),
        );
    }
}

fn clamped(report: &mut Report, session: &Session, work_area: &Rect) {
    let corner = json!({ "x": work_area.max_x() - 20.0, "y": work_area.y - 5.0 });
    session.ticker.write_settings(&settings(json!({ "position": corner })));
    let pulled = session.ticker.wait_for(5.0, |snapshot| {
        (snapshot.frame.max_x() - work_area.max_x()).abs() < 1.0 && (snapshot.frame.y - work_area.y).abs() < 1.0
    });
    match pulled {
        Some(snapshot) => {
            let rect = desktop::window_rect(desktop::hwnd(snapshot.hwnd));
            report.check(
                work_area.contains(&rect),
                "clamped",
                format!("a saved spot hanging off the top right of the work area {work_area} put the window at {rect}, flush with its corner"),
            );
        }
        None => report.line(
            "FAIL",
            "clamped",
            format!("not pulled into {work_area}; last {}", describe(session.ticker.snapshot().as_ref())),
        ),
    }
}

fn charted(label: &'static str) -> impl Fn(&Snapshot) -> bool {
    let kept = match label {
        "1m" => 120,
        "15m" => 96,
        _ => 90,
    };
    move |snapshot| {
        snapshot.mode == "expanded"
            && snapshot.timeframe.as_deref() == Some(label)
            && snapshot.chart.as_deref() == Some("loaded")
            && snapshot.candles == kept
    }
}

fn chart(report: &mut Report, session: &mut Session, shots: &Path, work_area: &Rect) {
    session.ticker.write_settings(&settings(json!({ "timeframe": "h4" })));
    let top_center = |frame: &Rect| {
        (frame.x + frame.width / 2.0 - work_area.x - work_area.width / 2.0).abs() <= 1.0
            && (frame.y - work_area.y - 12.0).abs() < 1.0
    };
    let collapsed = session.ticker.wait_for(5.0, |snapshot| {
        snapshot.locked
            && snapshot.visible
            && snapshot.mode == "collapsed"
            && is_ticker_line(&snapshot.text, true, 0)
            && top_center(&snapshot.frame)
    });
    let Some(collapsed) = collapsed else {
        let last = describe(session.ticker.snapshot().as_ref());
        report.line(
            "FAIL",
            "expand",
            format!("no collapsed, locked, live ticker at the top center of {work_area} to click; {last}"),
        );
        return;
    };
    let home = collapsed.ticker();
    let mut samples = vec![home];
    screenshot(report, &collapsed, shots, "collapsed", false);

    match session.click("ticker", 15.0, charted("4h")) {
        Ok(expanded) => {
            let rect = desktop::window_rect(desktop::hwnd(expanded.hwnd));
            report.check(
                rect.same(&expanded.frame) && work_area.contains(&rect),
                "expand",
                format!(
                    "a real click on the locked ticker opened the last chosen 4h with {} candles; window {} -> {rect}",
                    expanded.candles, collapsed.frame
                ),
            );
            samples.push(expanded.ticker());
            screenshot(report, &expanded, shots, "expanded-4h", true);
        }
        Err(error) => report.line("FAIL", "expand", error),
    }
    for (label, file) in [("15m", "expanded-15m"), ("1m", "expanded-1m")] {
        match session.click(label, 15.0, charted(label)) {
            Ok(shown) => {
                report.line(
                    "PASS",
                    &format!("timeframe-{label}"),
                    format!("clicked {label}: {}", describe(Some(&shown))),
                );
                samples.push(shown.ticker());
                screenshot(report, &shown, shots, file, true);
            }
            Err(error) => report.line("FAIL", &format!("timeframe-{label}"), error),
        }
    }
    zoom(report, session, shots);
    match session.click("ticker", 5.0, |snapshot| snapshot.mode == "collapsed") {
        Ok(folded) => {
            samples.push(folded.ticker());
            report.check(
                folded.ticker().same_spot(&home),
                "collapse",
                format!("clicked the ticker: collapsed back to {}", folded.frame),
            );
        }
        Err(error) => report.line("FAIL", "collapse", error),
    }
    let reopened = session.click("ticker", 15.0, charted("1m"));
    let saved = session.ticker.saved_settings().and_then(|saved| saved.get("timeframe").cloned());
    report.check(
        reopened.is_ok() && saved == Some(json!("m1")),
        "reexpand",
        format!("reopened on the last chosen 1m: {}; the settings file holds timeframe {saved:?}", reopened.is_ok()),
    );
    if let Ok(reopened) = &reopened {
        samples.push(reopened.ticker());
    }
    let _ = session.click("ticker", 5.0, |snapshot| snapshot.mode == "collapsed");
    let moved: Vec<String> = samples.iter().filter(|sample| !sample.same_spot(&home)).map(Rect::to_string).collect();
    report.check(
        moved.is_empty(),
        "ticker-anchored",
        if moved.is_empty() {
            format!("the ticker stayed at {home} across {} samples in both modes", samples.len())
        } else {
            format!("the ticker at {home} moved to {}", moved.join(", "))
        },
    );
}

/// Real wheel input over the expanded 1m chart at x1, which it leaves at x1. The stand-in game stays in the foreground
/// and must receive none of the wheel messages.
fn zoom(report: &mut Report, session: &mut Session, shots: &Path) {
    let start = session.ticker.snapshot().filter(|snapshot| charted("1m")(snapshot));
    let Some((start, chart)) = start.and_then(|start| Some((start.clone(), *start.targets.get("chart")?))) else {
        let last = describe(session.ticker.snapshot().as_ref());
        for name in ["zoom-wheel", "zoom-touchpad", "zoom-focus-routing"] {
            report.line("FAIL", name, format!("not tried: no loaded 1m chart with a 'chart' target to scroll; {last}"));
        }
        return;
    };
    let scale = desktop::scale(desktop::hwnd(start.hwnd));
    let budget = (((chart.width / scale - PRICE_AXIS_DIPS) / DIPS_PER_BAR).floor() as usize).max(1);
    let bars = move |visible: usize| visible / visible.div_ceil(budget);
    let shows = move |zoom: &'static str, visible: usize| {
        move |snapshot: &Snapshot| {
            snapshot.zoom.as_deref() == Some(zoom)
                && snapshot.visible_candles == visible
                && snapshot.candles == bars(visible)
                && snapshot.max_bars == budget
        }
    };
    let center = chart.center();
    let saved_zoom = |session: &Session| session.ticker.saved_settings().and_then(|saved| saved.get("zoom").cloned());
    let routing = desktop::WheelRouting::current();

    let game_before = session.game.wheel_messages();
    let notched = session.gesture(
        "wheel 2 notches toward the user",
        5.0,
        |mouse, allowed| mouse.wheel(center, -WHEEL_DELTA, 2, allowed),
        shows("×4", 480),
    );
    let widest = notched.as_ref().map_err(Clone::clone).and_then(|_| {
        session.gesture(
            "wheel 2 more notches toward the user",
            5.0,
            |mouse, allowed| mouse.wheel(center, -WHEEL_DELTA, 2, allowed),
            shows("all", HELD_CANDLES),
        )
    });
    let saved = saved_zoom(session);
    let game_got = session.game.wheel_messages() - game_before;
    report.check(
        widest.is_ok() && saved == Some(json!(5)) && game_got == 0,
        "zoom-wheel (SendInput MOUSEEVENTF_WHEEL, real input)",
        match (&notched, &widest) {
            (Ok(notched), Ok(widest)) => format!(
                "wheel routing {routing:?}; two notches over the {scale}x-scaled 1m chart stepped two rungs to x4 ({} candles in {} bars), two more reached and held all: {} candles in {} bars for a {budget}-bar budget; the settings file holds zoom {saved:?}; the stand-in game got {game_got} wheel messages",
                notched.visible_candles, notched.candles, widest.visible_candles, widest.candles
            ),
            (Err(error), _) | (_, Err(error)) => {
                format!("wheel routing {routing:?}; {error}; budget {budget}, saved zoom {saved:?}, game got {game_got}")
            }
        },
    );
    if let Ok(widest) = &widest {
        screenshot(report, widest, shots, "expanded-1m-all", true);
    }

    let game_before = session.game.wheel_messages();
    let standard = widest.as_ref().map_err(|_| "not tried: the wheel never reached all".to_string()).and_then(|_| {
        session.gesture(
            "6 touchpad-sized scrolls away from the user",
            5.0,
            |mouse, allowed| mouse.wheel(center, WHEEL_DELTA / 2, 6, allowed),
            shows("×1", 120),
        )
    });
    let saved = saved_zoom(session);
    let game_got = session.game.wheel_messages() - game_before;
    report.check(
        standard.is_ok() && saved == Some(json!(2)) && game_got == 0,
        "zoom-touchpad (SendInput half-notch wheel deltas, not a touchpad)",
        match &standard {
            Ok(standard) => format!(
                "six deltas of {} added up to three rungs, back from all to x1 with {} bars; the settings file holds zoom {saved:?}; the stand-in game got {game_got} wheel messages",
                WHEEL_DELTA / 2,
                standard.candles
            ),
            Err(error) => format!("{error}; saved zoom {saved:?}, game got {game_got}"),
        },
    );

    let Some(focus) = desktop::WheelRouting::set(FOCUS_ROUTING) else {
        return report.line(
            "INCONCLUSIVE",
            "zoom-focus-routing",
            format!("Windows would not route the wheel to the focused window for this session (routing {routing:?})"),
        );
    };
    let game_before = session.game.wheel_messages();
    let out = session.gesture(
        "wheel 1 notch with the wheel routed to focus",
        5.0,
        |mouse, allowed| mouse.wheel(center, -WHEEL_DELTA, 1, allowed),
        shows("×2", 240),
    );
    let game_got = session.game.wheel_messages() - game_before;
    drop(focus);
    let back = session.gesture(
        "wheel 1 notch back",
        5.0,
        |mouse, allowed| mouse.wheel(center, WHEEL_DELTA, 1, allowed),
        shows("×1", 120),
    );
    report.check(
        out.is_ok() && back.is_ok() && game_got == 0,
        "zoom-focus-routing",
        format!(
            "with \"Scroll inactive windows\" off for this session, a notch over the chart zoomed to x2 {} and the focused stand-in game got {game_got} wheel messages; routing restored to {:?}, a notch back reached x1 {}",
            out.is_ok(),
            desktop::WheelRouting::current(),
            back.is_ok()
        ),
    );

    let half = session.gesture(
        "half a notch away from the user",
        0.5,
        |mouse, allowed| mouse.wheel(center, WHEEL_DELTA / 2, 1, allowed),
        |_| true,
    );
    let closed = half.and_then(|_| session.click("ticker", 5.0, |snapshot| snapshot.mode == "collapsed"));
    let reopened = closed.and_then(|_| session.click("ticker", 5.0, shows("×1", 120)));
    let second_half = reopened.and_then(|_| {
        session.gesture(
            "another half notch away from the user",
            0.5,
            |mouse, allowed| mouse.wheel(center, WHEEL_DELTA / 2, 1, allowed),
            |_| true,
        )
    });
    let zoomed = session.ticker.wait_for(1.5, |snapshot| snapshot.zoom.as_deref() != Some("×1"));
    report.check(
        second_half.is_ok() && zoomed.is_none(),
        "zoom-travel-reset",
        match (&second_half, &zoomed) {
            (Err(error), _) => error.clone(),
            (Ok(_), Some(zoomed)) => {
                format!("half a notch, a collapse and reopen, then half a notch zoomed to {:?}", zoomed.zoom)
            }
            (Ok(_), None) => "half a notch, a collapse and reopen, then half a notch: still x1".to_string(),
        },
    );
}

fn screenshot(report: &mut Report, snapshot: &Snapshot, shots: &Path, name: &str, candles: bool) {
    std::thread::sleep(std::time::Duration::from_millis(400));
    let margin = 12.0;
    let region = Rect {
        x: snapshot.frame.x - margin,
        y: snapshot.frame.y - margin,
        width: snapshot.frame.width + 2.0 * margin,
        height: snapshot.frame.height + 2.0 * margin,
    };
    let path = shots.join(format!("{name}.png"));
    match desktop::capture(&region, &path) {
        Ok(picture) if candles => report.check(
            picture.distinct_colors >= 16 && picture.up >= 50 && picture.down >= 50,
            &format!("capture-{}", name.trim_start_matches("expanded-")),
            format!(
                "{}: {} colors, {} up-colored and {} down-colored pixels",
                path.display(),
                picture.distinct_colors,
                picture.up,
                picture.down
            ),
        ),
        Ok(_) => {}
        Err(error) => report.line("FAIL", &format!("capture-{name}"), format!("{}: {error}", path.display())),
    }
}

/// At background opacity 0 the overlay is invisible, yet it must still take clicks rather than pass them to the game.
fn opacity_zero(report: &mut Report, session: &mut Session) {
    session.ticker.write_settings(&settings(json!({ "opacity": 0 })));
    let Some(clear) = session.ticker.wait_for(5.0, |snapshot| snapshot.opacity == 0.0 && snapshot.mode == "collapsed")
    else {
        return report.line("FAIL", "opacity-zero", "the opacity 0 edit did not land");
    };
    let collapsed_misses = misses(&clear);
    let expanded = session.click("ticker", 15.0, |snapshot| snapshot.mode == "expanded" && snapshot.opacity == 0.0);
    let expanded_misses = expanded.as_ref().map(misses).unwrap_or_default();
    report.check(
        expanded.is_ok() && collapsed_misses.is_empty() && expanded_misses.is_empty(),
        "opacity-zero",
        match &expanded {
            Ok(expanded) => format!(
                "a real click on the invisible ticker opened the chart; WindowFromPoint finds the overlay at all 35 grid points of the collapsed {} and the expanded {} panel (misses {collapsed_misses:?} {expanded_misses:?})",
                clear.frame, expanded.frame
            ),
            Err(error) => format!("the click at opacity 0 did not expand the overlay: {error}"),
        },
    );
    let _ = session.click("ticker", 5.0, |snapshot| snapshot.mode == "collapsed");
}

/// Grid points inside the overlay's frame where some other window would take the click.
fn misses(snapshot: &Snapshot) -> Vec<(i32, i32)> {
    let overlay = desktop::hwnd(snapshot.hwnd);
    let frame = snapshot.frame;
    (0..=6)
        .flat_map(|column| (0..=4).map(move |row| (column, row)))
        .map(|(column, row)| POINT {
            x: (frame.x + 1.0 + (frame.width - 3.0) * f64::from(column) / 6.0) as i32,
            y: (frame.y + 1.0 + (frame.height - 3.0) * f64::from(row) / 4.0) as i32,
        })
        .filter(|point| desktop::window_at(*point) != overlay)
        .map(|point| (point.x, point.y))
        .collect()
}

fn dragging(report: &mut Report, session: &mut Session) {
    session.ticker.write_settings(&settings(json!({ "locked": false })));
    let Some(before) = session.ticker.wait_for(5.0, |snapshot| !snapshot.locked && snapshot.mode == "collapsed") else {
        return report.line("FAIL", "drag-unlocked", "the unlock edit did not land");
    };
    let from = before.ticker().center();
    let to = POINT { x: from.x + 150, y: from.y + 80 };
    let start = before.ticker();
    let moved = session.gesture(
        "drag unlocked ticker",
        3.0,
        |mouse, allowed| mouse.drag(from, to, allowed),
        |snapshot| {
            let ticker = snapshot.ticker();
            snapshot.mode == "collapsed"
                && (ticker.x - start.x - 150.0).abs() <= 2.0
                && (ticker.y - start.y - 80.0).abs() <= 2.0
        },
    );
    let saved = session.ticker.saved_settings().and_then(|saved| saved.get("position").cloned());
    match moved {
        Ok(after) => {
            let ticker = after.ticker();
            let persisted = saved.as_ref().is_some_and(|position| {
                (position["x"].as_f64().unwrap_or(f64::NAN) - ticker.x).abs() < 1.0
                    && (position["y"].as_f64().unwrap_or(f64::NAN) - ticker.y).abs() < 1.0
            });
            report.check(
                persisted,
                "drag-unlocked",
                format!("a real drag by (150, 80) moved the ticker {start} -> {ticker}, still collapsed; the settings file holds position {saved:?}"),
            );
        }
        Err(error) => report.line(
            "FAIL",
            "drag-unlocked",
            format!("the ticker at {start} did not follow a drag by (150, 80): {error}"),
        ),
    }
}

fn chart_manipulation(report: &mut Report, session: &mut Session, work: &Rect, shots: &Path) {
    let result = manipulate_chart(session, work);
    match result {
        Ok(detail) => report.line("PASS", "chart-drag-resize", detail),
        Err(error) => report.line("FAIL", "chart-drag-resize", error),
    }
    // Leave the menu check its expected unlocked, collapsed ticker even if this check failed midway.
    if let Some(mut saved) = session.ticker.saved_settings() {
        saved["locked"] = json!(false);
        session.ticker.write_settings(&saved);
        let _ = session.ticker.wait_for(5.0, |snapshot| !snapshot.locked);
    }
    if let Some(expanded) = session.ticker.snapshot().filter(|snapshot| snapshot.mode == "expanded") {
        screenshot(report, &expanded, shots, "expanded-resized", false);
        let _ = session.click("ticker", 5.0, |snapshot| snapshot.mode == "collapsed");
    }
}

/// Exercise the actual corner hit target and saved dimensions, with focus checked around every gesture.
fn manipulate_chart(session: &mut Session, work: &Rect) -> Result<String, String> {
    session.ticker.write_settings(&settings(json!({
        "locked": false, "refreshSeconds": 300, "chartWidth": 360, "chartHeight": 180,
        "position": { "x": work.x + 40.0, "y": work.y + 40.0 }
    })));
    session
        .ticker
        .wait_for(5.0, |snapshot| !snapshot.locked && (snapshot.ticker().x - work.x - 40.0).abs() < 1.0)
        .ok_or("chart manipulation settings did not apply")?;
    let opened = session.click("ticker", 5.0, |snapshot| snapshot.mode == "expanded")?;
    let from = session.target("chart")?;
    let to = POINT { x: from.x + 40, y: from.y + 30 };
    let moved = session.gesture(
        "drag open chart",
        3.0,
        |mouse, allowed| mouse.drag(from, to, allowed),
        |snapshot| {
            snapshot.mode == "expanded"
                && (snapshot.ticker().x - opened.ticker().x - 40.0).abs() <= 2.0
                && (snapshot.ticker().y - opened.ticker().y - 30.0).abs() <= 2.0
        },
    )?;
    let scale = desktop::scale(desktop::hwnd(moved.hwnd));
    let start = *moved.targets.get("chart").ok_or("open chart has no chart target")?;
    let grip = session.target("resize")?;
    let ticker = moved.ticker().center();
    let dx = if grip.x > ticker.x { 80 } else { -80 };
    let dy = if grip.y > ticker.y { 60 } else { -60 };
    let grown = session.gesture(
        "grow open chart",
        3.0,
        |mouse, allowed| mouse.drag(grip, POINT { x: grip.x + dx, y: grip.y + dy }, allowed),
        |snapshot| {
            snapshot.mode == "expanded"
                && snapshot.targets.get("chart").is_some_and(|chart| {
                    (chart.width - start.width - 80.0).abs() <= 2.0 && (chart.height - start.height - 60.0).abs() <= 2.0
                })
        },
    )?;
    if !grown.ticker().same_spot(&moved.ticker()) {
        return Err("resizing moved the ticker anchor".to_string());
    }
    let grip = session.target("resize")?;
    let shrunk = session.gesture(
        "shrink open chart",
        3.0,
        |mouse, allowed| mouse.drag(grip, POINT { x: grip.x - dx / 2, y: grip.y - dy / 2 }, allowed),
        |snapshot| {
            snapshot.mode == "expanded"
                && snapshot.targets.get("chart").is_some_and(|chart| {
                    (chart.width - start.width - 40.0).abs() <= 2.0 && (chart.height - start.height - 30.0).abs() <= 2.0
                })
        },
    )?;
    let mut saved = session.ticker.saved_settings().ok_or("no saved settings after resize")?;
    if (saved["chartWidth"].as_f64().unwrap_or_default() - 360.0 - 40.0 / scale).abs() > 1.0
        || (saved["chartHeight"].as_f64().unwrap_or_default() - 180.0 - 30.0 / scale).abs() > 1.0
    {
        return Err(format!("resized dimensions were not saved in DIPs: {saved}"));
    }
    session.click("ticker", 5.0, |snapshot| snapshot.mode == "collapsed")?;
    let reopened = session.click("ticker", 5.0, |snapshot| snapshot.mode == "expanded")?;
    if !reopened.frame.same(&shrunk.frame) {
        return Err(format!("reopened at {}, expected {}", reopened.frame, shrunk.frame));
    }
    let grip = session.target("resize")?;
    saved["locked"] = json!(true);
    session.ticker.write_settings(&saved);
    session
        .ticker
        .wait_for(5.0, |snapshot| snapshot.locked && !snapshot.targets.contains_key("resize"))
        .ok_or("locking did not remove the resize grip")?;
    session.gesture(
        "drag former resize grip while locked",
        3.0,
        |mouse, allowed| mouse.drag(grip, POINT { x: grip.x + dx, y: grip.y + dy }, allowed),
        |snapshot| snapshot.mode == "expanded" && snapshot.frame.same(&reopened.frame),
    )?;
    if session.ticker.saved_settings() != Some(saved) {
        return Err("dragging the locked corner changed saved settings".to_string());
    }
    Ok(format!(
        "real input moved the open chart (40, 30), grew it (80, 60), shrank it (40, 30), saved DIP dimensions at scale {scale}, reopened at {}, and left the locked chart unchanged",
        reopened.frame
    ))
}

fn menu(report: &mut Report, session: &mut Session) {
    let opened = session.open_menu();
    let game_in_front = desktop::foreground() == session.game.hwnd;
    let closed = opened.clone().and_then(|_| {
        let game = desktop::window_rect(session.game.hwnd);
        let outside = POINT { x: (game.x + 80.0) as i32, y: (game.max_y() - 80.0) as i32 };
        session.mouse.click(outside, false, &[session.game.hwnd])?;
        let pid = session.ticker.pid;
        wait_until(3.0, || desktop::windows_of(pid, MENU_CLASS).is_empty().then_some(()))
            .ok_or_else(|| "the menu stayed open after a click on the game".to_string())
    });
    report.check(
        opened.is_ok() && closed.is_ok() && game_in_front && desktop::foreground() == session.game.hwnd,
        "menu",
        match (&opened, &closed) {
            (Ok(_), Ok(())) => format!(
                "a real right-click opened the menu with the stand-in game still foreground ({game_in_front}); a click on the game closed it"
            ),
            (Err(error), _) | (_, Err(error)) => error.clone(),
        },
    );

    let locked = session.open_menu().and_then(|menu| {
        let row = Session::menu_row(menu, 1.0);
        session.gesture(
            "menu Lock Position",
            3.0,
            |mouse, allowed| mouse.click(row, false, allowed),
            |snapshot| snapshot.locked,
        )
    });
    let saved = session.ticker.saved_settings().and_then(|saved| saved.get("locked").cloned());
    report.check(
        locked.is_ok() && saved == Some(json!(true)),
        "menu-lock",
        format!(
            "chose Lock Position from the menu: locked {}, the settings file holds locked {saved:?}",
            locked.is_ok()
        ),
    );

    let Some(before) = session.ticker.snapshot() else { return };
    let start = before.ticker();
    let from = start.center();
    let to = POINT { x: from.x + 150, y: from.y + 80 };
    let position = session.ticker.saved_settings().and_then(|saved| saved.get("position").cloned());
    // Released off the ticker, a locked press is neither a drag nor a click on the body, so nothing changes.
    let dragged = session.gesture(
        "drag locked ticker",
        3.0,
        |mouse, allowed| mouse.drag(from, to, allowed),
        |snapshot| snapshot.mode == "collapsed" && snapshot.ticker().same_spot(&start),
    );
    let position_after = session.ticker.saved_settings().and_then(|saved| saved.get("position").cloned());
    report.check(
        dragged.is_ok() && position == position_after,
        "drag-locked",
        format!(
            "a real drag by (150, 80) on the locked ticker at {start}: stayed put and collapsed {}, saved position unchanged {}",
            dragged.is_ok(),
            position == position_after
        ),
    );
}

fn focus(report: &mut Report, session: &Session) {
    let snapshot = session.ticker.snapshot();
    let activations = snapshot.as_ref().map_or(u32::MAX, |snapshot| snapshot.activations);
    let lost: Vec<&str> = session
        .gestures
        .iter()
        .filter(|(_, before, after)| !before || !after)
        .map(|(name, ..)| name.as_str())
        .collect();
    report.check(
        activations == 0 && lost.is_empty(),
        "focus-kept",
        if lost.is_empty() {
            format!(
                "the stand-in Wow.exe was foreground before and 0.3 s after each of {} gestures and the launch; the app became foreground {activations} times",
                session.gestures.len() - 1
            )
        } else {
            format!("the stand-in game lost the foreground around: {}; the app became foreground {activations} times", lost.join(", "))
        },
    );
    report.check(
        session.landed + 1 >= session.gestures.len(),
        "click (SendInput, real input)",
        format!("{} of {} real gestures had their effect", session.landed, session.gestures.len() - 1),
    );
    let foreign = snapshot.map_or(u32::MAX, |snapshot| snapshot.foreign_presses);
    match foreign {
        0 => report.line("PASS", "undisturbed", "the overlay received only the presses this check sent"),
        u32::MAX => report.line("FAIL", "undisturbed", "no snapshot"),
        _ => report.line("INCONCLUSIVE", "undisturbed", format!("{foreign} presses this check did not send")),
    }
}

/// Settings is the one window that takes the foreground, and it must hand it back to the game when it closes.
fn settings_window(report: &mut Report, session: &mut Session, shots: &Path) {
    let pid = session.ticker.pid;
    let was_locked = session.ticker.snapshot().is_some_and(|snapshot| snapshot.locked);
    let mut expected = session.ticker.saved_settings();
    if let Some(Value::Object(expected)) = &mut expected {
        expected.insert("locked".to_string(), json!(!was_locked));
    }
    let opened = session.open_menu().and_then(|menu| {
        session.mouse.click(Session::menu_row(menu, 0.0), false, &session.allowed())?;
        wait_until(3.0, || desktop::windows_of(pid, SETTINGS_CLASS).first().copied())
            .ok_or_else(|| "no Settings window within 3 s of choosing Settings".to_string())
    });
    let window = match opened {
        Ok(window) => window,
        Err(error) => return report.line("FAIL", "settings-window", error),
    };
    std::thread::sleep(std::time::Duration::from_millis(400));
    let in_front = desktop::foreground() == window;
    let frame = desktop::window_rect(window);
    let _ = desktop::capture(&frame, &shots.join("settings.png"));
    let toggled = desktop::child(window, "Button", "Lock position")
        .ok_or_else(|| "no Lock position checkbox".to_string())
        .and_then(|checkbox| session.mouse.click(desktop::window_rect(checkbox).center(), false, &[window]))
        .and_then(|()| {
            session
                .ticker
                .wait_for(3.0, |snapshot| snapshot.locked != was_locked)
                .ok_or_else(|| "the checkbox did not change the lock".to_string())
        });
    let saved = session.ticker.saved_settings();
    let only_the_lock = saved.is_some() && saved == expected;
    desktop::post_close(window);
    let handed_back = wait_until(3.0, || {
        (desktop::windows_of(pid, SETTINGS_CLASS).is_empty() && desktop::foreground() == session.game.hwnd)
            .then_some(())
    });
    report.check(
        in_front && toggled.is_ok() && only_the_lock && handed_back.is_some(),
        "settings-window",
        format!(
            "Settings opened {frame} in the foreground {in_front}; a real click on Lock position set locked {} ({}); the file changed in that key only {only_the_lock}; closing it gave the foreground back to the stand-in game {}",
            !was_locked,
            toggled.as_ref().map_or_else(Clone::clone, |_| "applied".to_string()),
            handed_back.is_some()
        ),
    );
}

/// Starting the app again on the same settings file, as a second click on a desktop shortcut does, must not add a
/// second ticker. It opens the running one's Settings instead, restoring Settings if it was minimized.
fn second_launch(report: &mut Report, session: &Session, work: &Path) {
    let pid = session.ticker.pid;
    let launch = || -> Result<std::process::ExitStatus, String> {
        let mut copy = std::process::Command::new(&session.ticker.exe)
            .env_remove("BTCTICKER_SETTINGS")
            .env("BTCTICKER_SETTINGS_FILE", work.join("BTCTicker").join("settings.json"))
            .env("BTCTICKER_STATE_FILE", work.join("second-state.json"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|error| format!("could not start a second copy: {error}"))?;
        wait_until(5.0, || copy.try_wait().ok().flatten()).ok_or_else(|| {
            let _ = copy.kill();
            let _ = copy.wait();
            "a second copy was still running after 5 s, so it was killed".to_string()
        })
    };
    let exited = launch();
    let settings = wait_until(3.0, || desktop::windows_of(pid, SETTINGS_CLASS).first().copied());
    let running = desktop::processes_running(&session.ticker.exe);
    let restored = settings.map(|window| {
        desktop::minimize(window);
        let minimized = wait_until(2.0, || desktop::is_minimized(window).then_some(())).is_some();
        let again = launch();
        let shown = wait_until(3.0, || (!desktop::is_minimized(window)).then_some(())).is_some();
        minimized && again.is_ok_and(|status| status.success()) && shown
    });
    if let Some(window) = settings {
        desktop::post_close(window);
    }
    let closed = wait_until(3.0, || desktop::windows_of(pid, SETTINGS_CLASS).is_empty().then_some(()));
    report.check(
        exited.as_ref().is_ok_and(|status| status.success())
            && running == [pid]
            && restored == Some(true)
            && closed.is_some(),
        "second-launch",
        format!(
            "a second copy on the same settings file exited with {}; still running {running:?} (the first is pid {pid}); the first opened Settings {}; a third copy restored minimized Settings {}; Settings closed again {}",
            exited.map_or_else(|error| error, |status| status.to_string()),
            settings.is_some(),
            restored == Some(true),
            closed.is_some()
        ),
    );
}

fn quit(report: &mut Report, session: &mut Session) {
    let chosen = session.open_menu().and_then(|menu| {
        let row = Session::menu_row(menu, 2.0);
        session.mouse.click(row, false, &session.allowed())
    });
    let status = chosen.is_ok().then(|| session.ticker.exited_within(10.0)).flatten();
    report.check(
        chosen.is_ok() && status.is_some_and(|status| status.success()),
        "quit",
        match (&chosen, status) {
            (Ok(()), Some(status)) => format!("chose Quit BTC Ticker from the menu: the process exited with {status}"),
            (Ok(()), None) => "chose Quit BTC Ticker, but the process was still running 10 s later".to_string(),
            (Err(error), _) => error.clone(),
        },
    );
}

/// The dollar price in a ticker line like "BTC $80,392  -0.06%", and how many decimals it shows.
fn shown_price(text: &str) -> Option<(f64, usize)> {
    let after = &text[text.find('$')? + 1..];
    let digits: String = after.chars().take_while(|c| c.is_ascii_digit() || *c == ',' || *c == '.').collect();
    let decimals = digits.split_once('.').map_or(0, |(_, fraction)| fraction.len());
    Some((digits.replace(',', "").parse().ok()?, decimals))
}

/// "BTC $80,392  -0.06%" with the label, "$80,392.17  +1.20%" without it and two decimals.
fn is_ticker_line(text: &str, label: bool, decimals: usize) -> bool {
    let rest = if label { text.strip_prefix("BTC ") } else { Some(text) };
    let change = text.rsplit_once("  ").is_some_and(|(_, change)| {
        change.ends_with('%') && change.trim_start_matches(['+', '-']).trim_end_matches('%').parse::<f64>().is_ok()
    });
    rest.is_some_and(|rest| rest.starts_with('$'))
        && change
        && shown_price(text).is_some_and(|(_, shown)| shown == decimals)
}

fn kraken_price() -> Option<f64> {
    let output = std::process::Command::new("curl.exe").args(["-s", "--max-time", "8", KRAKEN_TICKER]).output().ok()?;
    let body: Value = serde_json::from_slice(&output.stdout).ok()?;
    let pair = body.get("result")?.as_object()?.values().next()?;
    pair.get("c")?.get(0)?.as_str()?.parse().ok()
}

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |since| since.as_secs_f64())
}

fn describe(snapshot: Option<&Snapshot>) -> String {
    match snapshot {
        Some(snapshot) => format!(
            "'{}' feed {} mode {} timeframe {} zoom {} chart {} candles {} of {} locked {} visible {} frame {}",
            snapshot.text,
            snapshot.feed,
            snapshot.mode,
            snapshot.timeframe.as_deref().unwrap_or("-"),
            snapshot.zoom.as_deref().map_or("-".to_string(), |zoom| zoom.replace('×', "x")),
            snapshot.chart.as_deref().unwrap_or("-"),
            snapshot.candles,
            snapshot.visible_candles,
            snapshot.locked,
            snapshot.visible,
            snapshot.frame
        ),
        None => "no snapshot".to_string(),
    }
}
