//! Wolfgang's pixel portrait for the terminal, drawn with half blocks
//! (two pixels per character cell). Milestone 5 will replace the grid with
//! the shared sprite source used by the app.

use crossterm::style::{Color, Stylize};
use wolfgang_dialogue::Mood;

/// 16×16 pixels, after the reference studies: bun, clear glasses, beard,
/// mustard/green striped cardigan. `.` is transparent.
const BASE: [&str; 16] = [
    "..........HH....",
    "....HHHHHHHHH...",
    "...HHiiHHHHHHh..",
    "...HHHHHHHHHHh..",
    "...HSSSSSSSSH...",
    "...GGGGSSGGGG...",
    "...GSEGSSGSEG...",
    "...GGGGSSGGGGE..",
    "...SSSSssSSSS...",
    "..BSBBBBBBBBSB..",
    "..BBBBMMMMBBBB..",
    "..BBBBBBBBBBBB..",
    "...BBBgBBgBBB...",
    ".YYVBBBBBBBBVYY.",
    "YYYVVTTTTTTVVYYY",
    "VVVVVTTTTTTVVVVV",
];

/// Mouth rows (10 and 11) per mood.
fn mouth(mood: Mood) -> [&'static str; 2] {
    match mood {
        Mood::Happy | Mood::Proud => ["..BBBMBBBBMBBB..", "..BBBBMMMMBBBB.."],
        Mood::Surprised => ["..BBBBBMMBBBBB..", "..BBBBBMMBBBBB.."],
        Mood::Sorry => ["..BBBBBBBBBBBB..", "..BBBMMMMMMBBB.."],
        Mood::Thinking => ["..BBBBBBMMMBBB..", "..BBBBBBBBBBBB.."],
        Mood::Neutral => [BASE[10], BASE[11]],
    }
}

/// Colours taken from the app's portraits.
fn rgb(c: char) -> Option<(u8, u8, u8)> {
    Some(match c {
        'H' => (0x52, 0x3c, 0x2d), // hair
        'h' => (0x3d, 0x27, 0x22),
        'i' => (0x69, 0x4f, 0x40),
        'S' => (0xe6, 0x9f, 0x5e), // skin
        's' => (0xd5, 0x75, 0x4c),
        'G' => (0xbd, 0xa7, 0x99), // clear glasses
        'E' => (0x1c, 0x0c, 0x16), // eyes, ear plug
        'B' => (0x5f, 0x44, 0x2e), // beard
        'g' => (0x86, 0x67, 0x54),
        'M' => (0x5c, 0x2b, 0x29),
        'Y' => (0xb4, 0x79, 0x2b), // cardigan mustard
        'V' => (0x48, 0x4f, 0x33), // cardigan green
        'T' => (0x1f, 0x22, 0x2d), // tee
        _ => return None,
    })
}

/// Full 24-bit colour, or the nearest of the 256 xterm colours.
fn color((r, g, b): (u8, u8, u8), truecolor: bool) -> Color {
    if truecolor {
        return Color::Rgb { r, g, b };
    }
    let step = |v: u8| if v < 48 { 0 } else if v < 115 { 1 } else { (v as u16 - 35) as u8 / 40 };
    Color::AnsiValue(16 + 36 * step(r) + 6 * step(g) + step(b))
}

pub fn truecolor() -> bool {
    std::env::var("COLORTERM").is_ok_and(|v| v.contains("truecolor") || v.contains("24bit"))
}

/// The portrait as 8 lines of 16 columns.
pub fn portrait(mood: Mood, truecolor: bool) -> Vec<String> {
    let mut grid: Vec<&str> = BASE.to_vec();
    let [m1, m2] = mouth(mood);
    grid[10] = m1;
    grid[11] = m2;
    grid.chunks(2)
        .map(|pair| {
            pair[0]
                .chars()
                .zip(pair[1].chars())
                .map(|(top, bottom)| match (rgb(top), rgb(bottom)) {
                    (None, None) => " ".to_string(),
                    (Some(t), None) => "▀".with(color(t, truecolor)).to_string(),
                    (None, Some(b)) => "▄".with(color(b, truecolor)).to_string(),
                    (Some(t), Some(b)) => "▀".with(color(t, truecolor)).on(color(b, truecolor)).to_string(),
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_is_square_and_fully_coloured() {
        for row in BASE.iter().chain(mouth(Mood::Happy).iter()).chain(mouth(Mood::Sorry).iter()) {
            assert_eq!(row.chars().count(), 16, "{row}");
            assert!(row.chars().all(|c| c == '.' || rgb(c).is_some()), "{row}");
        }
        assert_eq!(portrait(Mood::Neutral, true).len(), 8);
    }

    #[test]
    fn xterm_256_mapping() {
        assert_eq!(color((0, 0, 0), false), Color::AnsiValue(16));
        assert_eq!(color((255, 255, 255), false), Color::AnsiValue(231));
    }
}
