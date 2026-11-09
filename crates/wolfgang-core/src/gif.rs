//! Just enough of the GIF format to tell animated GIFs (→ videos) from still
//! ones (→ photos), and whether they have transparent areas. Nothing is decoded.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GifInfo {
    pub frames: usize,
    /// Any frame declares a transparent colour.
    pub transparent: bool,
}

impl GifInfo {
    pub fn animated(&self) -> bool {
        self.frames > 1
    }
}

/// Walks the block structure. `None` if it isn't a readable GIF.
pub fn info(data: &[u8]) -> Option<GifInfo> {
    if !(data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a")) {
        return None;
    }
    let mut p = 13; // header + logical screen descriptor
    let flags = *data.get(10)?;
    if flags & 0x80 != 0 {
        p += 3 << ((flags & 7) + 1); // global colour table
    }
    let mut out = GifInfo { frames: 0, transparent: false };
    loop {
        match *data.get(p)? {
            0x21 => {
                // Extension; a graphic control extension carries the transparency flag.
                let label = *data.get(p + 1)?;
                if label == 0xF9 && *data.get(p + 2)? == 4 && data.get(p + 3)? & 1 != 0 {
                    out.transparent = true;
                }
                p = skip_sub_blocks(data, p + 2)?;
            }
            0x2C => {
                out.frames += 1;
                let flags = *data.get(p + 9)?;
                p += 10;
                if flags & 0x80 != 0 {
                    p += 3 << ((flags & 7) + 1); // local colour table
                }
                p = skip_sub_blocks(data, p + 1)?; // after the LZW code size
            }
            0x3B => return Some(out),
            _ => return (out.frames > 0).then_some(out), // trailing garbage: keep what we have
        }
    }
}

fn skip_sub_blocks(data: &[u8], mut p: usize) -> Option<usize> {
    loop {
        let len = *data.get(p)? as usize;
        p += 1 + len;
        if len == 0 {
            return Some(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal GIF: `frames` 1×1 images, optionally with a transparent colour.
    fn gif(frames: usize, transparent: bool) -> Vec<u8> {
        let mut g = b"GIF89a".to_vec();
        g.extend([1, 0, 1, 0, 0x80, 0, 0]); // 1×1, global table with 2 colours
        g.extend([0, 0, 0, 255, 255, 255]);
        for _ in 0..frames {
            g.extend([0x21, 0xF9, 4, u8::from(transparent), 10, 0, 0, 0]);
            g.extend([0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0]);
            g.extend([2, 2, 0x44, 0x01, 0]); // LZW data
        }
        g.push(0x3B);
        g
    }

    #[test]
    fn counts_frames_and_transparency() {
        assert_eq!(info(&gif(1, false)), Some(GifInfo { frames: 1, transparent: false }));
        let anim = info(&gif(3, true)).unwrap();
        assert!(anim.animated() && anim.transparent);
        assert_eq!(info(b"not a gif"), None);
        let mut cut = gif(2, false);
        cut.truncate(cut.len() - 6);
        assert_eq!(info(&cut), None, "a truncated file isn't trusted");
    }
}
