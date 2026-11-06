# Wolfgang's pixel art

Everything here except `src/` is generated. Rebuild with:

```bash
cargo run --release -p wolfgang-art -- build
open target/wolfgang-art/preview.html     # the scene, animated and clickable
```

| File | From | What |
|---|---|---|
| `office.png` | `assets/reference/office.jpeg` | 320×180 scene background, 48 colours |
| `portraits/*.png` | `assets/reference/portrait-*.jpeg` | 96×96 mood portraits, shared 40-colour palette, transparent background |
| `wolfgang.png` | `src/wolfgang.sprite` | animated desk sprite (idle, talk, type, sip, hand …) |
| `ui.png` | `src/ui.sprite` | speech bubble, tail, answer bubbles, panel, cursor, sparkle |
| `scene.json` | the build tool | frame rectangles, animations, mood → portrait, clickable hotspots, bubble area |

The references are AI images in pixel-art *style*. The tool turns them into real pixel art:
a k-means palette in OKLab, then each target pixel takes the most frequent palette colour
of its block, plus a pass that keeps thin dark outlines continuous.

`src/*.sprite` are plain text, one character per pixel, editable by hand. The format is described at the top of `wolfgang.sprite`.
