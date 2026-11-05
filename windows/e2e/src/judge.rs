//! Pure judgements over facts the Windows half gathered.
use crate::checks::Verdict;
use crate::pixels::Inspection;
use ticker_core::{Rect, Snapshot, SnapshotFrame, Timeframe};

/// Extended window style bits, with their winuser.h values so this module builds on any OS.
pub const WS_EX_TOPMOST: u32 = 0x0000_0008;
pub const WS_EX_TRANSPARENT: u32 = 0x0000_0020;
pub const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
pub const WS_EX_APPWINDOW: u32 = 0x0004_0000;
pub const WS_EX_LAYERED: u32 = 0x0008_0000;
pub const WS_EX_NOACTIVATE: u32 = 0x0800_0000;

const REQUIRED: [(u32, &str); 4] = [
    (WS_EX_LAYERED, "LAYERED"),
    (WS_EX_TOPMOST, "TOPMOST"),
    (WS_EX_NOACTIVATE, "NOACTIVATE"),
    (WS_EX_TOOLWINDOW, "TOOLWINDOW"),
];
const FORBIDDEN: [(u32, &str); 2] = [
    (WS_EX_TRANSPARENT, "TRANSPARENT"),
    (WS_EX_APPWINDOW, "APPWINDOW"),
];

/// Kraken's price and ours may be a few seconds apart.
pub const PRICE_TOLERANCE: f64 = 0.02;

pub fn rect(frame: &SnapshotFrame) -> Rect {
    Rect {
        x: frame.x,
        y: frame.y,
        width: frame.width,
        height: frame.height,
    }
}

pub fn describe(r: &Rect) -> String {
    format!("{},{} {}x{}", r.x, r.y, r.width, r.height)
}

pub fn same_rect(a: &Rect, b: &Rect) -> bool {
    [
        (a.x, b.x),
        (a.y, b.y),
        (a.width, b.width),
        (a.height, b.height),
    ]
    .iter()
    .all(|(p, q)| (p - q).abs() < 1.0)
}

pub struct DesktopFacts {
    /// None when ProcessIdToSessionId failed.
    pub session_id: Option<u32>,
    /// None when OpenInputDesktop failed, as on a locked or non-interactive session.
    pub input_desktop: Option<String>,
    pub screen: (i32, i32),
    /// Points where SendInput's move did not land GetCursorPos exactly, as "target->actual".
    pub cursor_misses: Vec<String>,
    pub cursor_tries: usize,
    /// A tagged SendInput click on the stand-in raised its press counter.
    pub click_reached_standin: bool,
    /// What BitBlt read at a stand-in pixel, against the fill it paints.
    pub capture: Option<[u8; 3]>,
    pub standin_fill: [u8; 3],
}

/// Inconclusive, never Pass, when any precondition for the other checks is missing; names every one that is.
pub fn desktop(f: &DesktopFacts) -> Verdict {
    let mut missing = Vec::new();
    match f.session_id {
        Some(0) => {
            missing.push("running in session 0, which has no interactive desktop".to_string())
        }
        None => missing.push("ProcessIdToSessionId failed".into()),
        Some(_) => {}
    }
    match f.input_desktop.as_deref() {
        Some("Default") => {}
        Some(other) => missing.push(format!("input desktop is {other}, not Default")),
        None => missing.push("OpenInputDesktop failed".into()),
    }
    if f.screen.0 <= 0 || f.screen.1 <= 0 {
        missing.push(format!("screen is {}x{}", f.screen.0, f.screen.1));
    }
    if !f.cursor_misses.is_empty() {
        missing.push(format!(
            "SendInput moved the cursor off target {}/{} times: {}",
            f.cursor_misses.len(),
            f.cursor_tries,
            f.cursor_misses.join(" ")
        ));
    }
    if !f.click_reached_standin {
        missing.push("a SendInput click on the stand-in never arrived".into());
    }
    if f.capture != Some(f.standin_fill) {
        missing.push(format!(
            "BitBlt read {:?} where the stand-in paints {:?}",
            f.capture, f.standin_fill
        ));
    }
    if missing.is_empty() {
        Verdict::Pass(format!(
            "session {}, input desktop Default, {}x{} screen, SendInput lands the cursor exactly ({} of {}), \
             a click reaches the stand-in, BitBlt reads its fill",
            f.session_id.unwrap_or_default(),
            f.screen.0,
            f.screen.1,
            f.cursor_tries,
            f.cursor_tries
        ))
    } else {
        Verdict::Inconclusive(missing.join("; "))
    }
}

/// The ticker line as `TickerDisplay` writes it: "BTC $80,482  +1.23%", or with any part switched off.
#[derive(Clone, Debug, PartialEq)]
pub struct TickerText {
    pub label: bool,
    pub price: f64,
    pub decimals: usize,
    pub change: bool,
    pub stale: bool,
}

pub fn parse_ticker(text: &str) -> Option<TickerText> {
    let (label, rest) = match text.strip_prefix("BTC ") {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (rest, stale) = match rest.strip_suffix("  stale") {
        Some(rest) => (rest, true),
        None => (rest, false),
    };
    let (price_text, change_text) = match rest.split_once("  ") {
        Some((price, change)) => (price, Some(change)),
        None => (rest, None),
    };
    let digits = price_text.strip_prefix('$')?;
    let (whole, fraction) = match digits.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => (digits, ""),
    };
    let groups: Vec<&str> = whole.split(',').collect();
    let grouped = !groups[0].is_empty()
        && groups[0].len() <= 3
        && groups[1..].iter().all(|g| g.len() == 3)
        && groups
            .iter()
            .chain([&fraction])
            .all(|g| g.bytes().all(|b| b.is_ascii_digit()));
    if !grouped || (digits.contains('.') && fraction.is_empty()) {
        return None;
    }
    let price: f64 = digits.replace(',', "").parse().ok()?;
    if let Some(change) = change_text {
        let number = change.strip_suffix('%')?;
        let number = number.trim_start_matches(['+', '-']);
        let (whole, fraction) = number.split_once('.')?;
        let digits_only = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        if !digits_only(whole) || fraction.len() != 2 || !digits_only(fraction) {
            return None;
        }
    }
    Some(TickerText {
        label,
        price,
        decimals: fraction.len(),
        change: change_text.is_some(),
        stale,
    })
}

pub fn near(value: f64, reference: f64) -> bool {
    reference > 0.0 && ((value - reference) / reference).abs() <= PRICE_TOLERANCE
}

fn compared(what: &str, shown: f64, reference: &Result<f64, String>) -> Result<String, Verdict> {
    match reference {
        Ok(kraken) if near(shown, *kraken) => Ok(format!(
            "{what} {shown} is within {:.0}% of the harness's own Kraken fetch {kraken}",
            PRICE_TOLERANCE * 100.0
        )),
        Ok(kraken) => Err(Verdict::Fail(format!(
            "{what} {shown} but the harness's own Kraken fetch says {kraken}, {:.2}% apart",
            ((shown - kraken) / kraken).abs() * 100.0
        ))),
        Err(error) => Err(Verdict::Inconclusive(format!(
            "{what} {shown}, but the harness could not fetch Kraken itself to compare: {error}"
        ))),
    }
}

pub fn live_quote(snapshot: &Snapshot, reference: &Result<f64, String>) -> Verdict {
    let parsed = parse_ticker(&snapshot.text);
    let Some(text) = parsed.filter(|t| t.label && t.change && !t.stale && snapshot.feed == "live")
    else {
        return Verdict::Fail(format!(
            "feed {} shows '{}', not a live 'BTC $price  change%' line",
            snapshot.feed, snapshot.text
        ));
    };
    if !(1_000.0..=10_000_000.0).contains(&text.price) {
        return Verdict::Fail(format!("price {} is outside the sane band", text.price));
    }
    match compared("shows", text.price, reference) {
        Ok(detail) => Verdict::Pass(format!("'{}': {detail}", snapshot.text)),
        Err(verdict) => verdict,
    }
}

/// What the harness waits for after clicking a timeframe: that timeframe's candles loaded and drawn.
pub fn charted(snapshot: &Snapshot, timeframe: Timeframe) -> bool {
    let spec = timeframe.spec();
    snapshot.mode == "expanded"
        && snapshot.timeframe.as_deref() == Some(spec.label)
        && snapshot.chart.as_deref() == Some("loaded")
        && snapshot.candles == spec.candles_kept
}

pub fn describe_chart(snapshot: &Snapshot) -> String {
    format!(
        "mode {} timeframe {} chart {} candles {}",
        snapshot.mode,
        snapshot.timeframe.as_deref().unwrap_or("-"),
        snapshot.chart.as_deref().unwrap_or("-"),
        snapshot.candles
    )
}

/// The chart is live, not a fixture or a stale cache: every candle kept is drawn, the newest opened within two
/// intervals of `now`, and its close is near the harness's own fetch.
pub fn chart(
    snapshot: &Snapshot,
    timeframe: Timeframe,
    now: f64,
    reference: &Result<f64, String>,
) -> Verdict {
    let spec = timeframe.spec();
    if !charted(snapshot, timeframe) {
        return Verdict::Fail(format!(
            "{}, expected {} with {} loaded candles",
            describe_chart(snapshot),
            spec.label,
            spec.candles_kept
        ));
    }
    let interval = timeframe.interval_seconds();
    let Some(newest) = snapshot.newest_candle.filter(|t| now - t <= interval * 2.0) else {
        return Verdict::Fail(format!(
            "{} candles, but the newest opened at {:?}, more than two {} intervals before now ({now:.0})",
            snapshot.candles, snapshot.newest_candle, spec.label
        ));
    };
    let Some(close) = snapshot.last_close else {
        return Verdict::Fail("the snapshot carries no last close".into());
    };
    match compared("last close", close, reference) {
        Ok(detail) => Verdict::Pass(format!(
            "{} {} candles loaded and drawn, newest opened {:.0}s ago, {detail}",
            snapshot.candles,
            spec.label,
            now - newest
        )),
        Err(verdict) => verdict,
    }
}

/// The open overlay outside the ticker row: the chart and its insets. The timeframe controls sit in the ticker row.
pub fn chart_band(window: &Rect, ticker: &Rect) -> Rect {
    let above = ticker.y - window.y;
    let below = window.max_y() - ticker.max_y();
    if below >= above {
        Rect {
            y: ticker.max_y(),
            height: below,
            ..*window
        }
    } else {
        Rect {
            height: above,
            ..*window
        }
    }
}

pub fn capture(inspection: &Inspection, png: &str) -> Verdict {
    Verdict::of(
        inspection.shows_candles(),
        format!("{png}: {inspection} inside the chart rect"),
        format!(
            "{png}: only {inspection} inside the chart rect, need {} colors and {} of each",
            crate::pixels::MIN_COLORS,
            crate::pixels::MIN_TONED
        ),
    )
}

pub struct StyleFacts {
    pub real: u32,
    pub published: u32,
    pub per_monitor_v2: bool,
    /// Every other visible top-level window of the app's process: (class, GWL_EXSTYLE).
    pub others: Vec<(String, u32)>,
}

/// A visible unowned window gets a taskbar button unless it is a tool window without WS_EX_APPWINDOW.
pub fn styles(f: &StyleFacts) -> Verdict {
    let mut wrong: Vec<String> = REQUIRED
        .iter()
        .filter(|(bit, _)| f.real & bit == 0)
        .map(|(_, name)| format!("{name} missing"))
        .chain(
            FORBIDDEN
                .iter()
                .filter(|(bit, _)| f.real & bit != 0)
                .map(|(_, name)| format!("{name} set")),
        )
        .collect();
    if f.published & !f.real != 0 {
        wrong.push(format!(
            "the app set {:#010x} but the window has {:#010x}",
            f.published, f.real
        ));
    }
    if !f.per_monitor_v2 {
        wrong.push("the window is not per-monitor-v2 DPI aware, so the manifest is missing".into());
    }
    for (class, style) in &f.others {
        if style & WS_EX_TOOLWINDOW == 0 || style & WS_EX_APPWINDOW != 0 {
            wrong.push(format!(
                "visible window {class} ({style:#010x}) gets a taskbar button"
            ));
        }
    }
    Verdict::of(
        wrong.is_empty(),
        format!(
            "GWL_EXSTYLE {:#010x}: LAYERED TOPMOST NOACTIVATE TOOLWINDOW set, TRANSPARENT APPWINDOW clear, so no \
             taskbar button and clicks are caught; per-monitor-v2 DPI aware, so the manifest is embedded; {} other \
             visible windows",
            f.real,
            f.others.len()
        ),
        wrong.join("; "),
    )
}

/// `top_down` is EnumWindows' order, which is the z-order from the top.
pub fn above(top_down: &[u64], overlay: u64, standin: u64) -> Verdict {
    let at = |hwnd| top_down.iter().position(|&h| h == hwnd);
    match (at(overlay), at(standin)) {
        (Some(o), Some(s)) => Verdict::of(
            o < s,
            format!("overlay is #{o} and the stand-in #{s} from the top of the z-order"),
            format!("the stand-in is #{s} and the overlay only #{o} from the top of the z-order"),
        ),
        (o, s) => Verdict::Inconclusive(format!(
            "not both in the window list: overlay {o:?}, stand-in {s:?}"
        )),
    }
}

/// The stand-in's re-raise must first be seen to win, or recovery was never exercised.
pub fn topmost_recovers(seen_below: bool, recovered_ms: Option<u64>, bound_ms: u64) -> Verdict {
    match (seen_below, recovered_ms) {
        (false, _) => Verdict::Inconclusive(
            "the stand-in's SetWindowPos(HWND_TOPMOST) never put it above the overlay, so recovery was not exercised"
                .into(),
        ),
        (true, Some(ms)) if ms <= bound_ms => Verdict::Pass(format!(
            "the stand-in re-raised itself above the overlay with no foreground change; the overlay was back on \
             top {ms} ms later (bound {bound_ms} ms)"
        )),
        (true, ms) => Verdict::Fail(format!(
            "the stand-in re-raised itself above the overlay and the overlay was not back on top within \
             {bound_ms} ms (after {ms:?})"
        )),
    }
}

/// Where a ticker with no saved position sits: centered at the top of the work area, clear of WoW's minimap.
pub fn at_default_spot(frame: &Rect, work: &Rect, scale: f64) -> bool {
    let top = work.y + (ticker_core::placement::MARGIN.height * scale).round();
    (frame.mid().x - work.mid().x).abs() <= 1.0 && (frame.y - top).abs() < 1.0
}

pub fn clamped(frame: &Rect, work: &Rect) -> bool {
    (frame.max_x() - work.max_x()).abs() < 1.0
        && (frame.y - work.y).abs() < 1.0
        && work.contains_rect(frame)
}

/// One foreground change seen by the harness's watcher: the window GetForegroundWindow returned from `at_ms` on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub at_ms: u64,
    pub hwnd: u64,
    pub pid: u32,
}

/// A stretch in which the harness moved the foreground on purpose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub from_ms: u64,
    pub to_ms: u64,
}

pub struct FocusFacts<'a> {
    /// Foreground changes in order, the first being the foreground when watching started.
    pub timeline: &'a [Sample],
    pub ended_ms: u64,
    pub standin: u64,
    pub app_pid: u32,
    pub excused: &'a [Span],
    pub activations: u32,
    /// The deliberate steal by the harness's helper window, which proves the watcher can see one.
    pub steal_seen: bool,
    /// What happened while watching, for the detail.
    pub covered: &'a str,
}

/// Pass only if the watcher is shown to see a steal, the app never owned the foreground, the overlay never got
/// WM_ACTIVATE, and every stretch held by another window lies inside one excused span.
pub fn focus_kept(f: &FocusFacts) -> Verdict {
    if !f.steal_seen {
        return Verdict::Inconclusive(
            "the watcher never saw the helper take the foreground on purpose, so it cannot be trusted to see a steal"
                .into(),
        );
    }
    if let Some(sample) = f.timeline.iter().find(|s| s.pid == f.app_pid) {
        return Verdict::Fail(format!(
            "BTC Ticker (pid {}) owned the foreground at {} ms",
            f.app_pid, sample.at_ms
        ));
    }
    if f.activations > 0 {
        return Verdict::Fail(format!(
            "the overlay received WM_ACTIVATE {} times",
            f.activations
        ));
    }
    let ends = f
        .timeline
        .iter()
        .skip(1)
        .map(|s| s.at_ms)
        .chain([f.ended_ms]);
    let stray = f.timeline.iter().zip(ends).find(|(sample, end)| {
        sample.hwnd != f.standin
            && !f
                .excused
                .iter()
                .any(|span| span.from_ms <= sample.at_ms && *end <= span.to_ms)
    });
    if let Some((sample, end)) = stray {
        return Verdict::Fail(format!(
            "window {:#x} of pid {} held the foreground from {} to {end} ms, outside any deliberate switch",
            sample.hwnd, sample.pid, sample.at_ms
        ));
    }
    Verdict::Pass(format!(
        "the stand-in held the foreground across {}; {} foreground changes, all inside the harness's deliberate \
         switches; 0 activations; the watcher saw the helper's steal",
        f.covered,
        f.timeline.len().saturating_sub(1)
    ))
}

pub fn undisturbed(foreign_presses: u32, presses: u32) -> Verdict {
    if foreign_presses == 0 {
        Verdict::Pass(format!(
            "all {presses} presses the overlay received carried the harness's input tag"
        ))
    } else {
        Verdict::Inconclusive(format!(
            "the overlay received {foreign_presses} presses the harness did not send, so someone used the mouse \
             and the click checks may reflect that; rerun"
        ))
    }
}

pub struct GateFacts {
    /// Clicking the helper (Other.exe) made it the foreground window.
    pub helper_took_foreground: bool,
    /// Milliseconds until the snapshot and IsWindowVisible both said hidden, if they did.
    pub hidden_ms: Option<u64>,
    /// Clicking the stand-in (Wow.exe) made it the foreground window again.
    pub standin_took_foreground: bool,
    pub shown_ms: Option<u64>,
}

pub fn wow_gate(f: &GateFacts) -> Verdict {
    match (f.helper_took_foreground, f.hidden_ms, f.standin_took_foreground, f.shown_ms) {
        (false, ..) => Verdict::Inconclusive(
            "clicking the helper did not move the foreground to it, so the gate was not exercised".into(),
        ),
        (true, None, ..) => Verdict::Fail(
            "the overlay stayed visible after Other.exe took the foreground".into(),
        ),
        (true, Some(_), false, _) => Verdict::Inconclusive(
            "clicking the stand-in did not give it the foreground back, so showing again was not exercised".into(),
        ),
        (true, Some(_), true, None) => Verdict::Fail(
            "the overlay stayed hidden after Wow.exe took the foreground back".into(),
        ),
        (true, Some(hidden), true, Some(shown)) => Verdict::Pass(format!(
            "with onlyWhenWoWActive, hidden within {hidden} ms of the click that gave Other.exe the foreground \
             and shown within {shown} ms of the click that gave it back to Wow.exe, per the snapshot and \
             IsWindowVisible"
        )),
    }
}

pub struct OpaqueFacts {
    /// WindowFromPoint on the harness's clear window's opaque strip found that window.
    pub control_window_present: bool,
    /// A click on its alpha-0 pixels reached the stand-in beneath and not the clear window.
    pub control_passed_through: bool,
    /// Grid points over the overlay at opacity 0 where WindowFromPoint found another window.
    pub misses: Vec<String>,
    pub grid_points: usize,
    /// A click near the overlay's corner, where only the hit floor is painted, toggled the chart.
    pub corner_click_toggled: bool,
    pub corner_click_reached_standin: bool,
}

pub fn opaque_to_clicks(f: &OpaqueFacts) -> Verdict {
    if !f.control_window_present || !f.control_passed_through {
        return Verdict::Inconclusive(format!(
            "the negative control did not show alpha-0 pixels passing clicks through (window present {}, \
             passed through {}), so a pass would prove nothing",
            f.control_window_present, f.control_passed_through
        ));
    }
    let mut wrong = Vec::new();
    if !f.misses.is_empty() {
        wrong.push(format!(
            "{} of {} points fall through: {}",
            f.misses.len(),
            f.grid_points,
            f.misses.join(" ")
        ));
    }
    if !f.corner_click_toggled {
        wrong.push("a click at the corner did not toggle the chart".into());
    }
    if f.corner_click_reached_standin {
        wrong.push("a click at the corner reached the stand-in".into());
    }
    Verdict::of(
        wrong.is_empty(),
        format!(
            "a click on the harness's alpha-0 window fell through to the stand-in, while at background opacity 0 all \
             {} grid points over the collapsed and expanded overlay hit it and a click at its corner toggled the \
             chart without reaching the stand-in",
            f.grid_points
        ),
        wrong.join("; "),
    )
}

pub struct SettingsWindowFacts {
    pub opened: bool,
    pub took_foreground: bool,
    pub trackbars: usize,
    /// One per slider row and one alpha slider per color row of `FORM`.
    pub expected_trackbars: usize,
    /// Clicking the "Show BTC label" checkbox changed the stored settings and the ticker text.
    pub toggle_applied: bool,
    pub closed: bool,
    pub foreground_returned: bool,
}

pub fn expected_trackbars() -> usize {
    ticker_core::FORM
        .iter()
        .filter(|field| {
            matches!(
                field.kind,
                ticker_core::FieldKind::Slider { .. } | ticker_core::FieldKind::Color { .. }
            )
        })
        .count()
}

pub fn settings_window(f: &SettingsWindowFacts) -> Verdict {
    if !f.opened {
        return Verdict::Fail(
            "choosing Settings in the overlay menu opened no Settings window".into(),
        );
    }
    let mut wrong = Vec::new();
    if !f.took_foreground {
        wrong.push("the Settings window did not take the foreground".to_string());
    }
    if f.trackbars != f.expected_trackbars {
        wrong.push(format!(
            "{} sliders, but FORM's slider and color rows need {}",
            f.trackbars, f.expected_trackbars
        ));
    }
    if !f.toggle_applied {
        wrong.push(
            "clicking Show BTC label did not change the stored settings and the ticker".into(),
        );
    }
    if !f.closed {
        wrong.push("WM_CLOSE did not close it".into());
    }
    if !f.foreground_returned {
        wrong.push("closing it did not hand the foreground back to the stand-in".into());
    }
    Verdict::of(
        wrong.is_empty(),
        format!(
            "opened from the overlay menu and took the foreground; {} sliders for FORM's slider and color rows; \
             clicking Show BTC label changed the stored settings and the ticker; WM_CLOSE closed it and the \
             stand-in got the foreground back",
            f.trackbars
        ),
        wrong.join("; "),
    )
}
