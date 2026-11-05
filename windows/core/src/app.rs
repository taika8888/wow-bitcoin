use crate::chart::{
    CandleChart, ChartFeed, ChartNotice, FetchFailure, axis_ticks, time_label, time_ticks,
};
use crate::display::{TickerDisplay, Tone, format_price};
use crate::feed::{FeedSchedule, FeedStatus};
use crate::frame::{Draw, Env, Frame, Measure, TextStyle};
use crate::kraken;
use crate::layout::{ExpandedLayout, Point, Rect, ScreenArea, Size, placement};
use crate::market::{Candle, Quote, Time, Timeframe};
use crate::overlay::{
    DragPhase, Frontmost, MenuItem, OverlayEvent, OverlayMode, OverlayState, PressTarget, reduce,
};
use crate::settings::{Rgba, ScreenPoint, Settings};
use crate::snapshot::{Snapshot, SnapshotFrame};

/// A finished HTTP exchange as the shell saw it, body unparsed.
#[derive(Clone, Debug, PartialEq)]
pub enum HttpReply {
    Answered {
        status: u16,
        body: Vec<u8>,
    },
    /// Offline, refused or timed out.
    Unreachable,
}

/// Pointer input in screen physical px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerInput {
    LeftDown(Point),
    Move(Point),
    LeftUp(Point),
    /// WM_CAPTURECHANGED mid-press (Alt-Tab, a UAC prompt): the press ends without a click, and a drag drops at
    /// the cursor.
    CaptureLost(Point),
    RightUp(Point),
    /// WM_MOUSELEAVE.
    Left,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    /// A timer fired; `Effects::wake_in` says when the next one is needed.
    Tick,
    QuoteReply(HttpReply),
    CandlesReply(Timeframe, HttpReply),
    Pointer(PointerInput),
    /// EVENT_SYSTEM_FOREGROUND, already classified with `Frontmost::classify`.
    Foreground(Frontmost),
    /// From the Settings window: a whole `Settings` built by a `FORM` setter.
    SettingsEdited(Settings),
    /// The stored value changed under us. Raw JSON, decoded with the usual fallbacks and never saved back.
    SettingsStored(String),
    /// A tray menu choice. The overlay's own menu arrives as pointer input.
    Menu(MenuItem),
    /// Resumed from sleep.
    Woke,
    /// Monitors, DPI or the work area changed; the `env` passed with this input is the new one.
    DisplayChanged,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub url: String,
    pub timeout_seconds: f64,
}

/// What the shell must do after one `handle`. Applying the same Effects twice converges to the same state: fetches
/// are gated by in-flight flags inside `App`, frames and saves are whole values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Effects {
    pub fetch_quote: Option<Request>,
    pub fetch_candles: Option<(Timeframe, Request)>,
    /// Canonical JSON to write to the store, only when the settings changed.
    pub save_settings: Option<String>,
    /// Paint this and UpdateLayeredWindow it. None when nothing on screen changed.
    pub present: Option<Frame>,
    /// SetCapture (true) or ReleaseCapture (false), when a press on the overlay starts or ends.
    pub capture: Option<bool>,
    pub open_settings: bool,
    pub quit: bool,
    /// Seconds until `handle(Input::Tick)` is next needed.
    pub wake_in: f64,
}

/// Facts only the shell can know, folded into the snapshot.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShellFacts {
    pub hwnd: u64,
    pub ex_style: u32,
    pub activations: u32,
    pub presses: u32,
    pub foreign_presses: u32,
    pub tray_icon: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Gesture {
    press: Point,
    window_origin: Point,
    pointer: Point,
}

impl Gesture {
    fn dragged_origin(&self) -> Point {
        Point {
            x: self.window_origin.x + self.pointer.x - self.press.x,
            y: self.window_origin.y + self.pointer.y - self.press.y,
        }
    }
}

pub struct App<M: Measure> {
    overlay: OverlayState,
    last_good: Option<Quote>,
    started_at: Time,
    now: Time,
    schedule: FeedSchedule,
    quote_in_flight: bool,
    charts: ChartFeed,
    gesture: Option<Gesture>,
    presented: Option<Frame>,
    snapshot_seq: u64,
    measure: M,
}

const WAKE_FLOOR: f64 = 0.05;
const WAKE_CEILING: f64 = 60.0;

impl<M: Measure> App<M> {
    /// `stored` is the raw store value, None on first run.
    pub fn new(stored: Option<&str>, frontmost: Frontmost, now: Time, measure: M) -> Self {
        App {
            overlay: OverlayState::new(Settings::decode(stored), frontmost),
            last_good: None,
            started_at: now,
            now,
            schedule: FeedSchedule::default(),
            quote_in_flight: false,
            charts: ChartFeed::default(),
            gesture: None,
            presented: None,
            snapshot_seq: 0,
            measure,
        }
    }

    pub fn handle(&mut self, input: Input, now: Time, env: &Env) -> Effects {
        self.now = now;
        let mut effects = Effects::default();
        let settings_before = self.overlay.settings.clone();
        let captured_before = self.gesture.is_some();
        let mut from_store = false;

        match input {
            Input::Tick | Input::DisplayChanged => {}
            Input::Woke => self.schedule.woke(),
            Input::QuoteReply(reply) => self.finish_quote(reply),
            Input::CandlesReply(timeframe, reply) => {
                self.charts
                    .finish(timeframe, candles_result(reply, timeframe), now)
            }
            Input::Pointer(pointer) => self.pointer(pointer, env, &mut effects),
            Input::Foreground(app) => self.send(OverlayEvent::AppActivated(app)),
            Input::SettingsEdited(settings) => self.send(OverlayEvent::SettingsEdited(settings)),
            Input::SettingsStored(json) => {
                from_store = true;
                self.send(OverlayEvent::SettingsEdited(Settings::decode(Some(&json))));
            }
            Input::Menu(MenuItem::Lock) => self.send(OverlayEvent::ToggleLock),
            Input::Menu(MenuItem::Settings) => effects.open_settings = true,
            Input::Menu(MenuItem::Quit) => effects.quit = true,
        }

        if self.overlay.drag == DragPhase::Idle {
            self.gesture = None;
        }
        if self.gesture.is_some() != captured_before {
            effects.capture = Some(self.gesture.is_some());
        }
        if self.overlay.settings != settings_before && !from_store {
            effects.save_settings = Some(self.overlay.settings.to_json());
        }
        effects.fetch_quote = self.begin_quote();
        effects.fetch_candles = self
            .charts
            .begin_fetch(&self.overlay, now)
            .map(|timeframe| {
                (
                    timeframe,
                    Request {
                        url: kraken::ohlc_url(timeframe),
                        timeout_seconds: 10.0,
                    },
                )
            });
        let frame = self.layout(env);
        if self.presented.as_ref() != Some(&frame) {
            effects.present = Some(frame.clone());
        }
        self.presented = Some(frame);
        effects.wake_in = self.wake_in();
        effects
    }

    pub fn snapshot(&mut self, facts: ShellFacts) -> Snapshot {
        self.snapshot_seq += 1;
        let status = self.status();
        let snapshot_frame = |r: Rect| SnapshotFrame {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        };
        let frame = self.presented.as_ref();
        let expanded = match self.overlay.mode {
            OverlayMode::Expanded(timeframe) => Some(timeframe),
            OverlayMode::Collapsed => None,
        };
        let bars_drawn = frame.map_or(0, |f| f.bars_drawn);
        let newest = expanded
            .filter(|_| bars_drawn > 0)
            .and_then(|t| self.charts.get(t).candles().last());
        Snapshot {
            seq: self.snapshot_seq,
            text: TickerDisplay::new(&status, &self.overlay.settings).text(),
            feed: status.name().into(),
            fetched_at: match &status {
                FeedStatus::Live(quote) | FeedStatus::Stale(quote) => Some(quote.fetched_at.0),
                FeedStatus::Loading | FeedStatus::Offline => None,
            },
            mode: self.overlay.mode.name().into(),
            locked: self.overlay.settings.locked,
            opacity: self.overlay.settings.opacity,
            visible: self.overlay.is_visible(),
            menu_open: self.overlay.menu_open,
            hwnd: facts.hwnd,
            frame: snapshot_frame(frame.map_or(Rect::default(), |f| f.window)),
            scale: frame.map_or(1.0, |f| f.scale),
            timeframe: expanded.map(|t| t.spec().label.into()),
            chart: expanded.map(|t| self.charts.get(t).name().into()),
            candles: bars_drawn,
            newest_candle: newest.map(|c| c.time.0),
            last_close: newest.map(|c| c.close),
            targets: frame
                .map(|f| f.targets())
                .unwrap_or_default()
                .into_iter()
                .map(|(name, rect)| (name, snapshot_frame(rect)))
                .collect(),
            activations: facts.activations,
            foreign_presses: facts.foreign_presses,
            presses: facts.presses,
            ex_style: facts.ex_style,
            tray_icon: facts.tray_icon,
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.overlay.settings
    }

    fn send(&mut self, event: OverlayEvent) {
        self.overlay = reduce(&self.overlay, &event);
    }

    fn status(&self) -> FeedStatus {
        FeedStatus::derive(
            self.last_good.as_ref(),
            self.started_at,
            self.now,
            self.overlay.settings.refresh_seconds.into(),
        )
    }

    fn begin_quote(&mut self) -> Option<Request> {
        let refresh = self.overlay.settings.refresh_seconds;
        if self.quote_in_flight || self.schedule.next_start(refresh) > self.now {
            return None;
        }
        self.quote_in_flight = true;
        self.schedule.started(self.now);
        Some(Request {
            url: kraken::TICKER_URL.into(),
            timeout_seconds: FeedSchedule::request_timeout(refresh),
        })
    }

    fn finish_quote(&mut self, reply: HttpReply) {
        if !self.quote_in_flight {
            return;
        }
        self.quote_in_flight = false;
        let quote = match reply {
            HttpReply::Answered { status: 200, body } => kraken::parse_quote(&body, self.now).ok(),
            _ => None,
        };
        self.schedule.finished(quote.is_some());
        if quote.is_some() {
            self.last_good = quote;
        }
    }

    fn pointer(&mut self, input: PointerInput, env: &Env, effects: &mut Effects) {
        let Some(presented) = self.presented.clone() else {
            return;
        };
        match input {
            PointerInput::LeftDown(at) => {
                if presented.hit(at) == PressTarget::Outside {
                    return;
                }
                self.gesture = Some(Gesture {
                    press: at,
                    window_origin: presented.window.origin(),
                    pointer: at,
                });
                self.send(OverlayEvent::Pressed);
            }
            PointerInput::Move(at) => {
                if let Some(gesture) = &mut self.gesture {
                    gesture.pointer = at;
                    let distance = at.distance(gesture.press);
                    self.send(OverlayEvent::PointerMoved { distance });
                }
            }
            PointerInput::LeftUp(at) | PointerInput::CaptureLost(at) => {
                let Some(gesture) = &mut self.gesture else {
                    return;
                };
                gesture.pointer = at;
                let window_origin = if self.overlay.drag == DragPhase::Dragging {
                    gesture.dragged_origin()
                } else {
                    presented.window.origin()
                };
                let ticker = presented.ticker.offset(window_origin);
                let window = Rect::new(window_origin, presented.window.size());
                let landed = placement::screen_for(&ticker, &env.screens)
                    .or_else(|| placement::screen_for(&window, &env.screens))
                    .map_or(ticker, |screen| placement::clamp(ticker, &screen.work));
                let over = match input {
                    PointerInput::LeftUp(_) => presented.hit(at),
                    _ => PressTarget::Outside,
                };
                let menu_click = self.overlay.menu_open && self.overlay.drag == DragPhase::Pressed;
                self.send(OverlayEvent::Released {
                    over,
                    ticker_origin: ScreenPoint {
                        x: landed.x,
                        y: landed.y,
                    },
                });
                match over {
                    PressTarget::Menu(MenuItem::Settings) if menu_click => {
                        effects.open_settings = true
                    }
                    PressTarget::Menu(MenuItem::Quit) if menu_click => effects.quit = true,
                    _ => {}
                }
            }
            PointerInput::RightUp(at) => {
                if self.gesture.is_none() && presented.hit(at) != PressTarget::Outside {
                    self.send(OverlayEvent::SecondaryClicked);
                }
            }
            PointerInput::Left => self.send(OverlayEvent::PointerLeft),
        }
    }

    fn wake_in(&self) -> f64 {
        let refresh = self.overlay.settings.refresh_seconds;
        let threshold = f64::from(refresh) * FeedStatus::MISSED_REFRESHES_BEFORE_STALE;
        let status_turns = match &self.last_good {
            Some(quote) => quote.fetched_at.plus(threshold),
            None => self.started_at.plus(threshold),
        };
        let quote_due = (!self.quote_in_flight).then(|| self.schedule.next_start(refresh));
        let chart_due = match self.overlay.mode {
            OverlayMode::Expanded(timeframe) if self.overlay.is_visible() => {
                self.charts.get(timeframe).due_at(timeframe)
            }
            _ => None,
        };
        [Some(status_turns.plus(0.001)), quote_due, chart_due]
            .into_iter()
            .flatten()
            .map(|at| at.since(self.now))
            .filter(|wait| *wait > 0.0)
            .fold(WAKE_CEILING, f64::min)
            .max(WAKE_FLOOR)
    }

    fn layout(&self, env: &Env) -> Frame {
        let settings = &self.overlay.settings;
        let scale = scale_at_saved_corner(settings.position, &env.screens);
        let px = |dips: f64| (dips * scale).round();
        let display = TickerDisplay::new(&self.status(), settings);
        let ticker_style = TextStyle {
            px: settings.font_size * scale,
        };
        let padding = Size {
            width: px(10.0),
            height: px(4.0),
        };
        let text_width: f64 = display
            .segments
            .iter()
            .map(|s| self.measure.advance(&s.text, ticker_style))
            .sum();
        let ticker_size = Size {
            width: text_width.ceil() + padding.width * 2.0,
            height: self.measure.line_height(ticker_style).ceil() + padding.height * 2.0,
        };
        let ticker_on_screen = placement::frame(settings.position, ticker_size, &env.screens);
        let work = placement::screen_for(&ticker_on_screen, &env.screens)
            .or(env.screens.first())
            .map_or(ticker_on_screen, |s| s.work);

        let label_style = TextStyle {
            px: (settings.font_size * 0.75).max(9.0) * scale,
        };
        let (mut window, mut ticker, mut timeframes, mut chart, opens_down, opens_right) =
            match self.overlay.mode {
                OverlayMode::Collapsed => (
                    ticker_on_screen,
                    Rect::new(Point::default(), ticker_size),
                    Vec::new(),
                    None,
                    ticker_on_screen.mid().y < work.mid().y,
                    ticker_on_screen.mid().x < work.mid().x,
                ),
                OverlayMode::Expanded(_) => {
                    let widest = Timeframe::ALL
                        .iter()
                        .map(|t| self.measure.advance(t.spec().label, label_style))
                        .fold(0.0, f64::max);
                    let segment = Size {
                        width: widest.ceil() + px(14.0),
                        height: (self.measure.line_height(label_style).ceil() + px(6.0))
                            .min(ticker_size.height),
                    };
                    let chart_size = Size {
                        width: px(settings.chart_width),
                        height: px(settings.chart_height),
                    };
                    let layout =
                        ExpandedLayout::new(ticker_on_screen, segment, chart_size, &work, scale);
                    (
                        layout.window,
                        layout.ticker,
                        layout.timeframes.to_vec(),
                        Some(layout.chart),
                        layout.opens_down,
                        layout.opens_right,
                    )
                }
            };
        let panel_size = window.size();

        let menu_style = TextStyle { px: 13.0 * scale };
        let mut menu = Vec::new();
        if self.overlay.menu_open {
            let labels = MenuItem::ALL.map(|item| item.label(settings));
            let row = Size {
                width: labels
                    .iter()
                    .map(|l| self.measure.advance(l, menu_style))
                    .fold(0.0, f64::max)
                    .ceil()
                    + px(12.0) * 2.0,
                height: self.measure.line_height(menu_style).ceil() + px(5.0) * 2.0,
            };
            let height = row.height * 3.0 + px(4.0) * 2.0;
            let x = if opens_right {
                ticker.x
            } else {
                ticker.max_x() - row.width
            };
            let top = if opens_down {
                ticker.max_y() + px(4.0)
            } else {
                ticker.y - px(4.0) - height
            };
            menu = MenuItem::ALL
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    let rect = Rect {
                        x,
                        y: top + px(4.0) + row.height * i as f64,
                        width: row.width,
                        height: row.height,
                    };
                    (*item, rect)
                })
                .collect();
        }

        let panel = Rect::new(Point::default(), panel_size);
        let menu_box = menu_box(&menu, scale);
        let bounds = menu_box.map_or(panel, |m| panel.union(&m));
        let shift = Point {
            x: -bounds.x,
            y: -bounds.y,
        };
        window = Rect::new(
            Point {
                x: window.x - shift.x,
                y: window.y - shift.y,
            },
            bounds.size(),
        );
        if let Some(gesture) = self
            .gesture
            .filter(|_| self.overlay.drag == DragPhase::Dragging)
        {
            window = Rect::new(gesture.dragged_origin(), window.size());
        }
        ticker = ticker.offset(shift);
        for (_, rect) in &mut timeframes {
            *rect = rect.offset(shift);
        }
        chart = chart.map(|c| c.offset(shift));
        for (_, rect) in &mut menu {
            *rect = rect.offset(shift);
        }

        let mut draw = vec![Draw::RoundRect {
            rect: panel.offset(shift),
            radius: px(8.0).min(ticker_size.height / 2.0),
            color: BLACK.with_alpha(settings.opacity),
        }];
        let fade = if display.dimmed { 0.5 } else { 1.0 };
        let mut pen = ticker.x + padding.width;
        for segment in &display.segments {
            let color = tone_color(segment.tone, settings);
            draw.push(Draw::Text {
                origin: Point {
                    x: pen,
                    y: ticker.y + padding.height,
                },
                text: segment.text.clone(),
                style: ticker_style,
                color: color.with_alpha(color.alpha * fade),
            });
            pen += self.measure.advance(&segment.text, ticker_style);
        }

        let mut bars_drawn = 0;
        if let (OverlayMode::Expanded(selected), Some(chart)) = (self.overlay.mode, chart) {
            for (timeframe, rect) in &timeframes {
                let chosen = *timeframe == selected;
                draw.push(Draw::RoundRect {
                    rect: Rect {
                        x: rect.x + px(1.0),
                        width: rect.width - px(1.0) * 2.0,
                        ..*rect
                    },
                    radius: px(4.0),
                    color: WHITE.with_alpha(if chosen { 0.26 } else { 0.08 }),
                });
                let label = timeframe.spec().label;
                let text = settings.text_color;
                draw.push(Draw::Text {
                    origin: Point {
                        x: rect.mid().x - self.measure.advance(label, label_style) / 2.0,
                        y: rect.mid().y - self.measure.line_height(label_style) / 2.0,
                    },
                    text: label.into(),
                    style: label_style,
                    color: text.with_alpha(text.alpha * if chosen { 1.0 } else { 0.6 }),
                });
            }
            bars_drawn = self.draw_chart(&mut draw, chart, selected, settings, scale, env);
        }

        if let Some(menu_box) = menu_box {
            draw.push(Draw::RoundRect {
                rect: menu_box.offset(shift),
                radius: px(6.0),
                color: MENU_BACKGROUND,
            });
            for (item, rect) in &menu {
                draw.push(Draw::Text {
                    origin: Point {
                        x: rect.x + px(12.0),
                        y: rect.y + px(5.0),
                    },
                    text: item.label(settings).into(),
                    style: menu_style,
                    color: WHITE,
                });
            }
        }

        Frame {
            visible: self.overlay.is_visible(),
            window,
            scale,
            ticker,
            timeframes,
            menu,
            draw,
            bars_drawn,
        }
    }

    fn draw_chart(
        &self,
        draw: &mut Vec<Draw>,
        area: Rect,
        timeframe: Timeframe,
        settings: &Settings,
        scale: f64,
        env: &Env,
    ) -> usize {
        let px = |dips: f64| (dips * scale).round();
        let caption = TextStyle { px: 10.0 * scale };
        let text = settings.text_color;
        let load = self.charts.get(timeframe);
        let notice = load.notice(timeframe);
        let Some(chart) = CandleChart::new(load.candles(), timeframe) else {
            if let Some(ChartNotice::Placeholder(message)) = notice {
                let style = TextStyle { px: 11.0 * scale };
                draw.push(Draw::Text {
                    origin: Point {
                        x: area.mid().x - self.measure.advance(&message, style) / 2.0,
                        y: area.mid().y - self.measure.line_height(style) / 2.0,
                    },
                    text: message,
                    style,
                    color: text.with_alpha(text.alpha * 0.6),
                });
            }
            return 0;
        };

        let price_ticks = axis_ticks(chart.price_domain, 4);
        let price_labels: Vec<String> = price_ticks.iter().map(|p| format_price(*p, 0)).collect();
        let label_width = price_labels
            .iter()
            .map(|l| self.measure.advance(l, caption))
            .fold(0.0, f64::max)
            .ceil();
        let caption_height = self.measure.line_height(caption).ceil();
        let plot = Rect {
            x: area.x,
            y: area.y + px(2.0),
            width: area.width - label_width - px(6.0),
            height: area.height - caption_height - px(6.0),
        };
        let y_of = |price: f64| {
            let (low, high) = chart.price_domain;
            plot.y + (high - price) / (high - low) * plot.height
        };
        let (start, end) = chart.time_domain;
        let x_of = |time: Time| plot.x + time.since(start) / end.since(start) * plot.width;
        let grid = text.with_alpha(text.alpha * 0.12);
        let axis_text = text.with_alpha(text.alpha * 0.7);

        for (price, label) in price_ticks.iter().zip(&price_labels) {
            let y = y_of(*price);
            draw.push(Draw::Line {
                from: Point { x: plot.x, y },
                to: Point { x: plot.max_x(), y },
                width: px(1.0).max(1.0),
                dash: None,
                color: grid,
            });
            draw.push(Draw::Text {
                origin: Point {
                    x: plot.max_x() + px(6.0),
                    y: y - caption_height / 2.0,
                },
                text: label.clone(),
                style: caption,
                color: axis_text,
            });
        }
        let (time_ticks, step) = time_ticks(chart.time_domain, 4, env.utc_offset_seconds);
        for tick in time_ticks {
            let x = x_of(tick);
            draw.push(Draw::Line {
                from: Point { x, y: plot.y },
                to: Point { x, y: plot.max_y() },
                width: px(1.0).max(1.0),
                dash: None,
                color: grid,
            });
            let label = time_label(tick, step, env.utc_offset_seconds);
            let width = self.measure.advance(&label, caption);
            draw.push(Draw::Text {
                origin: Point {
                    x: (x - width / 2.0).clamp(area.x, area.max_x() - width),
                    y: plot.max_y() + px(3.0),
                },
                text: label,
                style: caption,
                color: axis_text,
            });
        }

        let body_width = (plot.width / chart.bars.len() as f64 * 0.7).max(1.0);
        for bar in &chart.bars {
            let color = tone_color(bar.tone, settings);
            let x = x_of(bar.time);
            draw.push(Draw::Line {
                from: Point {
                    x,
                    y: y_of(bar.high),
                },
                to: Point {
                    x,
                    y: y_of(bar.low),
                },
                width: px(1.0).max(1.0),
                dash: None,
                color,
            });
            let top = y_of(bar.body_high);
            draw.push(Draw::Rect {
                rect: Rect {
                    x: x - body_width / 2.0,
                    y: top,
                    width: body_width,
                    height: y_of(bar.body_low) - top,
                },
                color,
            });
        }
        let last = y_of(chart.last_price);
        draw.push(Draw::Line {
            from: Point { x: plot.x, y: last },
            to: Point {
                x: plot.max_x(),
                y: last,
            },
            width: px(1.0).max(1.0),
            dash: Some((px(4.0), px(3.0))),
            color: text.with_alpha(text.alpha * 0.75),
        });

        if let Some(ChartNotice::Stale(marker)) = notice {
            let width = self.measure.advance(&marker, caption).ceil();
            let pill = Rect {
                x: plot.x + px(4.0),
                y: plot.y + px(4.0),
                width: width + px(5.0) * 2.0,
                height: caption_height + px(1.0) * 2.0,
            };
            draw.push(Draw::RoundRect {
                rect: pill,
                radius: pill.height / 2.0,
                color: BLACK.with_alpha(0.6),
            });
            draw.push(Draw::Text {
                origin: Point {
                    x: pill.x + px(5.0),
                    y: pill.y + px(1.0),
                },
                text: marker,
                style: caption,
                color: settings.down_color,
            });
        }
        chart.bars.len()
    }
}

const BLACK: Rgba = Rgba::opaque(0.0, 0.0, 0.0);
const WHITE: Rgba = Rgba::opaque(1.0, 1.0, 1.0);
const MENU_BACKGROUND: Rgba = Rgba {
    red: 0.14,
    green: 0.14,
    blue: 0.14,
    alpha: 0.96,
};

fn tone_color(tone: Tone, settings: &Settings) -> Rgba {
    match tone {
        Tone::Neutral => settings.text_color,
        Tone::Up => settings.up_color,
        Tone::Down => settings.down_color,
    }
}

fn menu_box(menu: &[(MenuItem, Rect)], scale: f64) -> Option<Rect> {
    let margin = (4.0 * scale).round();
    let first = menu.first()?.1;
    let last = menu.last()?.1;
    Some(Rect {
        x: first.x,
        y: first.y - margin,
        width: first.width,
        height: last.max_y() - first.y + margin * 2.0,
    })
}

fn scale_at_saved_corner(position: Option<ScreenPoint>, screens: &[ScreenArea]) -> f64 {
    position
        .and_then(|p| {
            screens
                .iter()
                .find(|s| s.frame.contains(Point { x: p.x, y: p.y }))
        })
        .or(screens.first())
        .map_or(1.0, |s| s.scale)
}

fn candles_result(reply: HttpReply, timeframe: Timeframe) -> Result<Vec<Candle>, FetchFailure> {
    match reply {
        HttpReply::Unreachable => Err(FetchFailure::Unreachable),
        HttpReply::Answered { status: 200, body } => {
            kraken::parse_candles(&body, timeframe).map_err(FetchFailure::Rejected)
        }
        HttpReply::Answered { status, .. } => Err(FetchFailure::Status(status)),
    }
}
