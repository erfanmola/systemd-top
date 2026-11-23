use clap::Parser;
use crossterm::event::{self, Event, KeyCode};
use ratatui::{
    layout::{Constraint, Layout},
    style::Style,
    widgets::{Paragraph, Row, Table, TableState},
};
use std::{
    collections::HashMap,
    io,
    process::Command,
    time::{Duration, Instant},
};
#[derive(Parser)]
#[command(version, about = "Interactive systemd unit resource monitor")]
struct Args {
    #[arg(short = 'd', long, default_value = "2")]
    delay: f64,
    #[arg(short='s',long,default_value="time",value_parser=["time","memory","min","max","tasks","name","cpu"])]
    sort_key: String,
    #[arg(short = 'u', long)]
    user: bool,
    #[arg(short = 'F', long, default_value = "")]
    filter: String,
    #[arg(short = 'a', long)]
    all: bool,
    #[arg(long = "type", default_value = "service")]
    kind: String,
    #[arg(long)]
    raw: bool,
    #[arg(long)]
    once: bool,
    #[arg(short = 'C', long)]
    no_color: bool,
}
type Unit = HashMap<String, String>;
fn num(u: &Unit, k: &str) -> Option<u64> {
    u.get(k)?.parse().ok().filter(|n| *n != u64::MAX)
}
fn cmd(a: &Args) -> Command {
    let mut c = Command::new("systemctl");
    if a.user {
        c.arg("--user");
    }
    c.env("LC_ALL", "C");
    c
}
fn output(c: &mut Command) -> io::Result<String> {
    let o = c.output()?;
    if !o.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&o.stderr).to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into())
}
fn sample(a: &Args) -> io::Result<Vec<Unit>> {
    let mut c = cmd(a);
    c.args([
        "list-units",
        "--plain",
        "--no-legend",
        "--no-pager",
        "--full",
        "--type",
        &a.kind,
    ]);
    if a.all {
        c.arg("--all");
    }
    let list = output(&mut c)?;
    let names: Vec<_> = list
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    let mut units = vec![];
    for chunk in names.chunks(128) {
        let s=output(cmd(a).args(["show","--no-pager","--property=Id,Description,ActiveState,CPUUsageNSec,MemoryCurrent,MemoryMin,MemoryMax,TasksCurrent"]).args(chunk))?;
        units.extend(
            s.split("\n\n")
                .map(|b| {
                    b.lines()
                        .filter_map(|l| l.split_once('='))
                        .map(|(k, v)| (k.into(), v.into()))
                        .collect::<Unit>()
                })
                .filter(|u| u.contains_key("Id")),
        );
    }
    Ok(units)
}
fn human(n: Option<u64>, time: bool, raw: bool) -> String {
    let Some(n) = n else { return "—".into() };
    if raw {
        return n.to_string();
    }
    if time {
        let s = n / 1_000_000_000;
        return format!("{}h {:02}m {:02}s", s / 3600, s / 60 % 60, s % 60);
    }
    let mut n = n as f64;
    let mut i = 0;
    let labels = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    while n >= 1024. && i < 5 {
        n /= 1024.;
        i += 1
    }
    format!("{n:.1} {}", labels[i])
}
fn cells(u: &Unit, raw: bool) -> Vec<String> {
    vec![
        u["Id"].clone(),
        u.get("ActiveState").cloned().unwrap_or_default(),
        u.get("CPU%").cloned().unwrap_or("—".into()),
        human(num(u, "CPUUsageNSec"), true, raw),
        human(num(u, "MemoryCurrent"), false, raw),
        human(num(u, "MemoryMin"), false, raw),
        if num(u, "MemoryMax").is_none() {
            "∞".into()
        } else {
            human(num(u, "MemoryMax"), false, raw)
        },
        num(u, "TasksCurrent")
            .map(|n| n.to_string())
            .unwrap_or("—".into()),
    ]
}
fn sort(v: &mut [Unit], key: &str, reverse: bool) {
    let prop = match key {
        "memory" => "MemoryCurrent",
        "min" => "MemoryMin",
        "max" => "MemoryMax",
        "tasks" => "TasksCurrent",
        _ => "CPUUsageNSec",
    };
    v.sort_by(|a, b| {
        let order = if key == "name" {
            a["Id"].cmp(&b["Id"])
        } else if key == "cpu" {
            let p = |u: &Unit| {
                u.get("CPU%")
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(-1.)
            };
            p(b).total_cmp(&p(a))
        } else {
            num(b, prop).cmp(&num(a, prop))
        };
        if reverse { order.reverse() } else { order }
    })
}
fn main() -> io::Result<()> {
    let mut a = Args::parse();
    if !a.delay.is_finite() || a.delay < 0.1 || a.delay > 86400. {
        return Err(io::Error::other("delay must be 0.1–86400 seconds"));
    }
    if a.once {
        let mut v = sample(&a)?;
        sort(&mut v, &a.sort_key, false);
        println!("UNIT\tSTATE\tCPU%\tCPU TIME\tMEMORY\tMIN LIMIT\tMAX LIMIT\tTASKS");
        for u in v.iter().filter(|u| u["Id"].contains(&a.filter)) {
            println!("{}", cells(u, a.raw).join("\t"))
        }
        return Ok(());
    }
    let mut terminal = ratatui::init();
    let result = (|| -> io::Result<()> {
        let mut units = vec![];
        let mut old: HashMap<String, u64> = HashMap::new();
        let mut last = Instant::now();
        let mut next = Instant::now();
        let mut error = String::new();
        let mut paused = false;
        let mut editing = false;
        let mut help = false;
        let mut reverse = false;
        let mut state = TableState::default().with_selected(0);
        loop {
            if !paused && Instant::now() >= next {
                let now = Instant::now();
                match sample(&a) {
                    Ok(mut fresh) => {
                        for u in &mut fresh {
                            if let (Some(n), Some(p)) = (num(u, "CPUUsageNSec"), old.get(&u["Id"]))
                                && let Some(delta) = n.checked_sub(*p)
                            {
                                u.insert(
                                    "CPU%".into(),
                                    format!(
                                        "{:.1}",
                                        delta as f64 / 1e9 / now.duration_since(last).as_secs_f64()
                                            * 100.
                                    ),
                                );
                            }
                        }
                        old = fresh
                            .iter()
                            .filter_map(|u| num(u, "CPUUsageNSec").map(|n| (u["Id"].clone(), n)))
                            .collect();
                        units = fresh;
                        last = now;
                        error.clear()
                    }
                    Err(e) => error = e.to_string(),
                }
                next = Instant::now() + Duration::from_secs_f64(a.delay)
            }
            sort(&mut units, &a.sort_key, reverse);
            let shown: Vec<_> = units
                .iter()
                .filter(|u| u["Id"].to_lowercase().contains(&a.filter.to_lowercase()))
                .collect();
            let selected = state
                .selected()
                .unwrap_or(0)
                .min(shown.len().saturating_sub(1));
            state.select(Some(selected));
            terminal.draw(|f|{let areas=Layout::vertical([Constraint::Length(2),Constraint::Min(1),Constraint::Length(if help{6}else{2})]).split(f.area());f.render_widget(Paragraph::new(format!("systemd-top | {} units | sort {} | {}s {}\nFilter: {}{}",shown.len(),a.sort_key,a.delay,if paused{"PAUSED"}else{""},a.filter,if editing{"▏"}else{""})),areas[0]);let widths=[Constraint::Min(24),Constraint::Length(10),Constraint::Length(8),Constraint::Length(15),Constraint::Length(12),Constraint::Length(12),Constraint::Length(12),Constraint::Length(7)];f.render_stateful_widget(Table::new(shown.iter().map(|u|Row::new(cells(u,a.raw))),widths).header(Row::new(["UNIT","STATE","CPU%","CPU TIME","MEMORY","MIN LIMIT","MAX LIMIT","TASKS"])).row_highlight_style(Style::new().reversed()),areas[1],&mut state);f.render_widget(Paragraph::new(if !error.is_empty(){error.as_str()}else if help{"Arrows/PgUp/PgDn/Home/End navigate | / F3 F4 filter | Enter apply, Esc clear\nc CPU% | t CPU time | m memory | n name | [ min | ] max\nF6/s cycle sort | r reverse | Space pause | F5 refresh | F2 raw units\nF1/? help | F10/q quit | Ctrl-C quit\nMIN/MAX are configured limits, not historical extrema. ∞ unlimited; — unavailable.\nCPU%: 100% = one core. Missing metrics require systemd accounting."}else{"F1 Help  F2 Units  F3 Search  F4 Filter  F5 Refresh  F6 Sort  F10 Quit\nSpace Pause | r Reverse | c CPU% | t CPU time | m Memory"}),areas[2]);})?;
            if event::poll(Duration::from_millis(100))?
                && let Event::Key(e) = event::read()?
            {
                if e.kind != event::KeyEventKind::Press {
                    continue;
                }
                if e.modifiers.contains(event::KeyModifiers::CONTROL)
                    && e.code == KeyCode::Char('c')
                {
                    break;
                }
                if editing {
                    match e.code {
                        KeyCode::Esc => {
                            a.filter.clear();
                            editing = false
                        }
                        KeyCode::Enter => editing = false,
                        KeyCode::Backspace => {
                            a.filter.pop();
                        }
                        KeyCode::Char(c) => a.filter.push(c),
                        _ => {}
                    }
                    continue;
                }
                match e.code {
                    KeyCode::Char('q') | KeyCode::F(10) => break,
                    KeyCode::Char('/') | KeyCode::F(3) | KeyCode::F(4) => editing = true,
                    KeyCode::Char('?') | KeyCode::F(1) => help = !help,
                    KeyCode::F(2) => a.raw = !a.raw,
                    KeyCode::Char(' ') => paused = !paused,
                    KeyCode::F(5) => next = Instant::now(),
                    KeyCode::Char('r') => reverse = !reverse,
                    KeyCode::Char('c') => a.sort_key = "cpu".into(),
                    KeyCode::Char('t') => a.sort_key = "time".into(),
                    KeyCode::Char('m') => a.sort_key = "memory".into(),
                    KeyCode::Char('n') => a.sort_key = "name".into(),
                    KeyCode::Char('[') => a.sort_key = "min".into(),
                    KeyCode::Char(']') => a.sort_key = "max".into(),
                    KeyCode::F(6) | KeyCode::Char('s') => {
                        let keys = ["time", "cpu", "memory", "min", "max", "tasks", "name"];
                        a.sort_key = keys[(keys.iter().position(|k| *k == a.sort_key).unwrap_or(0)
                            + 1)
                            % keys.len()]
                        .into()
                    }
                    KeyCode::Down => state.select(Some(selected + 1)),
                    KeyCode::Up => state.select(Some(selected.saturating_sub(1))),
                    KeyCode::PageDown => state.select(Some(selected + 15)),
                    KeyCode::PageUp => state.select(Some(selected.saturating_sub(15))),
                    KeyCode::Home => state.select(Some(0)),
                    KeyCode::End => state.select(Some(shown.len().saturating_sub(1))),
                    _ => {}
                }
            }
        }
        Ok(())
    })();
    ratatui::restore();
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formatting() {
        assert_eq!(human(Some(1048576), false, false), "1.0 MiB");
        assert_eq!(human(Some(3661000000000), true, false), "1h 01m 01s");
        assert_eq!(human(None, false, false), "—");
    }
}
