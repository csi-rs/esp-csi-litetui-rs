//! Pre-capture touch configuration screen.
//!
//! Touch zones:
//! * top strip (`y < 35`): left half = previous field, right half = next field
//! * middle (`35 <= y < 205`): left half = decrement/clear, right half = increment/set
//! * bottom strip (`y >= 205`): START capture

use core::sync::atomic::Ordering;

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::{ACCENT, FRAME, HI, TXT};
use crate::config::{self, DeliveryMode, LogFormat, TRAFFIC_STEPS};
use crate::shared;

#[derive(Clone, Copy)]
enum Field {
    Mode,
    Channel,
    Traffic,
    Ht40,
    Delivery,
    Format,
    Flag(u8, &'static str),
    Shift,
}

const FIELDS: [Field; 13] = [
    Field::Mode,
    Field::Channel,
    Field::Traffic,
    Field::Ht40,
    Field::Delivery,
    Field::Format,
    Field::Flag(config::CSI_LLTF, "L-LTF"),
    Field::Flag(config::CSI_HTLTF, "HT-LTF"),
    Field::Flag(config::CSI_STBC_HTLTF2, "STBC HT-LTF2"),
    Field::Flag(config::CSI_LTF_MERGE, "LTF merge"),
    Field::Flag(config::CSI_CHANNEL_FILTER, "Chan filter"),
    Field::Flag(config::CSI_MANU_SCALE, "Manual scale"),
    Field::Shift,
];

pub struct ConfigUi {
    sel: usize,
}

impl ConfigUi {
    pub fn new() -> Self {
        // Seed the control atomics with sensible defaults (single-node sniffer).
        shared::MODE.store(config::DEFAULT_MODE as u8, Ordering::Relaxed);
        shared::CHANNEL.store(1, Ordering::Relaxed);
        shared::TRAFFIC_HZ.store(100, Ordering::Relaxed);
        shared::HT40_SEL.store(0, Ordering::Relaxed);
        shared::CSI_FLAGS.store(config::CSI_FLAGS_DEFAULT, Ordering::Relaxed);
        shared::CSI_SHIFT.store(0, Ordering::Relaxed);
        shared::DELIVERY.store(0, Ordering::Relaxed);
        shared::LOG_FORMAT.store(0, Ordering::Relaxed);
        Self { sel: 0 }
    }

    /// Returns `true` when START was pressed.
    pub fn on_touch(&mut self, x: u16, y: u16) -> bool {
        if y < 35 {
            let n = FIELDS.len();
            if x < 160 {
                self.sel = (self.sel + n - 1) % n;
            } else {
                self.sel = (self.sel + 1) % n;
            }
            false
        } else if y >= 205 {
            true
        } else {
            self.adjust(x >= 160);
            false
        }
    }

    fn adjust(&mut self, inc: bool) {
        match FIELDS[self.sel] {
            Field::Mode => {
                let m = super::current_mode();
                let m = if inc { m.next() } else { m.prev() };
                shared::MODE.store(m as u8, Ordering::Relaxed);
            }
            Field::Channel => {
                let c = shared::CHANNEL.load(Ordering::Relaxed);
                let c = if inc {
                    (c + 1).min(config::MAX_CHANNEL)
                } else {
                    c.saturating_sub(1).max(config::MIN_CHANNEL)
                };
                shared::CHANNEL.store(c, Ordering::Relaxed);
            }
            Field::Traffic => {
                let cur = shared::TRAFFIC_HZ.load(Ordering::Relaxed);
                let idx = TRAFFIC_STEPS.iter().position(|&v| v == cur).unwrap_or(2);
                let idx = if inc {
                    (idx + 1).min(TRAFFIC_STEPS.len() - 1)
                } else {
                    idx.saturating_sub(1)
                };
                shared::TRAFFIC_HZ.store(TRAFFIC_STEPS[idx], Ordering::Relaxed);
            }
            Field::Ht40 => {
                let v = shared::HT40_SEL.load(Ordering::Relaxed);
                let v = if inc { (v + 1).min(2) } else { v.saturating_sub(1) };
                shared::HT40_SEL.store(v, Ordering::Relaxed);
            }
            Field::Delivery => {
                let v = shared::DELIVERY.load(Ordering::Relaxed);
                shared::DELIVERY.store(if v == 0 { 1 } else { 0 }, Ordering::Relaxed);
            }
            Field::Format => {
                let v = shared::LOG_FORMAT.load(Ordering::Relaxed);
                shared::LOG_FORMAT.store(if v == 0 { 1 } else { 0 }, Ordering::Relaxed);
            }
            Field::Flag(bit, _) => {
                let mut flags = shared::CSI_FLAGS.load(Ordering::Relaxed);
                if inc {
                    flags |= bit;
                } else {
                    flags &= !bit;
                }
                shared::CSI_FLAGS.store(flags, Ordering::Relaxed);
            }
            Field::Shift => {
                let s = shared::CSI_SHIFT.load(Ordering::Relaxed);
                let s = if inc { (s + 1).min(15) } else { s.saturating_sub(1) };
                shared::CSI_SHIFT.store(s, Ordering::Relaxed);
            }
        }
    }

    fn value(field: Field) -> alloc::string::String {
        match field {
            Field::Mode => alloc::string::String::from(super::current_mode().label()),
            Field::Channel => alloc::format!("{}", shared::CHANNEL.load(Ordering::Relaxed)),
            Field::Traffic => alloc::format!("{} Hz", shared::TRAFFIC_HZ.load(Ordering::Relaxed)),
            Field::Ht40 => alloc::string::String::from(config::ht40_label(
                shared::HT40_SEL.load(Ordering::Relaxed),
            )),
            Field::Delivery => alloc::string::String::from(
                DeliveryMode::from_u8(shared::DELIVERY.load(Ordering::Relaxed)).label(),
            ),
            Field::Format => alloc::string::String::from(
                LogFormat::from_u8(shared::LOG_FORMAT.load(Ordering::Relaxed)).label(),
            ),
            Field::Flag(bit, _) => {
                let on = shared::CSI_FLAGS.load(Ordering::Relaxed) & bit != 0;
                alloc::string::String::from(if on { "[x]" } else { "[ ]" })
            }
            Field::Shift => alloc::format!("{}", shared::CSI_SHIFT.load(Ordering::Relaxed)),
        }
    }

    fn label(field: Field) -> &'static str {
        match field {
            Field::Mode => "Node mode",
            Field::Channel => "Channel",
            Field::Traffic => "Traffic",
            Field::Ht40 => "HT40 sec",
            Field::Delivery => "Delivery",
            Field::Format => "Log format",
            Field::Flag(_, name) => name,
            Field::Shift => "Scale shift",
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(3),
            ])
            .split(area);

        // Top: field navigation
        let nav = Paragraph::new("  < FIELD          FIELD >")
            .alignment(Alignment::Center)
            .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" CSI SCOPE - SETUP ")
                    .border_style(Style::default().fg(FRAME)),
            );
        f.render_widget(nav, rows[0]);

        // Middle: field list
        let mut lines: alloc::vec::Vec<Line> = alloc::vec::Vec::new();
        for (i, &field) in FIELDS.iter().enumerate() {
            let selected = i == self.sel;
            let marker = if selected { ">" } else { " " };
            let label_style = if selected {
                Style::default().fg(HI).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TXT)
            };
            let val_style = if selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(ACCENT)
            };
            lines.push(Line::from(alloc::vec![
                Span::styled(alloc::format!(" {} {:<13}", marker, Self::label(field)), label_style),
                Span::styled(Self::value(field), val_style),
            ]));
        }
        let list = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" - / +  (tap left / right) ")
                .border_style(Style::default().fg(FRAME)),
        );
        f.render_widget(list, rows[1]);

        // Bottom: START
        let start = Paragraph::new("START CAPTURE")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green)),
            );
        f.render_widget(start, rows[2]);
    }
}
