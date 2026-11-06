//! Wolfgang's pixel portrait for the terminal, drawn with half blocks
//! (two pixels per character cell). Milestone 5 will replace the grid with
//! the shared sprite source used by the app.

use crossterm::style::{Color, Stylize};
use wolfgang_dialogue::Mood;

/// 16×16 pixels. `.` is transparent.
const BASE: [&str; 16] = [
    "....hHHHHHHh....",
    "...hHHHHHHHHh...",
    "..hHHHHHHHHHHh..",
    "..rrrrrrrrrrrr..",
    "..sSSSSSSSSSSs..",
    "..sGGGGSGGGGSs..",
    "..sGLEGGGELGSs..",
    "..sGGGGSGGGGSs..",
    "..sSSSSssSSSSs..",
    "..sBSSSSSSSSBs..",
    "..BBBbMMMMbBBB..",
    "..BBBBbbbbBBBB..",
    "...BBBBBBBBBB...",
    "..cCCBBBBBBCCc..",
    ".cCCCCTTTTCCCCc.",
    ".cCCKCTTTTCKCCc.",
];

/// Mouth rows (10 and 11) per mood.
fn mouth(mood: Mood) -> [&'static str; 2] {
    match mood {
        Mood::Happy | Mood::Proud => ["..BBBMbbbbMBBB..", "..BBBBMMMMBBBB.."],
        Mood::Surprised => ["..BBBbbMMbbBBB..", "..BBBBbMMbBBBB.."],
        Mood::Sorry => ["..BBBbbbbbbBBB..", "..BBBMMMMMMBBB.."],
        Mood::Thinking => ["..BBBbbbMMMBBB..", "..BBBBbbbbBBBB.."],
        Mood::Neutral => [BASE[10], BASE[11]],
    }
}

fn rgb(c: char) -> Option<(u8, u8, u8)> {
    Some(match c {
        'H' => (0xc0, 0x66, 0x3a), // beanie
        'h' => (0x9a, 0x4f, 0x2c),
        'r' => (0xd9, 0x8a, 0x5a),
        'S' => (0xf0, 0xc3, 0x9b), // skin
        's' => (0xd9, 0xa4, 0x7f),
        'G' => (0x3b, 0x2f, 0x2a), // glasses
        'E' => (0x2a, 0x24, 0x20),
        'L' => (0xdf, 0xe8, 0xec), // lens
        'B' => (0x7a, 0x52, 0x36), // beard
        'b' => (0x95, 0x68, 0x3f),
        'M' => (0x5a, 0x34, 0x24),
        'C' => (0x5b, 0x7a, 0x62), // cardigan
        'c' => (0x46, 0x60, 0x4c),
        'K' => (0xe8, 0xd9, 0xb0),
        'T' => (0xef, 0xe7, 0xd6), // shirt
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
