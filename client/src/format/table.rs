use crate::format::xterm_color;
use std::collections::HashMap;

pub struct TableColumn {
    pub(crate) idx: usize,
    pub(crate) name: &'static str,
}

impl TableColumn {
    pub fn new(idx: usize, name: &'static str) -> Self {
        Self { idx, name }
    }
}

pub trait Schema<const N: usize> {
    fn names() -> [&'static TableColumn; N];
}

pub trait TableEntry<const N: usize, S: Schema<N>> {
    fn fmt(&self) -> HashMap<usize, String>;
}

pub(crate) struct TableFormatter<'a, const N: usize, S: Schema<N>> {
    phantom_entry: std::marker::PhantomData<&'a dyn TableEntry<N, S>>,
}

impl<'a, const N: usize, S> TableFormatter<'a, N, S>
where
    S: Schema<N>,
{
    pub fn new() -> Self {
        Self {
            phantom_entry: std::marker::PhantomData,
        }
    }

    fn format_data(&self, data: &'a [impl TableEntry<N, S>]) -> Vec<Vec<String>> {
        data.iter()
            .map(|entry| {
                let mut row = entry.fmt();
                // Collect keys and sort them ascending
                let mut keys: Vec<usize> = row.keys().copied().collect();
                keys.sort_unstable();
                let values = keys
                    .into_iter()
                    .map(|k| row.get(&k).cloned().unwrap_or_default())
                    .collect::<Vec<String>>();
                values
            })
            .collect::<Vec<_>>()
    }

    #[inline]
    fn visible_len(text: &str) -> usize {
        // Count visible characters without allocating: skip ANSI escape sequences.
        // We handle CSI sequences of the form "\x1b[ ... m" which our colorizer emits.
        let mut count = 0usize;
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                // Skip optional '[' and parameters until and including the terminating 'm'.
                if matches!(chars.peek(), Some('[')) {
                    let _ = chars.next();
                }
                while let Some(nc) = chars.next() {
                    if nc == 'm' {
                        break;
                    }
                }
                continue;
            }
            count += 1;
        }
        count
    }

    #[inline]
    fn push_char_n(buf: &mut String, ch: char, n: usize) {
        if n == 0 {
            return;
        }
        buf.reserve(n);
        for _ in 0..n {
            buf.push(ch);
        }
    }

    #[inline]
    fn push_padded(result: &mut String, text: &str, width: usize) {
        result.push_str(text);
        let vlen = Self::visible_len(text);
        if width > vlen {
            Self::push_char_n(result, ' ', width - vlen);
        }
    }

    pub fn fmt<E>(&self, data: &'a [E]) -> String
    where
        E: TableEntry<N, S>,
    {
        // Calculate max width for each column using visible width (ignore ANSI color codes)
        let mut widths = S::names()
            .iter()
            .map(|h| h.name.chars().count())
            .collect::<Vec<_>>();
        let rows = self.format_data(data);

        for row in rows.iter() {
            for (i, cell) in row.iter().enumerate() {
                if i < widths.len() {
                    widths[i] = widths[i].max(Self::visible_len(cell));
                }
            }
        }

        let mut result = String::new();
        // Format header
        for (i, header) in S::names().iter().enumerate() {
            Self::push_padded(&mut result, &xterm_color::bold(header.name), widths[i]);
            if i < S::names().len() - 1 {
                result += " | ";
            }
        }
        result += "\n";

        // Format separator
        for (i, &width) in widths.iter().enumerate() {
            result += &"-".repeat(width);
            if i < widths.len() - 1 {
                result += "-+-";
            }
        }
        result += "\n";

        // Format rows
        for row in rows {
            for (i, cell) in row.iter().enumerate() {
                if i < widths.len() {
                    Self::push_padded(&mut result, cell, widths[i]);
                    if i < row.len() - 1 {
                        result += " | ";
                    }
                }
            }
            result += "\n";
        }
        result
    }
}

pub fn format_table<'a, const N: usize, S, E>(f: &'a TableFormatter<N, S>, items: &'a [E]) -> String
where
    S: Schema<N>,
    E: TableEntry<N, S>,
{
    let formatted_table = f.fmt(items);
    formatted_table
}
