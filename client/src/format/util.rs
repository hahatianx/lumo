pub fn readable_format(date: std::time::SystemTime) -> String {
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
