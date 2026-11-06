# Wolfgang 2.0 — Plan

> Wolfgang is the personification of conversion: a cozy, bearded guy in a cardigan who sits behind his desk with a coffee and converts whatever you put in front of him.
> Usability for non-technical colleagues is the top priority.

Status: **approved**. Milestones 1–5 (spike, core engine, dialogue, CLI, pixel art) are done.

---

## 1. Goals of iteration 1

1. Rebuild Wolfgang as a **Rust engine** shared by a **CLI** and a **macOS desktop app (Tauri)**.
2. **Image conversion** in two modes: *„Hochwertig behalten“* (keep high quality) and *„Fürs Web vorbereiten“* (prepare for web).
3. **Font conversion** to WOFF2 (complete fonts, no subsetting) with filename normalization.
4. The configuration is a **conversation with Wolfgang**: game-style speech bubbles in a pixel-art office (app) and the same dialogue in the terminal (CLI).
5. **Zero setup** for users: all codecs are bundled, no Homebrew or ImageMagick needed.

### Out of scope for v2.0 (roadmap, but the architecture must allow it)

- Audio (WAV/AIFF → MP3/AAC/Opus)
- Video (MP4/WebM, poster frames), which will mean bundling ffmpeg
- PDF image extraction
- V3/V4: speech-to-text transcription of audio/video via Python + a local model (spoken word only)
- English UI, Intel/Linux builds, code signing, auto-update

---

## 2. Decisions so far

| Topic | Decision |
|---|---|
| Stack | Rust core + Tauri app (web frontend for the pixel-art UI) |
| Codecs | Everything self-contained: Apple ImageIO (system) for decoding + statically linked encoders. No runtime dependencies. |
| Platform | macOS Apple Silicon (arm64) only |
| Distribution | Unsigned `.dmg` via GitHub Releases; the app offers to install the `wolfgang` CLI. Docs explain right-click → Öffnen on first launch. |
| External tools | Allowed: colleagues' Macs can get Homebrew tools (e.g. `ffmpeg`) installed. Images/fonts stay dependency-free because that already works; **video/audio will use an installed `ffmpeg`** rather than bundling it. Wolfgang checks for missing tools and explains how to install them. |
| Fonts | Converted 1:1 to WOFF2, **no subsetting** (decided after the spike, to avoid broken fonts) |
| Language | German only. Strings still live in a separate dialogue/strings file so other languages can be added later. |
| Voice | Gemütlich, uses *du*, light slang, warm, a few running gags, never annoying |
| Look | Defined by the reference studies in `pixel-art/` (2026-11-07): man bun, clear round glasses, full grey-brown beard, ear plugs, mustard/green striped shawl cardigan over a plaid shirt and dark tee, rainbow lanyard, pins, coffee mug |
| Art | Reference images by the user (AI-generated, pixel-art *style*), turned into real pixel art by a pixelize tool (grid snap + fixed palette). The animated desk sprite is drawn in code to match the references. |
| Migration | Clean break as **v2.0**. Old Bash script moves to `legacy/`. Old run logs are ignored. `wolfgang legacy-reset` can still undo the old in-place renames. |

---

## 3. Conversion behaviour

### 3.1 Image inputs

`jpg, jpeg, png, webp, heic, heif, gif (first frame), tif, tiff, avif` and Camera RAW `cr2, cr3, nef, arw, dng` (developed by Apple's RAW engine via ImageIO; colours match Preview/Photos, not Lightroom).

Files are numbered in **Finder order** (case-insensitive, `IMG_2` before `IMG_10`), so `0001` is the first file the user sees.

### 3.2 Mode „Hochwertig behalten“ (keep high quality)

- Format change only: **original dimensions**, never resized
- Output: any of **JPG, WebP, AVIF, PNG** (user picks)
- Near-lossless: JPG/WebP/AVIF at very high quality (~95), PNG lossless; alpha kept where the format supports it
- Applies EXIF orientation. No SEO renaming: original filenames are kept (`Foto.jpg` + `Foto.png` → `Foto.webp` + `Foto-png.webp`).
- **Colour profile kept** for JPG/WebP/PNG (wide-gamut iPhone photos stay Display P3). **AVIF is converted to sRGB**, because the encoder can only write sRGB. CMYK/grayscale sources become sRGB.
- Other metadata (EXIF, GPS) is dropped. Orientation is baked into the pixels.
- Output is 8 bits per channel, including from 16-bit TIFF/RAW sources.

### 3.3 Mode „Fürs Web vorbereiten“ (prepare for web)

- Output: **WebP or AVIF only**. The default is a user setting (initially WebP).
- **One size** per image: longest side = N px (default 1400), never upscaled
- Quality configurable (default ~80 equivalent, tuned per codec)
- Applies EXIF rotation, converts to **sRGB**, then **strips all metadata** (including GPS)
- **SEO naming** (web mode only): `[PREFIX]-[KEYWORDS]-[ORIGINAL?]-[INDEX].ext`
  - Separator styles `dash`, `underscore`, `caps`, as today
  - Keywords: a `*.txt` in each folder is used as today, and Wolfgang mentions that he found it. If none is found, he asks „Worum geht's auf den Bildern?“, either per folder or once for all.
  - **Originals are never touched.** The index is tracked by file hash in the run log.

### 3.4 Fonts → WOFF2

- Inputs: `ttf, otf, woff` → `woff2`
- **No subsetting.** The font is converted 1:1 and every glyph, feature and table is kept. Fonts don't follow standards reliably enough, and stripping glyphs risks shipping broken fonts.
- **Safety net:** after encoding, Wolfgang decodes the WOFF2 again and compares it table by table with the original. If anything differs, he re-encodes without the WOFF2 glyf transform (`encode_no_transform`), which is slightly bigger but byte-faithful.
- **Filename normalization** from font metadata: `Inter Bold Italic.otf` → `inter-700-italic.woff2`, variable fonts → `inter-variable.woff2`
- Not supported in v2.0: font collections (`.ttc`). Wolfgang says so instead of guessing.
- No CSS generation in v2.0

### 3.5 Mixed drops

Wolfgang sorts what was dropped into piles: „Ich hab 42 Fotos, 6 Schriften und 3 Sachen, mit denen ich nix anfangen kann.“ He asks the image questions (fonts need none) and then does everything in one run. Files with identical content are converted once. Unknown files are listed and skipped.

### 3.6 Output location & layout

- A single dropped folder → `<folder>-wolfgang/` next to it, mirroring the folder's contents.
- Anything else (single files, several items) → `<parent>-wolfgang/` inside the first item's parent folder, mirroring the dropped items by name.
- If that folder exists but belongs to a different source (or isn't Wolfgang's), `-2`, `-3`, … are used. Wolfgang never writes into a foreign folder.
- SEO names are lowercase for dash/underscore styles.
- Mirrors the input's subfolder structure. Files of all formats sit side by side; fonts go under `fonts/` with the same mirroring.
- Run log `.wolfgang/run.jsonl` inside the output folder holds settings, file hashes and indexes.

### 3.7 Re-runs

When the same input is dropped again: „Kenn ich schon! 10 sind neu. Nur die neuen oder alles nochmal?“ New files continue the index (0043, 0044 …) in the existing output folder.

---

## 4. The conversation

### 4.1 Shared dialogue engine

- The dialogue is **data, not code**: a dialogue file (e.g. `dialogue/de.yaml`) defines nodes, Wolfgang's lines (with variants so he doesn't repeat himself), answers, conditions and the setting each answer writes.
- The Rust core holds the **dialogue state machine**. The app and the CLI only render it. The two can never drift apart.
- Every question has a **typed custom answer** (number with range, text, …) that is validated locally. Wolfgang reacts in character to invalid input („2 Millionen Pixel? Da platzt mir der Monitor.“).
- Offline only, no LLM in v2.0.

### 4.2 Flow (images, web mode)

1. *Drop* → Wolfgang looks at the pile and summarises it
2. „So wie letztes Mal?“ (summary of the last run) / named presets / „Lass uns das neu machen“
3. Mode: „Hochwertig behalten“ / „Fürs Web vorbereiten“
4. Format: WebP / AVIF (default marked)
5. Size: Klein (800) / Normal (1400) / Groß (2400) / custom px
6. Quality: Kleiner / Ausgewogen / Schöner / custom 1–100
7. Naming: prefix (text), keywords (found `.txt` or ask), separator, append original name
8. Re-run question if applicable
9. Confirmation summary → „Los geht's!“ / „Nochmal ändern“
10. Optional: „Soll ich mir das als Vorlage merken?“ → name the preset

HQ mode asks only for the format (JPG/WebP/AVIF/PNG). Fonts need no questions: Wolfgang just says what he'll do („Die 6 Schriften mach ich dir zu WOFF2, ganz ohne Rumschnippeln.“).

### 4.3 Memory

- „So wie letztes Mal“ is always the first answer (when there is a last run)
- **Named presets** can be saved after a run and are offered as answers
- Stored in `~/Library/Application Support/Wolfgang/settings.json`, shared by the CLI and the app

---

## 5. Desktop app (Tauri)

### 5.0 Art direction (decided 2026-11-07)

- **Office:** reference #10 (`pixel-art/Gemini_Generated_Image_464ll5…`), the calmer wall, so speech bubbles fit up top. It's turned into a real low-res pixel background (~320–400 px wide, fixed palette) by a pixelize tool and scaled up crisply.
- **Wolfgang in the scene:** a small animated sprite sitting in the desk chair (idle/blink, talk, type, sip coffee, hand over a folder), drawn in code in the colours of the references.
- **Mood portraits:** the eight expression studies, cleaned the same way and cropped to head and shoulders, appear next to the speech bubble and change with the dialogue `mood`:
  `happy` → #1 smiling · `thinking` → #2 chin / #7 looking up · `surprised` → #3 · `proud` → #5 laughing / #4 content · `sorry` → #8 head scratch · `neutral` → #6 side-eye
- **Objects:** filing cabinets (left) → settings, clipboard on the desk → presets, mug → his coffee, desk → drop zone
- **CLI portrait:** stays a tiny 16×16, but gets the new look (bun, clear glasses, striped cardigan)
- The AI references have garbled text (certificates, „CALEPDAY“). It disappears at the target resolution or gets painted over.

### 5.1 Scene

- One cozy **pixel-art office**: Wolfgang behind his desk, with a window, plant, shelf, coffee mug and filing cabinet. Fixed internal resolution (e.g. 320×180) scaled by whole numbers for crisp pixels.
- **The desk is the drop zone.** When files are dragged over it, it highlights and Wolfgang looks up.
- **Speech bubbles in the scene**: Wolfgang's bubble pops over his head with typewriter text that can be skipped with a click or Space. Your answers appear as bubbles from the bottom, as if you were speaking. The last answer is a custom text field when the question allows one. Keyboard: arrows + Enter.
- **Clickable objects** for settings:
  - Notebook on the desk → presets
  - Filing cabinet → settings (default web format, default size/quality, separator, sound)
  - Settings open as a pixel-styled panel, not as a conversation

### 5.2 Working & done

- Wolfgang types and stamps at the desk while a pixel progress bar fills („Datei 12 von 42“)
- When done he hands over a folder sprite and gives a summary in character: „Hab 38 MB auf 4 MB gekriegt.“ Errors are mentioned briefly.
- Button „Im Finder zeigen“

### 5.3 Ambience

- Idle animations: blinking, sipping coffee, steam rising, occasional stretch
- Soft SFX: chiptune blips while text types, stamp/paper sounds while working, a short jingle when done
- A single mute toggle that is remembered

### 5.4 Extras

- Dropping files onto the Dock icon works too
- Menu item „Kommandozeile installieren“ symlinks the bundled CLI to `/usr/local/bin/wolfgang`, asking for an admin password

---

## 6. CLI ✅

- `wolfgang [PATH...]` in a terminal starts the **conversation**: Wolfgang's pixel portrait (half-block characters, truecolor or 256 colours, changing with his mood) next to speech bubbles, and answers picked with the arrow keys (★ marks the current/default one). „✎ Selbst eintippen …“ opens the custom answer, „← Zurück“ or Esc goes back, Ctrl-C says goodbye.
- `wolfgang` without a path asks for one: drag a folder into the terminal window, or press Enter for the current folder.
- **Flags pre-answer questions.** Any question fully answered by flags is skipped, and the rest are asked. Flags win over „wie letztes Mal“ and presets:
  ```
  wolfgang --web -f avif -d 1600 -q 80 -n urlaub ./bilder
  wolfgang --hq -f png ./scans          # jpg/png imply --hq
  wolfgang --only fonts ./projekt       # leave the images alone
  wolfgang -p "Blog-Bilder" ./bilder
  ```
  Old 1.x flags still work: `-n`, `-d` (also `--dimension`), `-q`, `-s dash|underscore|caps`, `-a`, `-r` (= `legacy-reset`).
- `--yes`, or no terminal: **no conversation**. Defaults, preset and flags decide, and web mode is the default. `--all` re-converts known files.
- `--json`: one JSON object per line: `plan`, then a `file` event per file, then `done`. It never asks anything.
- Ctrl-C during conversion stops after the current file (Wolfgang then talks about what's done). A second Ctrl-C quits.
- Subcommands: `wolfgang presets [list|remove NAME]`, `wolfgang settings [show|set KEY VALUE]` (keys: `web-format`, `size`, `quality`, `hq-format`, `separator`, `sound`), `wolfgang legacy-reset PATH [--yes]`.
- `legacy-reset` only renames files recorded in the old `.wolfgang_run_log.jsonl`, with their exact index. Wolfgang 1.x renamed anything ending in `-NNNN`, which could hit files like `Sommer-2023.jpg`. Without `--yes`, it shows the changes first. The old log is kept as `.jsonl.done`.
- Exit codes: 0 ok, 1 some files failed (or another error), 2 usage error, 130 aborted.
- `./install.sh` builds and installs into `/usr/local/bin` (it sets up Homebrew/cmake/nasm/Rust if missing). The 1.x scripts and installers are in `legacy/`.

## 7. Architecture

```
wolfgang/
├── crates/
│   ├── wolfgang-core/        # library ✅
│   │   ├── src/scan.rs       # walking drops, piles, keyword files, Finder order
│   │   ├── src/workspace.rs  # output folder, summary, re-run detection, planning jobs + names
│   │   ├── src/run.rs        # parallel execution, progress events, cancel, atomic writes
│   │   ├── src/image/        # ImageIO decode (+ colour), Lanczos resize, jpg/webp/avif/png encode
│   │   ├── src/font.rs       # ttf/otf/woff → woff2 with round-trip check, web names
│   │   ├── src/naming.rs     # SEO names, transliteration, separators
│   │   ├── src/runlog.rs     # .wolfgang/run.jsonl
│   │   ├── src/settings.rs   # RunSettings (what the conversation produces)
│   │   ├── src/store.rs      # user settings, last run, presets
│   │   └── examples/drop.rs  # dev stand-in for the CLI
│   ├── wolfgang-dialogue/    # dialogue state machine + loader ✅ (examples/chat.rs = text preview)
│   └── wolfgang-cli/         # binary `wolfgang`: flags, terminal conversation, portrait ✅
├── app/                      # Tauri app
│   ├── src-tauri/            # thin Rust layer → wolfgang-core, emits progress events
│   └── src/                  # frontend: canvas scene, sprites, bubbles, panels (TypeScript)
├── dialogue/de.yaml          # all of Wolfgang's lines and the question tree
├── assets/reference/         # the user's reference images (office + 8 portraits)
├── assets/pixel/             # generated pixel art + scene.json; src/*.sprite = hand-editable sources
│   (crates/wolfgang-art)     # the build tool
├── legacy/                   # old Bash wolfgang
├── spike/                    # M1 throwaway spike
└── PLAN.md
```

- **Extending with new converters**: each pile is a `FileKind` (classification), a section in `RunSettings`, a `Task` variant (planning → target names) and a branch in `run::convert_job`. Audio, video and PDF are added the same way, with their questions living in the dialogue file. This is deliberately simpler than the trait from the first draft: two converters didn't justify the abstraction, and the shape will be clearer once audio/video exist. The later Python/local-model transcription can be a task that calls a bundled sidecar process.
- **Parallelism**: jobs run on a thread pool (rayon) with cancellation support.
- **Image pipeline (decided by the spike, replaces the libvips idea)**:
  - **Decode with Apple ImageIO** (system framework, via `objc2-image-io`). It reads JPG/PNG/GIF/TIFF/WebP/HEIC/AVIF and Camera RAW, applies EXIF orientation and develops RAW with Apple's engine.
  - **Colour**: drawn into an sRGB `CGBitmapContext`, so ColorSync converts the colour profile. No lcms2 needed.
  - **Resize**: `fast_image_resize` Lanczos3 on premultiplied pixels, then unpremultiply
  - **Encode**: `mozjpeg` (JPG), `libwebp` (WebP), `ravif`/rav1e (AVIF), `png` (PNG), all statically linked
  - Decoding sits behind a `Decoder` trait so a Linux CLI can swap ImageIO for another decoder later
- **Fonts**: `woofwoof`, which wraps **Google's reference woff2 library** (the same decoder Chrome uses), for encoding and for the round-trip check. No subsetting, no HarfBuzz. WOFF1 input is unpacked by our own small zlib reader.

### Spike result (2026-11-06) ✅

One **8.8 MB arm64 binary that links only macOS system libraries** (ImageIO, CoreGraphics, libc++). It needs no Homebrew and no bundled dylibs, and its minimum macOS is 11.0. *(Build machines need `cmake` + `nasm`; users need nothing.)*

| Test | Result |
|---|---|
| HEIC 6016² → WebP 1400, q80 | 25.6 MB → 33 KB, 0.37 s |
| HEIC → AVIF 1400, q60 | 25.6 MB → 19 KB, 0.49 s |
| HEIC → JPG HQ (full size, q95) / PNG | 8.0 MB in 3.9 s / 53 MB in 5.3 s |
| NEF RAW (Nikon D3S) → WebP 1400 / JPG HQ full 4256×2832 | 10.4 MB → 29 KB / 985 KB, colours look natural |
| JPG with EXIF orientation 6 | Correctly output upright (933×1400) |
| PNG with alpha → WebP/AVIF/JPG | Clean edges, no dark fringes. JPG is flattened onto white. |
| AVIF and WebP as *input* | Works |
| Arial.ttf (TrueType) → WOFF2 | 755 KB → 352 KB |
| Inter-Bold.otf (CFF) → WOFF2 | 617 KB → 189 KB |

Lessons for the real implementation:
- HQ PNG/JPG of very large images takes a few seconds each, so per-file progress and parallel jobs matter
- AVIF decoding through ImageIO needs macOS 13+, so the app requires **macOS 13** minimum
- WOFF2 validity: since M2, every font is decoded again with the reference decoder (same as Chrome) and compared glyph by glyph. Still open: an actual browser render test.
- Still open: confirm RAW is truly developed rather than the embedded preview used, across several camera brands (needs sample files from colleagues).

### Milestone 2 result (2026-11-06) ✅

Real-world drop (HEIC wallpaper, Nikon RAW, EXIF-rotated JPG, transparent PNG, Inter OTF/variable TTF, Arial): **8 files, 41.7 MB → 1.1 MB in 1.5 s**. A second drop of the same folder recognises everything as known.

Findings:
- Google's woff2 encoder always **normalises glyph padding and drops `DSIG`** (the digital signature, which is invalid after any repacking; browsers ignore it). The round-trip check therefore compares glyphs by content (points, contours, components, instructions), not by raw bytes. A deliberately corrupted glyph is still caught (tested).
- Google's encoder packs better than `ttf2woff2` from the spike (Arial: 312 KB vs 352 KB).
- Quality mapping (`settings::encoder_quality`): WebP/JPG use the user's number; AVIF uses 0.9× that. This still needs a visual calibration pass with real photos.

### Milestone 3 result (2026-11-06) ✅

- `dialogue/de.yaml` holds the whole conversation: around 30 nodes, random line variants, conditions, plural forms (`{images:ein Foto:Fotos}`), typed custom answers and Wolfgang's reactions to invalid input. It is embedded into the binary. Writers can try edits without rebuilding via `WOLFGANG_DIALOGUE=path/to/de.yaml`.
- The file is **validated on load**: unknown targets, settings keys, conditions or placeholders are reported all at once, with node names.
- The engine (`Conversation`) returns serialisable `Step`s (`ask` / `start` / `cancelled` / `end`) with mood, lines, answers (the current/default one is marked) and an input field description. Back-navigation, `foreach` (keywords per folder) and post-run talk („98 Prozent gespart“, „Als Vorlage merken“) are supported.
- Name previews in the separator question use the real first image, prefix and keywords (`kunde-mueller-team-meeting-koeln-0001.webp`).
- „So wie letztes Mal“ and presets reuse format, size, quality, prefix and separator, but **not typed keywords**, which describe the earlier pictures. Wolfgang asks again when needed.
- If everything was already converted, Wolfgang says so right after the greeting.
- The confirmation summary text is generated in Rust (`summary_text`), not in the YAML, because it combines many settings. Change it there.

### Milestone 5 result (2026-11-07) ✅

- **`crates/wolfgang-art`** (`cargo run --release -p wolfgang-art -- build`) turns `assets/reference/` and `assets/pixel/src/*.sprite` into `assets/pixel/`, and writes a clickable preview to `target/wolfgang-art/preview.html`.
- **Scene: 320×180** (16:9, shown at 4× = 1280×720). Office and portraits end up with almost the same pixel size, so the whole scene has one pixel scale.
- **Pixelizing:** a k-means palette in OKLab, the majority colour per block, and an outline pass so thin dark lines stay continuous. The office has no strict grid (art pixels ≈ 4 reference pixels, not aligned). The portraits have a clean 11.5-pixel grid.
- **Portraits:** 96×96 head-and-shoulders, one shared 40-colour palette, transparent background (including areas enclosed by an arm). Moods map in `scene.json`: neutral → sideeye/content, happy → happy, thinking → thinking/dreamy, surprised → surprised, proud → laughing/content, sorry → sorry.
- **Desk sprite:** 46×56, 8 frames (idle, blink, talk, smile, typeA/B, sip, hand), animations idle/talk/type/sip/hand/happy with timings. The source is a plain-text pixel grid (`wolfgang.sprite`), validated by a test.
- **UI pieces** (`ui.sprite`): speech bubble and tail, answer bubbles (normal and hover), wooden panel, cursor, sparkle. 9-slice insets are in `scene.json`.
- **Hotspots** (`scene.json`): desk = drop zone, filing cabinets = settings, clipboard = presets, mug = sip coffee, computer = about/CLI install. Speech bubbles go on the free wall above Wolfgang's head.
- **Preview layout (visual-novel style):** Wolfgang's bubble at the top with its tail pointing at him, the mood portrait in a wooden frame at the bottom left, answers at the bottom right, and the desk in the middle as the drop zone. The font in the preview is Pixelify Sans (Google Fonts, OFL); the app will bundle it to work offline.

---

## 8. Milestones

1. ✅ **Spike: bundling.** Proved that one self-contained binary (ImageIO decoding + static encoders + WOFF2) converts HEIC/RAW → WebP/AVIF/JPG/PNG and TTF/OTF → WOFF2. See section 7.
2. ✅ **Core engine**: scanning, piles, image/font converters, naming, run log, re-run detection, settings/presets. 26 unit + 9 end-to-end tests.
3. ✅ **Dialogue engine + `dialogue/de.yaml`**: state machine, typed custom answers, written in Wolfgang's voice. 18 tests, including 300 random conversations.
4. ✅ **CLI**: flags, TUI conversation, `--yes`/`--json`, presets/settings commands, `legacy-reset`. Moved the Bash script to `legacy/`. 17 CLI tests; the interactive mode was driven in a pseudo-terminal.
5. ✅ **Pixel art**: pixelize tool for the references; office background from #10; mood portraits from the eight studies; animated desk sprite (idle/talk/type/sip/handover) drawn in code; bubbles, panels, object highlights; new-look CLI portrait. See *Milestone 5 result*.
6. **Tauri app**: scene, drag & drop on the desk, bubbles, progress, result, settings/preset panels, SFX, CLI install.
7. **Release**: `.dmg` build via GitHub Actions (macOS arm64 runner), README rewrite (German), first-launch instructions for the unsigned app. Tag v2.0.0.

---

## 9. Open points

- Exact default quality values per codec (calibrated during the spike so that WebP/AVIF at „Ausgewogen“ look the same)
- Final dialogue texts: I write the first draft in `dialogue/de.yaml` for you to review
- App icon: Wolfgang's face in pixel art
