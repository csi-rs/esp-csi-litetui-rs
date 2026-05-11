use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Paragraph, Gauge, Chart, Axis, Dataset, GraphType, Wrap
    },
    symbols,
    Frame,
};

pub const LOGO_TEXT: &str = r#"            █████╗ ██████╗██╗            
             ██╔═══╝ ██╔═══╝██║            
             ██║     ██████╗██║            
             ██║     ╚═══██║██║            
             ╚█████╗ ██████║██║            
              ╚════╝ ╚═════╝╚═╝            
██╗   ██╗██████╗█████╗   ██████╗██╗ ██╗██╗
██║   ██║╚═██╔═╝██╔══╝   ╚═██╔═╝██║ ██║██║
██║   ██║  ██║  ████╗      ██║  ██║ ██║██║
██║   ██║  ██║  ██╔═╝      ██║  ██║ ██║██║
█████╗██║  ██║  █████╗     ██║  ╚████╔╝██║
╚════╝╚═╝  ╚═╝  ╚════╝     ╚═╝   ╚═══╝ ╚═╝"#;

#[derive(PartialEq)]
pub enum Page {
    Welcome,
    Settings,
    Radar,
    SavePrompt,
    Halted,
}

pub fn render_frame(
    f: &mut Frame, 
    current_page: &Page, 
    finger_down: bool, 
    csi_phases_f64: &[(f64, f64); 64],
    waterfall_history: &[[u8; 64]; 30], 
    rssi: i32,
    active_tab: u8,
    packet_count: usize,
    packet_ring: &[([u8; 64], u64)],
    now_ms: u64,
    rssi_history: &[(f64, f64)],
    ema_history: &[(f64, f64)],
    rssi_count: usize
) {
    let area = f.area(); 

    match current_page {
        Page::Welcome => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(14), Constraint::Min(0), Constraint::Length(5), Constraint::Min(0), Constraint::Length(1), Constraint::Length(2)])
                .split(area);
            
            let logo = Paragraph::new(LOGO_TEXT).style(Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD))
                .alignment(Alignment::Center).block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(Color::Magenta)));
            f.render_widget(logo, chunks[0]);
            
            let button_layout = Layout::default().direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(25), Constraint::Percentage(50), Constraint::Percentage(25)]).split(chunks[2]);

            let btn_color = if finger_down { Color::Yellow } else { Color::Cyan };
            let start_text = Paragraph::new("\nSTART VISUALIZING").style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
                .alignment(Alignment::Center).block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(btn_color)));
            f.render_widget(start_text, button_layout[1]);

            // let credit_line = Line::from(alloc::vec![
            //     Span::styled("System Architects: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            //     Span::styled("Maryam & Abdullah", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            // ]);
            // f.render_widget(Paragraph::new(credit_line).alignment(Alignment::Center), chunks[4]);
        }

        Page::Settings => {
            let chunks = Layout::default().direction(Direction::Vertical)
                .constraints([Constraint::Percentage(33), Constraint::Percentage(33), Constraint::Percentage(34)]).split(area);

            let color = if finger_down { Color::Yellow } else { Color::Cyan };
            let style = Style::default().fg(Color::White).add_modifier(Modifier::BOLD);

            let p1 = Paragraph::new("\nConnect to an existing Wi-Fi Router").style(style).alignment(Alignment::Center)
                .block(Block::default().title(" STATION MODE ").borders(Borders::ALL).border_style(Style::default().fg(color)));
            let p2 = Paragraph::new("\nListen to all Wi-Fi traffic on channel").style(style).alignment(Alignment::Center)
                .block(Block::default().title(" SNIFFER MODE ").borders(Borders::ALL).border_style(Style::default().fg(color)));
            let p3 = Paragraph::new("\nDirect peer-to-peer connection").style(style).alignment(Alignment::Center)
                .block(Block::default().title(" ESP-NOW MODE ").borders(Borders::ALL).border_style(Style::default().fg(color)));

            f.render_widget(p1, chunks[0]);
            f.render_widget(p2, chunks[1]);
            f.render_widget(p3, chunks[2]);
        }

        Page::Radar => {
            let chunks = Layout::default().direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(0)]).split(area);

            let top_bar = Layout::default().direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(8),  // EXIT
                    Constraint::Min(0),     // RSSI Gauge
                    Constraint::Length(14), // PKT COUNT
                    Constraint::Length(8)   // NEXT
                ]).split(chunks[0]);

            let exit_style = Style::default().fg(if finger_down { Color::Yellow } else { Color::Red });
            let exit_text = Paragraph::new("EXIT").alignment(Alignment::Center).style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
                .block(Block::default().borders(Borders::ALL).border_style(exit_style));
            f.render_widget(exit_text, top_bar[0]);

            let rssi_clamped = rssi.clamp(-100, -30);
            let percent = (((rssi_clamped + 100) as f32 / 70.0) * 100.0) as u16;
            
            let gauge = Gauge::default()
                .block(Block::default().title(" RSSI ").borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)))
                .gauge_style(Style::default().fg(Color::Magenta).bg(Color::DarkGray))
                .percent(percent)
                .label(alloc::format!("{} dBm", rssi));
            f.render_widget(gauge, top_bar[1]);

            let pkt_text = Paragraph::new(alloc::format!("{}", packet_count))
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
                .block(Block::default().title(" PKTS ").borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)));
            f.render_widget(pkt_text, top_bar[2]);

            let next_style = Style::default().fg(if finger_down { Color::Yellow } else { Color::Green });
            let next_text = Paragraph::new("NEXT").alignment(Alignment::Center).style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
                .block(Block::default().borders(Borders::ALL).border_style(next_style));
            f.render_widget(next_text, top_bar[3]); 

            // ==========================================
            // MAIN TABS VISUALIZATION
            // ==========================================
            if active_tab == 0 {
                // 1. AMPLITUDE TAB (Recent Packets Subcarrier Graph)
                let mut recent_packets = alloc::vec::Vec::new();
                for (amp, t) in packet_ring.iter() {
                    // Extract packets arrived strictly within the last 1 second
                    if *t > 0 && now_ms.saturating_sub(*t) <= 1000 {
                        recent_packets.push((amp, t));
                    }
                }
                
                recent_packets.sort_by_key(|k| k.1);

                if recent_packets.is_empty() {
                    let block = Block::default().borders(Borders::ALL).border_type(BorderType::Double).title(" PACKET SUBCARRIER GRAPHS ").border_style(Style::default().fg(Color::Magenta));
                    let text = alloc::vec![
                        Line::from(Span::styled(" NO PACKETS IN THE LAST SECOND ", Style::default().add_modifier(Modifier::BOLD))),
                        Line::from(""),
                        Line::from(Span::raw("Waiting for new CSI packets in the current second...")),
                    ];
                    let p = Paragraph::new(text).block(block).alignment(Alignment::Center);
                    f.render_widget(p, chunks[1]);
                } else {
                    let mut datasets = alloc::vec::Vec::new();
                    let mut all_points = alloc::vec::Vec::new();
                    let mut lines = alloc::vec::Vec::new(); // Keeps slice references alive for charting

                    for (amp, _) in &recent_packets {
                        let mut series = alloc::vec::Vec::with_capacity(64);
                        for (sc_idx, amp_val) in amp.iter().enumerate() {
                            let x = sc_idx as f64;
                            let y = *amp_val as f64;
                            series.push((x, y));
                            all_points.push((x, y));
                        }
                        lines.push(series);
                    }

                    let colors = [Color::Red, Color::Green, Color::Yellow, Color::LightBlue, Color::Magenta, Color::Cyan];
                    for (idx, series) in lines.iter().enumerate() {
                        let color = colors[idx % colors.len()];
                        datasets.push(
                            Dataset::default()
                                .marker(symbols::Marker::Braille)
                                .graph_type(GraphType::Line)
                                .style(Style::default().fg(color))
                                .data(series)
                        );
                    }

                    // Dynamically compute upper Y bound
                    let y_max = all_points.iter().map(|&(_, y)| y).fold(0.0, |a: f64, b| if a > b { a } else { b });
                    let y_upper = libm::ceil(if y_max < 10.0 { 10.0 } else { y_max + 5.0 });

                    let chart = Chart::new(datasets)
                        .block(Block::default().borders(Borders::ALL).title(" Amplitude (Packets per Second) ").border_style(Style::default().fg(Color::Magenta)).border_type(BorderType::Double))
                        .x_axis(Axis::default().style(Style::default().fg(Color::Magenta)).bounds([0.0, 64.0]).labels(alloc::vec![Span::raw("0"), Span::raw("32"), Span::raw("64")]))
                        .y_axis(Axis::default().style(Style::default().fg(Color::Magenta)).bounds([0.0, y_upper])
                            .labels(alloc::vec![Span::raw("0"), Span::raw(alloc::format!("{:.0}", y_upper))]));

                    f.render_widget(chart, chunks[1]);
                }

            } else if active_tab == 1 {
                // 2. PHASE TAB
                let min_phase = csi_phases_f64.iter().map(|&(_, y)| y).fold(core::f64::INFINITY, |a, b| if a < b { a } else { b });
                let max_phase = csi_phases_f64.iter().map(|&(_, y)| y).fold(core::f64::NEG_INFINITY, |a, b| if a > b { a } else { b });
                
                let lower_bound = libm::floor(min_phase - 1.0);
                let upper_bound = libm::ceil(max_phase + 1.0);

                let datasets = alloc::vec![
                    Dataset::default().name("Unwrapped Phase").marker(symbols::Marker::Braille).graph_type(GraphType::Line)
                        .style(Style::default().fg(Color::LightBlue)).data(csi_phases_f64),
                ];

                let chart = Chart::new(datasets)
                    .block(Block::default().borders(Borders::ALL).title(" UNWRAPPED PHASE OFFSET ").border_style(Style::default().fg(Color::Magenta)).border_type(BorderType::Double))
                    .x_axis(Axis::default().style(Style::default().fg(Color::Magenta)).bounds([0.0, 64.0]).labels(alloc::vec![Span::raw("0"), Span::raw("64")]))
                    .y_axis(Axis::default().style(Style::default().fg(Color::Magenta)).bounds([lower_bound, upper_bound])
                        .labels(alloc::vec![Span::raw(alloc::format!("{:.1}", lower_bound)), Span::raw(alloc::format!("{:.1}", upper_bound))]));
                f.render_widget(chart, chunks[1]);
                
            } else if active_tab == 2 {
                // 3. WATERFALL TAB
                let history_len = 30.0;
                let num_subcarriers = 64.0;
                
                let canvas = ratatui::widgets::canvas::Canvas::default()
                    .block(Block::default().title(" WATERFALL (Time vs Frequency) ").borders(Borders::ALL).border_type(BorderType::Double).border_style(Style::default().fg(Color::Magenta)))
                    .x_bounds([0.0, num_subcarriers])
                    .y_bounds([0.0, history_len])
                    .paint(|ctx| {
                        for (y_idx, row) in waterfall_history.iter().enumerate() {
                            for (x_idx, &amp) in row.iter().enumerate() {
                                let intensity = (amp as f64 / 60.0).clamp(0.0, 1.0);
                                
                               let color = if intensity > 0.85 {
                                    Color::Rgb(0, 0, 4)       
                                } else if intensity > 0.65 {
                                    Color::Rgb(20, 11, 52)    
                                } else if intensity > 0.45 {
                                    Color::Rgb(81, 18, 124)   
                                } else if intensity > 0.25 {
                                    Color::Rgb(182, 54, 121)  
                                } else if intensity > 0.05 {
                                    Color::Rgb(251, 136, 97)  
                                } else {
                                    Color::Rgb(252, 253, 191) 
                                };
                                
                                let y_pos = history_len - (y_idx as f64);
                                
                                ctx.draw(&ratatui::widgets::canvas::Rectangle {
                                    x: x_idx as f64,
                                    y: y_pos - 1.0, 
                                    width: 1.0,    
                                    height: 1.0,    
                                    color,
                                });
                            }
                        }
                    });
                f.render_widget(canvas, chunks[1]);
            } else if active_tab == 3 {
                // 4. RSSI TAB
                if rssi_count == 0 {
                    let block = Block::default().borders(Borders::ALL).border_type(BorderType::Double).title(" RSSI & SIGNAL QUALITY ").border_style(Style::default().fg(Color::Magenta));
                    let p = Paragraph::new("Waiting for RSSI samples...").block(block).alignment(Alignment::Center);
                    f.render_widget(p, chunks[1]);
                } else {
                    let active_rssi = &rssi_history[..rssi_count];
                    let active_ema = &ema_history[..rssi_count];

                    let x_min = active_rssi.first().map(|(x, _)| *x).unwrap_or(0.0);
                    let x_max = active_rssi.last().map(|(x, _)| *x).unwrap_or(10.0).max(x_min + 1.0);

                    let datasets = alloc::vec![
                        Dataset::default().name("Raw RSSI").marker(symbols::Marker::Dot).style(Style::default().fg(Color::Cyan)).data(active_rssi),
                        Dataset::default().name("EMA Signal Trend").marker(symbols::Marker::Braille).graph_type(GraphType::Line).style(Style::default().fg(Color::Yellow)).data(active_ema),
                    ];

                    let chart = Chart::new(datasets)
                        .block(Block::default().title(" SIGNAL STRENGTH & HISTORY ").borders(Borders::ALL).border_type(BorderType::Double).border_style(Style::default().fg(Color::Magenta)))
                        .x_axis(Axis::default().title("Time (s)").bounds([x_min, x_max]).labels(alloc::vec![Span::raw(alloc::format!("{:.1}", x_min)), Span::raw(alloc::format!("{:.1}", x_max))]))
                        .y_axis(Axis::default().title("RSSI (dBm)").bounds([-100.0, -20.0]).labels(alloc::vec![Span::raw("-100"), Span::raw("-20")]));
                    
                    f.render_widget(chart, chunks[1]);
                }
            } else if active_tab == 4 {
                // 5. INFO / MANUAL TAB
                let text = alloc::vec![
                    Line::from(alloc::vec![Span::styled(" CSI LITE TUI Visualizer Manual ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))]),
                    Line::from(""), 
                    Line::from(alloc::vec![Span::styled(" 1. Amp (Spectrum): ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)), Span::styled("Subcarriers for packets/sec.", Style::default().fg(Color::White))]),
                    Line::from(""),
                    Line::from(alloc::vec![Span::styled(" 2. Phs (Unwrapped): ", Style::default().fg(Color::LightBlue).add_modifier(Modifier::BOLD)), Span::styled("Phase shift calculation.", Style::default().fg(Color::White))]),
                    Line::from(""),
                    Line::from(alloc::vec![Span::styled(" 3. Heat (Waterfall): ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)), Span::styled("Amplitude history over time.", Style::default().fg(Color::White))]),
                    Line::from(""),
                    Line::from(alloc::vec![Span::styled(" 4. RSSI (Signal): ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)), Span::styled("Signal strength tracking & EMA.", Style::default().fg(Color::White))]),
                    Line::from(""),
                    Line::from(alloc::vec![Span::styled("    [Waterfall Colormap Guide]", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))]),
                    Line::from(alloc::vec![Span::styled("    • Dark Indigo: ", Style::default().fg(Color::Rgb(20, 11, 52))), Span::styled("> 65% Intensity", Style::default().fg(Color::White))]),
                    Line::from(alloc::vec![Span::styled("    • Purple:      ", Style::default().fg(Color::Rgb(81, 18, 124))), Span::styled("> 45% Intensity", Style::default().fg(Color::White))]),
                    Line::from(alloc::vec![Span::styled("    • Magenta:     ", Style::default().fg(Color::Rgb(182, 54, 121))), Span::styled("> 25% Intensity", Style::default().fg(Color::White))]),
                    Line::from(alloc::vec![Span::styled("    • Orange:      ", Style::default().fg(Color::Rgb(251, 136, 97))), Span::styled("> 5%  Intensity", Style::default().fg(Color::White))]),
                    Line::from(alloc::vec![Span::styled("    • Pale Yellow: ", Style::default().fg(Color::Rgb(252, 253, 191))), Span::styled("Background/Noise", Style::default().fg(Color::White))]),
                ];

                let p = Paragraph::new(text)
                    .block(Block::default().borders(Borders::ALL).title(" Information ").border_style(Style::default().fg(Color::Magenta)).border_type(BorderType::Double))
                    .alignment(Alignment::Left)
                    .wrap(Wrap { trim: true });

                f.render_widget(p, chunks[1]);
            } else if active_tab == 5 {
                // 6. CREATORS TAB (NEW)
                let text = alloc::vec![
                    Line::from(""),
                    Line::from(alloc::vec![Span::styled(" System Creators:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))]),
                    Line::from(""),
                    Line::from(alloc::vec![
                        Span::styled(" Name                 GitHub", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    ]),
                    Line::from(alloc::vec![
                        Span::styled(" -----------------------------------", Style::default().fg(Color::DarkGray)),
                    ]),
                    Line::from(alloc::vec![
                        Span::styled(" Maryam Odat          ", Style::default().fg(Color::White)),
                        Span::styled("modatt", Style::default().fg(Color::Green)),
                    ]),
                    Line::from(alloc::vec![
                        Span::styled(" Abdullah Alawad      ", Style::default().fg(Color::White)),
                        Span::styled("Abdullah-Alawad", Style::default().fg(Color::Green)),
                    ]),
                ];

                let p = Paragraph::new(text)
                    .block(Block::default().borders(Borders::ALL).title(" Credits ").border_style(Style::default().fg(Color::Magenta)).border_type(BorderType::Double))
                    .alignment(Alignment::Left)
                    .wrap(Wrap { trim: true });

                f.render_widget(p, chunks[1]);
            }
        }

        Page::SavePrompt => {
            let chunks = Layout::default().direction(Direction::Vertical)
                .constraints([Constraint::Length(6), Constraint::Length(5), Constraint::Min(0)]).split(area);

            let title = Paragraph::new("\nSAVE CSI DATA TO SD CARD?")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)).alignment(Alignment::Center);
            f.render_widget(title, chunks[0]);

            let btn_chunks = Layout::default().direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(20), Constraint::Percentage(25), Constraint::Percentage(10), Constraint::Percentage(25), Constraint::Percentage(20)]).split(chunks[1]);

            let yes_btn = Paragraph::new("\nYES").alignment(Alignment::Center)
                .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Green)));
            let no_btn = Paragraph::new("\nNO").alignment(Alignment::Center)
                .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Red)));

            f.render_widget(yes_btn, btn_chunks[1]);
            f.render_widget(no_btn, btn_chunks[3]);
        }

        Page::Halted => {
            let text = Paragraph::new("\n\n\n\nSYSTEM HALTED.\n\nPLEASE RESET THE DEVICE\nTO START VISUALIZATION AGAIN.")
                .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)).alignment(Alignment::Center);
            f.render_widget(text, area);
        }
    }
}