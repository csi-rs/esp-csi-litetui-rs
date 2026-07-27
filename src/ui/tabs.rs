//! Live instrument tabs: spectrum, phase, waterfall, signal trend, stats.

use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Rectangle};
use ratatui::widgets::{Axis, Chart, Dataset, GraphType, Paragraph};
use ratatui::Frame;

use super::{panel, Model, ACCENT, FRAME, HI, TXT};
use crate::shared::NUM_SUBCARRIERS;

/// Amplitude (linear magnitude) vs subcarrier index.
pub fn spectrum(f: &mut Frame, area: Rect, m: &Model) {
    let data: alloc::vec::Vec<(f64, f64)> = m
        .amps
        .iter()
        .enumerate()
        .map(|(i, &a)| (i as f64, a as f64))
        .collect();
    let y_max = m.amps.iter().copied().max().unwrap_or(0) as f64;
    let y_upper = if y_max < 10.0 { 10.0 } else { y_max + 5.0 };

    let datasets = alloc::vec![Dataset::default()
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(ACCENT))
        .data(&data)];

    let chart = Chart::new(datasets)
        .block(panel("Amplitude vs Subcarrier"))
        .x_axis(
            Axis::default()
                .style(Style::default().fg(FRAME))
                .bounds([0.0, NUM_SUBCARRIERS as f64])
                .labels(alloc::vec![Span::raw("0"), Span::raw("64"), Span::raw("127")]),
        )
        .y_axis(
            Axis::default()
                .style(Style::default().fg(FRAME))
                .bounds([0.0, y_upper])
                .labels(alloc::vec![
                    Span::raw("0"),
                    Span::raw(alloc::format!("{:.0}", y_upper))
                ]),
        );
    f.render_widget(chart, area);
}

/// Unwrapped phase (radians) vs subcarrier index.
pub fn phase(f: &mut Frame, area: Rect, m: &Model) {
    let min = m.phases.iter().map(|&(_, y)| y).fold(f64::INFINITY, f64::min);
    let max = m.phases.iter().map(|&(_, y)| y).fold(f64::NEG_INFINITY, f64::max);
    let lo = libm::floor(min - 1.0);
    let hi = libm::ceil(max + 1.0);

    let datasets = alloc::vec![Dataset::default()
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(Color::LightBlue))
        .data(&m.phases)];

    let chart = Chart::new(datasets)
        .block(panel("Unwrapped Phase (rad)"))
        .x_axis(
            Axis::default()
                .style(Style::default().fg(FRAME))
                .bounds([0.0, NUM_SUBCARRIERS as f64])
                .labels(alloc::vec![Span::raw("0"), Span::raw("127")]),
        )
        .y_axis(
            Axis::default()
                .style(Style::default().fg(FRAME))
                .bounds([lo, hi])
                .labels(alloc::vec![
                    Span::raw(alloc::format!("{:.1}", lo)),
                    Span::raw(alloc::format!("{:.1}", hi))
                ]),
        );
    f.render_widget(chart, area);
}

/// Time x frequency amplitude heatmap (newest row at the top).
pub fn waterfall(f: &mut Frame, area: Rect, m: &Model) {
    let rows = m.waterfall.len() as f64;
    let cols = NUM_SUBCARRIERS as f64;
    let canvas = Canvas::default()
        .block(panel("Waterfall (time x subcarrier)"))
        .x_bounds([0.0, cols])
        .y_bounds([0.0, rows])
        .paint(move |ctx| {
            for (y_idx, row) in m.waterfall.iter().enumerate() {
                for (x_idx, &amp) in row.iter().enumerate() {
                    let i = (amp as f64 / 60.0).clamp(0.0, 1.0);
                    // Perceptually ordered (magma-like) ramp: dark = strong.
                    let color = if i > 0.85 {
                        Color::Rgb(0, 0, 4)
                    } else if i > 0.65 {
                        Color::Rgb(20, 11, 52)
                    } else if i > 0.45 {
                        Color::Rgb(81, 18, 124)
                    } else if i > 0.25 {
                        Color::Rgb(182, 54, 121)
                    } else if i > 0.05 {
                        Color::Rgb(251, 136, 97)
                    } else {
                        Color::Rgb(252, 253, 191)
                    };
                    ctx.draw(&Rectangle {
                        x: x_idx as f64,
                        y: rows - (y_idx as f64) - 1.0,
                        width: 1.0,
                        height: 1.0,
                        color,
                    });
                }
            }
        });
    f.render_widget(canvas, area);
}

/// RSSI and SNR trend vs time.
pub fn signal(f: &mut Frame, area: Rect, m: &Model) {
    if m.hist_len == 0 {
        let p = Paragraph::new("Waiting for packets...")
            .block(panel("Signal (RSSI / SNR)"))
            .alignment(Alignment::Center)
            .style(Style::default().fg(TXT));
        f.render_widget(p, area);
        return;
    }
    let rssi = &m.rssi_hist[..m.hist_len];
    let snr = &m.snr_hist[..m.hist_len];
    let x_min = rssi.first().map(|&(x, _)| x).unwrap_or(0.0);
    let x_max = rssi.last().map(|&(x, _)| x).unwrap_or(10.0).max(x_min + 1.0);

    let datasets = alloc::vec![
        Dataset::default()
            .name("RSSI dBm")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(ACCENT))
            .data(rssi),
        Dataset::default()
            .name("SNR dB")
            .marker(symbols::Marker::Dot)
            .style(Style::default().fg(Color::Yellow))
            .data(snr),
    ];
    let chart = Chart::new(datasets)
        .block(panel("Signal (RSSI / SNR)"))
        .x_axis(
            Axis::default()
                .title("t (s)")
                .style(Style::default().fg(FRAME))
                .bounds([x_min, x_max])
                .labels(alloc::vec![
                    Span::raw(alloc::format!("{:.0}", x_min)),
                    Span::raw(alloc::format!("{:.0}", x_max))
                ]),
        )
        .y_axis(
            Axis::default()
                .title("dB")
                .style(Style::default().fg(FRAME))
                .bounds([-100.0, 40.0])
                .labels(alloc::vec![Span::raw("-100"), Span::raw("0"), Span::raw("40")]),
        );
    f.render_widget(chart, area);
}

/// Live statistics + last-packet metadata + SD status.
pub fn stats(f: &mut Frame, area: Rect, m: &Model) {
    use crate::shared;
    use core::sync::atomic::Ordering;

    let pps = esp_csi_rs::get_pps_rx();
    let rate_hz = esp_csi_rs::get_rx_rate_hz();
    let total = esp_csi_rs::get_total_rx_packets();
    let radio_drops = esp_csi_rs::get_dropped_packets_rx();
    // TX side: the only sign of life in ESP-NOW Fast Source mode, which
    // transmits the flood while all CSI is captured on the collector.
    let tx_pps = esp_csi_rs::get_pps_tx();
    let tx_total = esp_csi_rs::get_total_tx_packets();
    let ring_drops = shared::RING_DROPS.load(Ordering::Relaxed);
    let recs = shared::SD_RECORDS.load(Ordering::Relaxed);

    let bw = if m.bw == 1 { "40MHz" } else { "20MHz" };
    let phy = match m.sig_mode {
        0 => "11b/g",
        1 => "11n(HT)",
        3 => "11ac",
        _ => "?",
    };

    fn kv<'a>(k: &'a str, v: alloc::string::String) -> Line<'a> {
        Line::from(alloc::vec![
            Span::styled(k, Style::default().fg(TXT)),
            Span::styled(v, Style::default().fg(HI).add_modifier(Modifier::BOLD)),
        ])
    }

    let lines = alloc::vec![
        kv("RX pps avg : ", alloc::format!("{}", pps)),
        kv("RX rate Hz : ", alloc::format!("{}", rate_hz)),
        kv("total rx   : ", alloc::format!("{}", total)),
        kv("radio drops: ", alloc::format!("{}", radio_drops)),
        kv("ring drops : ", alloc::format!("{}", ring_drops)),
        kv("logged recs: ", alloc::format!("{}", recs)),
        kv("TX pps/tot : ", alloc::format!("{} / {}", tx_pps, tx_total)),
        Line::from(""),
        kv("phy / bw   : ", alloc::format!("{} / {}", phy, bw)),
        kv("mcs / rate : ", alloc::format!("{} / {}", m.mcs, m.rate)),
        kv("format     : ", alloc::string::String::from(crate::config::fmt_label(m.fmt))),
        kv("noise floor: ", alloc::format!("{} dBm", m.noise)),
        kv("seq / len  : ", alloc::format!("{} / {}", m.seq, m.csi_len)),
    ];

    let p = Paragraph::new(lines).block(panel("Statistics"));
    f.render_widget(p, area);
}
