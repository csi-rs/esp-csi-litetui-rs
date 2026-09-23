//! On-device scientific UI (rendered to the LCD via ratatui + mousefood).
//!
//! Four screens: [`Screen::Config`] (pre-capture setup), [`Screen::Live`]
//! (tabbed instruments), [`Screen::SavePrompt`], and [`Screen::Stopped`].
//! All input is single-touch; [`Ui::on_touch`] returns an [`Action`] the
//! core-1 main loop turns into SD-card / capture-control operations.

mod config_ui;
mod tabs;

use core::sync::atomic::Ordering;

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::config::{Config, NodeMode};
use crate::shared::{self, NUM_SUBCARRIERS};

/// Panel width/height in raw touch coordinates (matches the 320x240 LCD).
const SCREEN_W: u16 = 320;

const HIST_LEN: usize = 120;
const WATERFALL_ROWS: usize = 30;

/// Accent / chrome colours for a restrained, instrument-like look.
pub const ACCENT: Color = Color::Cyan;
pub const FRAME: Color = Color::Indexed(244); // mid grey
pub const TXT: Color = Color::Gray;
pub const HI: Color = Color::White;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Config,
    Live,
    SavePrompt,
    Stopped,
}

/// What the main loop should do after a touch.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    StartCapture,
    StopCapture,
    Save,
    Discard,
}

/// Live data model: rolling history filled from the shared atomics each frame.
pub struct Model {
    pub amps: [u8; NUM_SUBCARRIERS],
    pub phases: [(f64, f64); NUM_SUBCARRIERS],
    pub waterfall: [[u8; NUM_SUBCARRIERS]; WATERFALL_ROWS],
    pub rssi_hist: [(f64, f64); HIST_LEN],
    pub snr_hist: [(f64, f64); HIST_LEN],
    pub hist_len: usize,
    pub rssi: i32,
    pub noise: i32,
    pub channel: u8,
    pub mcs: u8,
    pub bw: u8,
    pub sig_mode: u8,
    pub rate: u16,
    pub seq: u16,
    pub csi_len: u16,
    pub fmt: u8,
    pub packets: usize,
    start_ms: u64,
    last_pkt: usize,
}

impl Model {
    fn new(now_ms: u64) -> Self {
        Self {
            amps: [0; NUM_SUBCARRIERS],
            phases: [(0.0, 0.0); NUM_SUBCARRIERS],
            waterfall: [[0; NUM_SUBCARRIERS]; WATERFALL_ROWS],
            rssi_hist: [(0.0, 0.0); HIST_LEN],
            snr_hist: [(0.0, 0.0); HIST_LEN],
            hist_len: 0,
            rssi: -100,
            noise: 0,
            channel: 0,
            mcs: 0,
            bw: 0,
            sig_mode: 0,
            rate: 0,
            seq: 0,
            csi_len: 0,
            fmt: 16, // RxCSIFmt::Undefined
            packets: 0,
            start_ms: now_ms,
            last_pkt: 0,
        }
    }

    fn reset(&mut self, now_ms: u64) {
        self.hist_len = 0;
        self.last_pkt = shared::PACKET_COUNT.load(Ordering::Relaxed);
        self.start_ms = now_ms;
        self.waterfall = [[0; NUM_SUBCARRIERS]; WATERFALL_ROWS];
    }

    fn snr(&self) -> i32 {
        self.rssi - self.noise
    }

    fn update(&mut self, now_ms: u64) {
        self.rssi = shared::RSSI.load(Ordering::Relaxed);
        self.noise = shared::NOISE_FLOOR.load(Ordering::Relaxed);
        self.channel = shared::RX_CHANNEL.load(Ordering::Relaxed);
        self.mcs = shared::MCS.load(Ordering::Relaxed);
        self.bw = shared::BANDWIDTH.load(Ordering::Relaxed);
        self.sig_mode = shared::SIG_MODE.load(Ordering::Relaxed);
        self.rate = shared::RATE.load(Ordering::Relaxed);
        self.seq = shared::SEQUENCE.load(Ordering::Relaxed);
        self.csi_len = shared::CSI_LEN.load(Ordering::Relaxed);
        self.fmt = shared::DATA_FORMAT.load(Ordering::Relaxed);
        let pc = shared::PACKET_COUNT.load(Ordering::Relaxed);
        self.packets = pc;

        for i in 0..NUM_SUBCARRIERS {
            self.amps[i] = shared::CSI_AMPLITUDES[i].load(Ordering::Relaxed);
            let phase = shared::CSI_PHASES[i].load(Ordering::Relaxed) as f64 / 1000.0;
            self.phases[i] = (i as f64, phase);
        }

        if pc != self.last_pkt {
            self.last_pkt = pc;
            // Scroll the waterfall and insert the newest row at the top.
            for r in (1..WATERFALL_ROWS).rev() {
                self.waterfall[r] = self.waterfall[r - 1];
            }
            self.waterfall[0] = self.amps;

            let t = now_ms.saturating_sub(self.start_ms) as f64 / 1000.0;
            let rssi = self.rssi as f64;
            let snr = self.snr() as f64;
            if self.hist_len < HIST_LEN {
                self.rssi_hist[self.hist_len] = (t, rssi);
                self.snr_hist[self.hist_len] = (t, snr);
                self.hist_len += 1;
            } else {
                for i in 0..HIST_LEN - 1 {
                    self.rssi_hist[i] = self.rssi_hist[i + 1];
                    self.snr_hist[i] = self.snr_hist[i + 1];
                }
                self.rssi_hist[HIST_LEN - 1] = (t, rssi);
                self.snr_hist[HIST_LEN - 1] = (t, snr);
            }
        }
    }
}

/// Top-level UI state.
pub struct Ui {
    pub screen: Screen,
    pub tab: u8,
    pub model: Model,
    config: config_ui::ConfigUi,
}

const TAB_COUNT: u8 = 5;
const TAB_NAMES: [&str; TAB_COUNT as usize] =
    ["Spectrum", "Phase", "Waterfall", "Signal", "Stats"];

impl Ui {
    pub fn new(now_ms: u64) -> Self {
        Self {
            screen: Screen::Config,
            tab: 0,
            model: Model::new(now_ms),
            config: config_ui::ConfigUi::new(),
        }
    }

    /// Refresh the live model (call once per frame).
    pub fn tick(&mut self, now_ms: u64) {
        if self.screen != Screen::Config {
            self.model.update(now_ms);
        }
    }

    /// Handle a single touch at raw panel coordinates.
    pub fn on_touch(&mut self, x: u16, y: u16, now_ms: u64) -> Action {
        match self.screen {
            Screen::Config => {
                if self.config.on_touch(x, y) {
                    self.model.reset(now_ms);
                    self.screen = Screen::Live;
                    return Action::StartCapture;
                }
                Action::None
            }
            Screen::Live => {
                // Top bar: left ~64px = STOP, right ~64px = NEXT tab.
                if y < 24 {
                    if x < 64 {
                        self.screen = Screen::SavePrompt;
                        return Action::StopCapture;
                    } else if x > SCREEN_W - 64 && current_mode().captures_csi() {
                        // The emitter screen has no tabs to cycle.
                        self.tab = (self.tab + 1) % TAB_COUNT;
                    }
                }
                Action::None
            }
            Screen::SavePrompt => {
                if y > 90 && y < 170 {
                    if x > 30 && x < 150 {
                        self.screen = Screen::Stopped;
                        return Action::Save;
                    } else if x > 170 && x < 290 {
                        self.screen = Screen::Stopped;
                        return Action::Discard;
                    }
                }
                Action::None
            }
            Screen::Stopped => {
                // Tap anywhere to set up and run another capture.
                shared::to_config();
                self.tab = 0;
                self.screen = Screen::Config;
                Action::None
            }
        }
    }

    pub fn render(&self, f: &mut Frame) {
        let area = f.area();
        match self.screen {
            Screen::Config => self.config.render(f, area),
            Screen::Live => self.render_live(f, area),
            Screen::SavePrompt => self.render_save_prompt(f, area),
            Screen::Stopped => self.render_stopped(f, area),
        }
    }

    fn render_live(&self, f: &mut Frame, area: Rect) {
        use ratatui::layout::{Constraint, Direction, Layout};
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        self.render_status_bar(f, rows[0]);

        // An emitter captures nothing, so the instrument tabs would only plot an
        // empty model. Show its transmit status instead.
        if !current_mode().captures_csi() {
            tabs::emitter(f, rows[1]);
            return;
        }

        match self.tab {
            0 => tabs::spectrum(f, rows[1], &self.model),
            1 => tabs::phase(f, rows[1], &self.model),
            2 => tabs::waterfall(f, rows[1], &self.model),
            3 => tabs::signal(f, rows[1], &self.model),
            _ => tabs::stats(f, rows[1], &self.model),
        }
    }

    fn render_status_bar(&self, f: &mut Frame, area: Rect) {
        use ratatui::layout::{Constraint, Direction, Layout};
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(8),
                Constraint::Min(0),
                Constraint::Length(8),
            ])
            .split(area);

        let stop = Paragraph::new("STOP")
            .alignment(ratatui::layout::Alignment::Center)
            .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(FRAME)));
        f.render_widget(stop, cols[0]);

        let cfg = Config::load();
        let tab = if cfg.mode.captures_csi() {
            TAB_NAMES[self.tab as usize]
        } else {
            "Emitter"
        };
        let sd = match shared::SD_STATUS.load(Ordering::Relaxed) {
            shared::SD_OK => Span::styled("REC", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            shared::SD_NO_CARD => Span::styled("NO SD", Style::default().fg(Color::Yellow)),
            shared::SD_ERROR => Span::styled("SD ERR", Style::default().fg(Color::Red)),
            _ => Span::styled("---", Style::default().fg(FRAME)),
        };
        let info = Line::from(alloc::vec![
            Span::styled(alloc::format!(" {} ", cfg.mode.label()), Style::default().fg(ACCENT)),
            Span::styled(alloc::format!("ch{} ", self.model.channel), Style::default().fg(TXT)),
            Span::styled(alloc::format!("{}dBm ", self.model.rssi), Style::default().fg(HI)),
            Span::styled(alloc::format!("SNR{} ", self.model.snr()), Style::default().fg(TXT)),
            sd,
        ]);
        let mid = Paragraph::new(info).block(
            Block::default()
                .borders(Borders::ALL)
                .title(alloc::format!(" {} ", tab))
                .border_style(Style::default().fg(FRAME)),
        );
        f.render_widget(mid, cols[1]);

        let next = Paragraph::new("NEXT")
            .alignment(ratatui::layout::Alignment::Center)
            .style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(FRAME)));
        f.render_widget(next, cols[2]);
    }

    fn render_save_prompt(&self, f: &mut Frame, area: Rect) {
        use ratatui::layout::{Alignment, Constraint, Direction, Layout};
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(6), Constraint::Length(5), Constraint::Min(0)])
            .split(area);

        let recs = shared::SD_RECORDS.load(Ordering::Relaxed);
        let drops = shared::RING_DROPS.load(Ordering::Relaxed);
        let title = Paragraph::new(alloc::format!(
            "\nStopped. {} records written to SD ({} dropped).\nKeep this capture file?",
            recs, drops
        ))
        .style(Style::default().fg(HI))
        .alignment(Alignment::Center);
        f.render_widget(title, rows[0]);

        let btns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(10),
                Constraint::Percentage(35),
                Constraint::Percentage(10),
                Constraint::Percentage(35),
                Constraint::Percentage(10),
            ])
            .split(rows[1]);
        let save = Paragraph::new("\nKEEP")
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Green)));
        let discard = Paragraph::new("\nDELETE")
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Red)));
        f.render_widget(save, btns[1]);
        f.render_widget(discard, btns[3]);
    }

    fn render_stopped(&self, f: &mut Frame, area: Rect) {
        let saved = shared::run_state() == shared::RUN_SAVED;
        let msg = if saved {
            "\n\n\nCapture file kept on SD card.\n\nTap anywhere to start a new capture."
        } else {
            "\n\n\nCapture file deleted.\n\nTap anywhere to start a new capture."
        };
        let p = Paragraph::new(msg)
            .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(p, area);
    }
}

/// Helper used by submodules: a standard bordered block with a title.
pub(crate) fn panel(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .title(alloc::format!(" {} ", title))
        .border_style(Style::default().fg(FRAME))
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
}

/// Convenience for the selected node mode (used by config + status).
pub(crate) fn current_mode() -> NodeMode {
    crate::config::mode_from_stored(shared::MODE.load(Ordering::Relaxed))
}
