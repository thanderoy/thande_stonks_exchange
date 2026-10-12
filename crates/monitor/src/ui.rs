use crate::app::{EventKind, InputMode, MonitorState};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use rust_decimal::Decimal;

pub fn render(f: &mut Frame, state: &MonitorState) {
    let area = f.area();

    // Outer layout: header | main | footer
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(area);

    render_header(f, outer[0], state);
    render_main(f, outer[1], state);
    render_footer(f, outer[2], state);

    if state.mode == InputMode::SubmitForm {
        render_submit_popup(f, area, state);
    }
    if state.mode == InputMode::CancelInput {
        render_cancel_popup(f, area, state);
    }
}

fn render_header(f: &mut Frame, area: Rect, state: &MonitorState) {
    let conn_indicator = if state.connected { "●" } else { "○" };
    let conn_color = if state.connected { Color::Green } else { Color::Red };
    let mut spans = vec![
        Span::styled(" Thande Stonks Exchange ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(conn_indicator, Style::default().fg(conn_color)),
        Span::raw(format!(" {}  ", state.base_url)),
    ];
    if state.auto_stream {
        spans.push(Span::styled(
            format!(" ▶ AUTO {}ms ", state.auto_interval_ms),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_main(f: &mut Frame, area: Rect, state: &MonitorState) {
    // Left (symbols) | Right (book+metrics, events)
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(22), Constraint::Min(0)])
        .split(area);

    render_symbols(f, cols[0], state);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(cols[1]);

    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(right[0]);

    render_book(f, top[0], state);
    render_metrics(f, top[1], state);
    render_events(f, right[1], state);
}

fn render_symbols(f: &mut Frame, area: Rect, state: &MonitorState) {
    let items: Vec<ListItem> = state
        .symbols
        .iter()
        .enumerate()
        .map(|(i, sym)| {
            let session = state.sessions.get(sym).map(|s| s.as_str()).unwrap_or("?");
            let (session_color, session_short) = session_style(session);
            let prefix = if i == state.selected { "▶ " } else { "  " };
            let style = if i == state.selected {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{}{}", prefix, sym), style),
                Span::raw(" "),
                Span::styled(session_short, Style::default().fg(session_color)),
            ]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Symbols "),
    );
    f.render_widget(list, area);
}

fn session_style(state: &str) -> (Color, &str) {
    match state {
        "Open" => (Color::Green, "Open"),
        "PreMarket" => (Color::Yellow, "Pre"),
        "PostMarket" => (Color::Yellow, "Post"),
        "Closed" => (Color::DarkGray, "Closed"),
        "Halted" => (Color::Red, "Halted"),
        _ => (Color::DarkGray, "?"),
    }
}

fn render_book(f: &mut Frame, area: Rect, state: &MonitorState) {
    let sym = state.selected_symbol();
    let title = format!(" Order Book: {} ", sym);

    let book = state.books.get(sym);
    let (bids, asks) = book
        .map(|b| (b.bids.as_slice(), b.asks.as_slice()))
        .unwrap_or((&[], &[]));

    // Find max qty for bar scaling
    let max_qty = bids
        .iter()
        .chain(asks.iter())
        .map(|(_, q)| *q)
        .fold(Decimal::ZERO, |acc, q| if q > acc { q } else { acc });
    let max_qty_f = max_qty.to_string().parse::<f64>().unwrap_or(1.0).max(1.0);

    let bar_width = (area.width.saturating_sub(26)) as usize;

    let mut lines: Vec<Line> = Vec::new();

    // Asks (show in reverse so lowest ask is closest to spread)
    for (price, qty) in asks.iter().rev() {
        let qty_f = qty.to_string().parse::<f64>().unwrap_or(0.0);
        let bar_len = ((qty_f / max_qty_f) * bar_width as f64) as usize;
        let bar = "█".repeat(bar_len.min(bar_width));
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {:>9}  ", price),
                Style::default().fg(Color::Red),
            ),
            Span::styled(format!("{:<width$}", bar, width = bar_width), Style::default().fg(Color::Red)),
            Span::raw(format!(" {:>8}", qty)),
        ]));
    }

    // Spread line
    let spread_str = if !bids.is_empty() && !asks.is_empty() {
        let best_bid = bids[0].0;
        let best_ask = asks[0].0;
        let spread = best_ask - best_bid;
        format!(" spread {:>9} ", spread)
    } else {
        " ─── no crossing ─── ".to_string()
    };
    lines.push(Line::from(Span::styled(
        spread_str,
        Style::default().fg(Color::DarkGray),
    )));

    // Bids
    for (price, qty) in bids.iter() {
        let qty_f = qty.to_string().parse::<f64>().unwrap_or(0.0);
        let bar_len = ((qty_f / max_qty_f) * bar_width as f64) as usize;
        let bar = "█".repeat(bar_len.min(bar_width));
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {:>9}  ", price),
                Style::default().fg(Color::Green),
            ),
            Span::styled(format!("{:<width$}", bar, width = bar_width), Style::default().fg(Color::Green)),
            Span::raw(format!(" {:>8}", qty)),
        ]));
    }

    let para = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: false });
    f.render_widget(para, area);
}

fn render_metrics(f: &mut Frame, area: Rect, state: &MonitorState) {
    let m = &state.metrics;
    let lines = vec![
        Line::from(vec![
            Span::raw(" Submit/s   "),
            Span::styled(
                format!("{:>8.1}", m.submits_per_sec()),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw(" Trade/s    "),
            Span::styled(
                format!("{:>8.1}", m.trades_per_sec()),
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw(" p50 lat ms "),
            Span::styled(
                format!("{:>8.2}", m.latency_p50()),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(vec![
            Span::raw(" p99 lat ms "),
            Span::styled(
                format!("{:>8.2}", m.latency_p99()),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(vec![
            Span::raw(" Fill rate  "),
            Span::styled(
                format!("{:>7.1}%", m.fill_rate()),
                Style::default().fg(Color::Magenta),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw(" Total sub  "),
            Span::styled(
                format!("{:>8}", m.total_submits),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::raw(" Total trd  "),
            Span::styled(
                format!("{:>8}", m.total_trades),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
    ];

    let para = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Metrics "));
    f.render_widget(para, area);
}

fn render_events(f: &mut Frame, area: Rect, state: &MonitorState) {
    let max = area.height.saturating_sub(2) as usize;
    let items: Vec<ListItem> = state
        .events
        .iter()
        .take(max)
        .map(|e| {
            let (kind_str, kind_color) = match e.kind {
                EventKind::Trade => ("TRADE    ", Color::Green),
                EventKind::Accepted => ("ACCEPTED ", Color::Cyan),
                EventKind::Cancelled => ("CANCELLED", Color::Yellow),
                EventKind::Session => ("SESSION  ", Color::Magenta),
                EventKind::System => ("SYS      ", Color::DarkGray),
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {} ", e.ts.format("%H:%M:%S%.3f")),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!(" {}  ", kind_str),
                    Style::default().fg(kind_color),
                ),
                Span::raw(e.detail.clone()),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Event Feed "));
    f.render_widget(list, area);
}

fn render_footer(f: &mut Frame, area: Rect, state: &MonitorState) {
    let text = match state.mode {
        InputMode::Normal => {
            " [a]uto-stream  [+/-]speed  [s]ubmit  [c]ancel  [o]pen  [H]alt  [r]esume  [w]×10  [W]×100  [↑↓]sym  [q]uit"
        }
        InputMode::SubmitForm => " [Tab]next field  [Space]toggle side  [Enter]submit  [Esc]cancel",
        InputMode::CancelInput => " [Tab]next field  [Enter]cancel order  [Esc]back",
    };
    let para = Paragraph::new(text).block(Block::default().borders(Borders::ALL));
    f.render_widget(para, area);
}

fn centered_rect(percent_x: u16, height: u16, r: Rect) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((r.height.saturating_sub(height)) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(r);
    let horiz = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vert[1]);
    horiz[1]
}

fn field_style(active: bool) -> Style {
    if active {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::UNDERLINED)
    } else {
        Style::default()
    }
}

pub fn render_submit_popup(f: &mut Frame, area: Rect, state: &MonitorState) {
    let popup = centered_rect(50, 14, area);
    f.render_widget(Clear, popup);

    let form = &state.form;
    let fields = [
        ("Side    ", form.side.as_str(), 0_usize),
        ("Price   ", form.price.as_str(), 1),
        ("Qty     ", form.qty.as_str(), 2),
        ("Client  ", form.client.as_str(), 3),
    ];

    let mut lines = vec![
        Line::from(vec![
            Span::raw(" Symbol  "),
            Span::styled(format!(" {} ", &form.symbol), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(""),
    ];
    for (label, val, idx) in &fields {
        let active = form.active_field == *idx;
        let cursor = if active { "_" } else { "" };
        lines.push(Line::from(vec![
            Span::raw(format!(" {} ", label)),
            Span::styled(format!(" {}{} ", val, cursor), field_style(active)),
        ]));
    }

    if let Some(err) = &form.error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(" ⚠  {}", err),
            Style::default().fg(Color::Red),
        )));
    }

    let para = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Submit Order "));
    f.render_widget(para, popup);
}

pub fn render_cancel_popup(f: &mut Frame, area: Rect, state: &MonitorState) {
    let popup = centered_rect(50, 10, area);
    f.render_widget(Clear, popup);

    let form = &state.cancel_form;
    let fields = [
        ("Order ID", form.order_id.as_str(), 0_usize),
        ("Symbol  ", form.symbol.as_str(), 1),
        ("Side    ", form.side.as_str(), 2),
    ];

    let mut lines = vec![Line::from("")];
    for (label, val, _idx) in &fields {
        lines.push(Line::from(vec![
            Span::raw(format!(" {} ", label)),
            Span::raw(format!(" {} ", val)),
        ]));
    }

    if let Some(err) = &form.error {
        lines.push(Line::from(Span::styled(
            format!(" ⚠  {}", err),
            Style::default().fg(Color::Red),
        )));
    }

    let para = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Cancel Order "));
    f.render_widget(para, popup);
}
