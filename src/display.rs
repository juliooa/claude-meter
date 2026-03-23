use comfy_table::{presets::UTF8_FULL, Table};

use crate::db;

pub fn show_summary(usage: &db::UsageSummary, label: &str) {
    println!("{}", label);
    println!("{}", "─".repeat(40));
    println!("{:<28} {:>10}", "Tokens (input)", fmt_num(usage.input_tokens));
    println!(
        "{:<28} {:>10}",
        "Tokens (output)",
        fmt_num(usage.output_tokens)
    );
    println!(
        "{:<28} {:>10}",
        "Tokens (cache read)",
        fmt_num(usage.cache_read_tokens)
    );
    println!(
        "{:<28} {:>10}",
        "Tokens (cache creation)",
        fmt_num(usage.cache_creation_tokens)
    );
    println!("{:<28} {:>10}", "Cost", fmt_cost(usage.cost));
    println!("{:<28} {:>10}", "Sessions", usage.sessions);
    println!(
        "{:<28} {:>10}",
        "Lines added",
        fmt_num(usage.lines_added)
    );
    println!(
        "{:<28} {:>10}",
        "Lines removed",
        fmt_num(usage.lines_removed)
    );
    println!(
        "{:<28} {:>10}",
        "Active time",
        fmt_duration(usage.active_time_secs)
    );
}

pub fn show_history(rows: &[db::DaySummary]) {
    if rows.is_empty() {
        println!("No data for the selected period.");
        return;
    }

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec![
        "Date",
        "Input",
        "Output",
        "Cache Read",
        "Cost",
        "Sessions",
    ]);

    for row in rows {
        table.add_row(vec![
            row.date.clone(),
            fmt_num(row.input_tokens),
            fmt_num(row.output_tokens),
            fmt_num(row.cache_read_tokens),
            fmt_cost(row.cost),
            row.sessions.to_string(),
        ]);
    }

    println!("{table}");
}

pub fn show_by_model(rows: &[db::ModelSummary]) {
    if rows.is_empty() {
        println!("No model data recorded.");
        return;
    }

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec!["Model", "Input", "Output", "Cost"]);

    for row in rows {
        table.add_row(vec![
            row.model.clone(),
            fmt_num(row.input_tokens),
            fmt_num(row.output_tokens),
            fmt_cost(row.cost),
        ]);
    }

    println!("{table}");
}

pub fn show_by_session(rows: &[db::SessionSummary]) {
    if rows.is_empty() {
        println!("No session data recorded.");
        return;
    }

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec!["Session", "Date", "Tokens", "Cost", "Duration"]);

    for row in rows {
        let short_id = if row.session_id.len() > 8 {
            format!("{}..", &row.session_id[..8])
        } else {
            row.session_id.clone()
        };
        table.add_row(vec![
            short_id,
            row.date.clone(),
            fmt_num(row.total_tokens),
            fmt_cost(row.cost),
            fmt_duration(row.duration_secs as f64),
        ]);
    }

    println!("{table}");
}

fn fmt_num(n: f64) -> String {
    let n = n as i64;
    if n == 0 {
        return "0".to_string();
    }
    let negative = n < 0;
    let mut s: Vec<char> = n.unsigned_abs().to_string().chars().collect();
    let mut i = s.len() as isize - 3;
    while i > 0 {
        s.insert(i as usize, ',');
        i -= 3;
    }
    let result: String = s.into_iter().collect();
    if negative {
        format!("-{}", result)
    } else {
        result
    }
}

fn fmt_cost(c: f64) -> String {
    format!("${:.2}", c)
}

fn fmt_duration(secs: f64) -> String {
    let total = secs as u64;
    if total == 0 {
        return "0s".to_string();
    }
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{}h {}m {}s", h, m, s)
    } else if m > 0 {
        format!("{}m {}s", m, s)
    } else {
        format!("{}s", s)
    }
}
