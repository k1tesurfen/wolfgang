# Wolfgang's pixel art

**Everything in `src/` is made by hand and edited directly.** Everything else here is generated from it.

| Source (`src/`) | What | Rules |
|---|---|---|
| `office.png` | the 320×180 scene background | exactly 320×180 |
| `portraits/*.png` | the eight 96×96 mood portraits (happy, thinking, surprised, content, laughing, sideeye, dreamy, sorry) | exactly 96×96, transparent background |
| `wolfgang.sprite` | the animated desk sprite, one character per pixel | format described at the top of the file |
| `ui.sprite` | speech bubble, answers, panel, portrait frame, cursor, sparkle | same format |

Edit the PNGs in any pixel editor (Aseprite, Pixelorama, Photoshop with nearest-neighbour…).
Keep pixels fully opaque or fully transparent and avoid smoothing; the build warns about
half-transparent pixels and about suspiciously many colours.

## Build

```bash
cargo run --release -p wolfgang-art -- build
open target/wolfgang-art/preview.html     # the scene, animated and clickable
```

`build` checks the sources and writes `wolfgang.png`, `ui.png` (sprite sheets) and `scene.json`
(frames, animations, mood → portrait, clickable hotspots, bubble area). It never touches `src/`.

## Where the PNG sources came from

`office.png` and the portraits were converted once from the reference images in
`assets/reference/` (AI images in pixel-art *style*) and then touched up by hand:

```bash
cargo run --release -p wolfgang-art -- import           # refuses to overwrite existing sources
cargo run --release -p wolfgang-art -- import --force   # overwrites them: hand edits are lost!
```

The conversion uses a k-means palette in OKLab, then gives each target pixel the most frequent
palette colour of its block, plus a pass that keeps thin dark outlines continuous.
You only need `import` again for new reference images.
