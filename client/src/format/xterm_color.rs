#[inline]
fn color_code(fg_rgb: (u8, u8, u8), _bg_rgb: (u8, u8, u8), bold: bool, text: &str) -> String {
    let bold_code = if bold { "1;" } else { "" };
    format!(
        "\x1b[{}38;2;{};{};{}m{}\x1b[0m",
        bold_code, fg_rgb.0, fg_rgb.1, fg_rgb.2, text
    )
}

#[inline]
pub fn red(text: &str) -> String {
    color_code((255, 0, 0), (0, 0, 0), false, text)
}

#[inline]
pub fn bold_red(text: &str) -> String {
    color_code((255, 0, 0), (0, 0, 0), true, text)
}

#[inline]
pub fn green(text: &str) -> String {
    color_code((0, 255, 0), (0, 0, 0), false, text)
}

#[inline]
pub fn bold_green(text: &str) -> String {
    color_code((0, 255, 0), (0, 0, 0), true, text)
}

#[inline]
pub fn yellow(text: &str) -> String {
    color_code((255, 255, 0), (0, 0, 0), false, text)
}

#[inline]
pub fn bold_yellow(text: &str) -> String {
    color_code((255, 255, 0), (0, 0, 0), true, text)
}

#[inline]
pub fn blue(text: &str) -> String {
    color_code((0, 0, 255), (0, 0, 0), false, text)
}

#[inline]
pub fn bold(text: &str) -> String {
    format!("\x1b[1m{}\x1b[0m", text)
}
