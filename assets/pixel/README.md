# The pixel art of Wolfgang, Ully and Burgunde

## The exchange folder: `characters/`

One folder per character. **Take a PNG out, paint it, put it back** (same name, same size):
the next build (`scripts/build-app.sh <key>`, or `wolfgang-art build`) uses it.

```
characters/
  wolfgang/  portraits/happy.png … sorry.png   8 moods, 96×96, transparent background
             desk.png                          the animated desk sprite: 8 frames side by side
             office.png          (optional)    an own office, 320×180; otherwise src/office.png
  ully/      … the same
  burgunde/  … the same
```

| File | What happens to it |
|---|---|
| `portraits/<mood>.png` | used directly. Moods: happy, thinking, surprised, content, laughing, sideeye, dreamy, sorry |
| `desk.png` | turned into `desk.sprite` (text) by the build, because the animations are written in text. Frames left to right: idle, blink, talk, smile, typeA, typeB, sip, hand, each 46×56 with a 1 px gap after it. A colour that is new gets added to the palette |
| `desk.sprite` | the same sprite as text (one letter per pixel, palette on top). Edit this *or* the PNG: the build carries whichever changed over to the other |
| `office.png` | used directly, if it's there |
| `palette.hex` | the character's portrait colours, one per line. Load it into your editor (Aseprite, Pixelorama, Lospec) to paint with exactly these colours |
| `.desk-sync` | the build's note of what `desk.png` and `desk.sprite` looked like last time; leave it alone |

**Colour clean-up.** Some editors and websites (Pixilart, for one) shift colours by a few steps
when they save, so one colour becomes thousands of nearly identical ones. The build snaps them back:
every portrait pixel within 12 steps (of 255, per R/G/B) of a `palette.hex` colour becomes its
*nearest* palette colour, and the PNG is rewritten. A colour further away is a new one you painted:
it stays and is added to `palette.hex`. `desk.png` is cleaned the same way against `desk.sprite`'s
colours. Half-transparent pixels become fully opaque or fully transparent.

Missing pieces are fine while a character is in the making: a missing mood shows a related one of
the same character (laughing → happy, dreamy → thinking, …), and without any portraits, desk or
office, Wolfgang's stand in. The build lists what each character has.

A new character's desk sprite: copy `wolfgang/desk.png` into their folder and paint over it.
Without a `desk.sprite` the build takes Wolfgang's frame layout and animations and starts a
fresh palette from the painting.

If `desk.png` and `desk.sprite` **both** changed since the last build, it stops and asks you to pick:

```bash
cargo run --release -p wolfgang-art -- keep-png ully    # the painting wins, the text is rewritten
cargo run --release -p wolfgang-art -- keep-text ully   # the text wins, the PNG is rewritten
```

Paint in any pixel editor (Aseprite, Pixelorama, Photoshop with nearest-neighbour …). Keep
pixels fully opaque or fully transparent and avoid smoothing: the build warns about
half-transparent pixels and suspiciously many colours.

## Shared art: `src/`

| Source (`src/`) | What | Rules |
|---|---|---|
| `office.png` | the 320×180 office, for every character without their own | exactly 320×180 |
| `ui.sprite` | speech bubble, answers, panel, portrait frame, cursor, sparkle | same text format as `desk.sprite` |
| `wilma.sprite` | Wilma the office cat and her cushion; every frame has its own size, drawn with its bottom centre on her spot | same format; her routine is in the app (`app.js`) |

The two sprites can be painted too:

```bash
cargo run --release -p wolfgang-art -- to-png wilma     # → assets/pixel/edit/wilma.png
# paint, save
cargo run --release -p wolfgang-art -- from-png wilma   # → back into src/wilma.sprite
```

`edit/` isn't committed; `to-png` refuses to overwrite a sheet with painting you haven't brought
back yet (`--force` throws it away).

## Build

```bash
cargo run --release -p wolfgang-art -- build
open target/wolfgang-art/preview.html            # Wolfgang's scene, animated and clickable
open target/wolfgang-art/preview-ully.html       # … Ully's, preview-burgunde.html Burgunde's
```

`build` checks the sources, syncs every `desk.png` with its `desk.sprite`, and writes `ui.png`,
`wilma.png` and `scene.json` (per character: office, desk frames and animations, mood → portrait;
shared: clickable hotspots, bubble area). The app picks its own character from `scene.json`.

## Where the portraits came from

They were converted from AI images in pixel-art *style* (`assets/reference/`) and then touched
up by hand. Ully's and Burgunde's references are named by mood (`assets/reference/ully/happy.jpeg`);
a new one becomes a portrait with

```bash
cargo run --release -p wolfgang-art -- import-character burgunde           # only moods without a portrait
cargo run --release -p wolfgang-art -- import-character burgunde --force   # all again: hand edits are lost!
```

It finds the grid of "pixels" Gemini drew and reads every one from its centre (cell edges are where
JPEG halos and soft borders sit). Where Gemini drew finer than a portrait pixel (Burgunde: 2×2), the
majority colour wins, a tie goes to the colour that stands out (lines survive), and small distinct
features that would vanish (a pupil, an earring) get exactly one pixel back, so dark never spreads
into light. Colours close to ones the character's portraits already use become those, so the palette
only grows by really new colours. Outlines it doesn't see clearly stay open: drawing them is quicker
than removing wrong ones.

To compare the old and the new conversion on any reference: `wolfgang-art compare OUT_DIR IMAGE...`.

Wolfgang's office and portraits came from the older block-based conversion (`import`, refuses to
overwrite without `--force`).
