pub fn system_time_to_human_readable(date: std::time::SystemTime) -> String {
    let last_seen_time: chrono::DateTime<chrono::Local> = date.into();
    last_seen_time.format("%Y-%m-%d %H:%M:%S %Z").to_string()
}

pub fn str_safe_truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}...", &s[0..max_len - 3])
    } else {
        s.to_string()
    }
}

pub fn u64_to_human_readable(n: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    match n {
        0..KB => format!("{} B", n),
        KB..MB => format!("{:.2} KB", n as f64 / KB as f64),
        MB..GB => format!("{:.2} MB", n as f64 / MB as f64),
        GB..TB => format!("{:.2} GB", n as f64 / GB as f64),
        _ => format!("{:.2} TB", n as f64 / TB as f64),
    }
}
