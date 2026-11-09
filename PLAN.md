# Wolfgang 2.0 — Plan

> Wolfgang is the personification of conversion: a cozy, bearded guy in a cardigan who sits behind his desk with a coffee and converts whatever you put in front of him.
> Usability for non-technical colleagues is the top priority.

Status: **approved**. Milestones 1–10 are done; v2.2.0 brought video and audio, milestone 10 is waiting for the next release.

---

## 1. Goals of iteration 1

1. Rebuild Wolfgang as a **Rust engine** shared by a **CLI** and a **macOS desktop app (Tauri)**.
2. **Image conversion** in two modes: *„Hochwertig behalten“* (keep high quality) and *„Fürs Web vorbereiten“* (prepare for web).
3. **Font conversion** to WOFF2 (complete fonts, no subsetting) with filename normalization.
4. The configuration is a **conversation with Wolfgang**: game-style speech bubbles in a pixel-art office (app) and the same dialogue in the terminal (CLI).
5. **Zero setup** for users: all codecs are bundled, no Homebrew or ImageMagick needed.

### Out of scope for v2.0 (roadmap, but the architecture must allow it)

- ~~Audio (WAV/AIFF → MP3/AAC/Opus)~~ — done in v2.2 (MP3/AAC)
- ~~Video (MP4/WebM, poster frames), which will mean bundling ffmpeg~~ — done in v2.2
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
| External tools | None for users. ~~Video/audio would use a Homebrew `ffmpeg`~~ — **changed for v2.2: ffmpeg is bundled inside Wolfgang.app** (colleagues never update Homebrew; the self-update keeps the bundled copy fresh). See *Milestone 9*. |
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
- **One size** per image: longest side = N px (default 1600 since 2026-11-07, previously 1400), never upscaled
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

- **After 2.3.0 (2026-11-09, user's request): always `wolfgang_output/` where the files are.** A single dropped folder → `<folder>/wolfgang_output/` inside it, mirroring the folder's contents. Anything else (single files, several items) → `wolfgang_output/` next to the first item, mirroring the dropped items by name. Before, it was `<folder>-wolfgang/` next to the folder, and a single file got a folder named after its parent, which was confusing.
- Every drop from the same place shares one `wolfgang_output` (the run log knows files by content, so „Kenn ich schon“ works across folder and single-file drops). A `wolfgang_output` that isn't Wolfgang's (no run log, not empty) is never written into; `-2`, `-3`, … are used instead. Scanning skips `wolfgang_output*` and the old `*-wolfgang` folders. Old `*-wolfgang` folders are not picked up again: things converted there count as new once.
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
5. Size: Klein (800) / Normal (1600) / Groß (2400) / custom px
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
- **Visibility (2026-11-09):** Wolfgang's brown beard disappeared against the brown leather armchair behind him. Chosen from four mockups: the armchair is now dark blue-grey (`office.png`), and every sprite frame has a 1-pixel outline in the outline colour `O` (`wolfgang.sprite`). A light chair was rejected (bright slivers next to his ears looked restless); recolouring the whole office or changing the perspective was rejected as too costly for the cosy look.
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
│   │   ├── src/media.rs      # videos + audio via the bundled ffmpeg: probe, filters, progress, cancel
│   │   ├── src/naming.rs     # SEO names, transliteration, separators
│   │   ├── src/runlog.rs     # .wolfgang/run.jsonl
│   │   ├── src/settings.rs   # RunSettings (what the conversation produces)
│   │   ├── src/store.rs      # user settings, last run, presets
│   │   └── examples/drop.rs  # dev stand-in for the CLI
│   ├── wolfgang-dialogue/    # dialogue state machine + loader ✅ (examples/chat.rs = text preview)
│   └── wolfgang-cli/         # binary `wolfgang`: flags, terminal conversation, portrait ✅
│   └── wolfgang-app/         # Tauri app ✅: src/ (Rust), ui/ (HTML/CSS/JS), icons/
├── dialogue/de.yaml          # all of Wolfgang's lines and the question tree
├── assets/reference/         # the user's reference images (office + 8 portraits)
├── assets/pixel/             # generated pixel art + scene.json; src/*.sprite = hand-editable sources
│   (crates/wolfgang-art)     # the build tool
├── scripts/                  # build-app.sh (app + dmg), build-ffmpeg.sh (Wolfgang's own ffmpeg)
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

- **`crates/wolfgang-art`**: `import` converts `assets/reference/` **once** into hand-editable PNG sources (`assets/pixel/src/office.png`, `src/portraits/*.png`), and refuses to overwrite them without `--force`. `build` turns everything in `src/` (PNGs + `*.sprite`) into sprite sheets, `scene.json` and a clickable preview (`target/wolfgang-art/preview.html`). From now on the PNGs are the source of truth and are drawn by hand (2026-11-07).
- **Scene: 320×180** (16:9, shown at 4× = 1280×720). Office and portraits end up with almost the same pixel size, so the whole scene has one pixel scale.
- **Pixelizing:** a k-means palette in OKLab, the majority colour per block, and an outline pass so thin dark lines stay continuous. The office has no strict grid (art pixels ≈ 4 reference pixels, not aligned). The portraits have a clean 11.5-pixel grid.
- **Portraits:** 96×96 head-and-shoulders, one shared 40-colour palette, transparent background (including areas enclosed by an arm). Moods map in `scene.json`: neutral → sideeye/content, happy → happy, thinking → thinking/dreamy, surprised → surprised, proud → laughing/content, sorry → sorry.
- **Desk sprite:** 46×56, 8 frames (idle, blink, talk, smile, typeA/B, sip, hand), animations idle/talk/type/sip/hand/happy with timings. The source is a plain-text pixel grid (`wolfgang.sprite`), validated by a test.
- **UI pieces** (`ui.sprite`): speech bubble and tail, answer bubbles (normal and hover), wooden panel, cursor, sparkle. 9-slice insets are in `scene.json`.
- **Hotspots** (`scene.json`): desk = drop zone, filing cabinets = settings, clipboard = presets, mug = sip coffee, computer = about/CLI install. Speech bubbles go on the free wall above Wolfgang's head.
- **Portrait frame:** a dedicated wooden frame with no gap. The portrait sits flush and the frame crops the bust at the bottom; the backdrop behind him is the office wall colour.
- **Preview layout (visual-novel style):** Wolfgang's bubble at the top with its tail pointing at him, the mood portrait in a wooden frame at the bottom left, answers at the bottom right, and the desk in the middle as the drop zone. The font in the preview is Pixelify Sans (Google Fonts, OFL); the app will bundle it to work offline.

### Milestone 6 result (2026-11-07) ✅

- **`crates/wolfgang-app`** (Tauri 2.12): `cargo run -p wolfgang-app` opens Wolfgang's office. The UI in `ui/` is plain HTML/CSS/JS with no bundler and no Node. The build script copies `assets/pixel` to `ui/pixel`.
- **`src/session.rs`** holds the app logic without Tauri (drop → conversation → run → remember, settings, presets) and is tested. `main.rs` exposes it as commands (`drop_files`, `choose`, `type_answer`, `back`, `start`, `cancel`, `settings`, `presets`, `about`, `install_cli`, …) and events (`progress`, `finished`, `files-opened`).
- **Window = office:** the window is exactly 320×180 × a whole scale, with no border. It opens at scale 4 (1280×720), or smaller if the screen is too small (largest scale fitting 75 % of the screen). ⌘+ / ⌘− change it in whole steps and the choice is remembered (`defaults.window_scale`); ⌘0 goes back to the default. The window isn't freely resizable. The title bar is transparent (`titleBarStyle: Overlay`): the native traffic lights float over the scene, and a strip along the top edge drags the window. A fully custom pixel title bar is possible, but was deliberately not built, to keep native macOS behaviour.
- **Screen:** Wolfgang's bubble types out with chiptune blips, and a whole bubble takes at most about 1.8 s (long texts type faster). Answers appear immediately and can be picked while he's still talking; Space skips the text, Enter answers. The mood portrait sits bottom left. Answers sit bottom right, starting just right of Wolfgang; small extras (Zurück, Im Finder zeigen) go side by side. Custom answers are typed in a field styled like an answer.
- **Controls:** mouse, ↑↓ Enter, Esc = back. Drop files anywhere in the window (the desk lights up).
- **Working:** Wolfgang types, a pixel progress bar fills, his working lines change every few seconds, and each file gets a stamp sound. Cancel stops after the current file. When he's done he hands over the folder, a jingle plays, and „Im Finder zeigen“ appears.
- **Objects (everything on or at the desk, changed 2026-11-07):** the computer opens settings (default web format, size, quality, HQ format, separator, sounds). The clipboard opens presets (list, delete). Clicking **Wolfgang himself** opens „Über Wolfgang“ (version, install CLI, credits). Clicking the mug makes him sip, and he also sips now and then when idle. The filing cabinet is decoration only.
- **Sound:** synthesised with WebAudio, no sound files; switched on/off in settings.
- **Font:** Pixelify Sans (OFL), bundled as WOFF2, converted by Wolfgang himself (80 → 24 KB).
- **App icon:** the happy portrait in a framed rounded square (`wolfgang-art icon`). Revamped 2026-11-09 (user's request: bigger face, bigger frame, Mac corners): Apple's template (shape 824 of 1024 px, continuous „squircle“ corners as a superellipse, soft shadow below), 10 px per art pixel and the portrait cropped around head and beard (face ≈ 1.25× bigger), a 4-pixel wooden frame. Then `cargo tauri icon crates/wolfgang-app/icons/master.png -o …` and copy the Mac files (icns, 32/128/256 PNGs, icon.png).
- **Checks:** `ui/mock.js` fakes the Rust side when `ui/index.html` is opened in a normal browser (`?demo=mode|size|confirm|work|done`, `?panel=settings|presets|about`), which is how the UI was screenshot-tested. In the real app, the UI reports errors to the log, and `WOLFGANG_SELFTEST=1` quits after start-up with exit 1 on any error.
- **Needs the bundle (M7):** dropping on the Dock icon needs document types in Info.plist, and the „install CLI“ button needs the `wolfgang` binary bundled next to the app executable. The code for both is in place.

### Milestone 7 result (2026-11-07) ✅

- **`scripts/build-app.sh`** builds the pixel art, the CLI and then `Wolfgang.app` plus the `.dmg` (`cargo tauri build`), and checks the result: CLI inside the app, signature, document types. Result: app 26 MB, **dmg 9.9 MB**.
- **CLI inside the app:** `bundle.macOS.files` copies `target/release/wolfgang` to `Contents/MacOS/wolfgang`. „Über Wolfgang → Installieren“ links it to `/usr/local/bin/wolfgang` (admin password via `osascript`).
- **Dock drop:** `crates/wolfgang-app/Info.plist` adds `CFBundleDocumentTypes` (`public.item`, `public.folder`) with `LSHandlerRank = Alternate`, so Wolfgang never becomes the default app for anything. Paths arriving via LaunchServices are kept in Rust (`take_pending`) until the UI is ready, so dropping on the icon of a closed app works too (tested with `open -a`).
- **Signing:** ad-hoc (`signingIdentity: "-"`), no notarization. Without any signature, Apple Silicon calls downloaded apps "damaged" with no way past it; ad-hoc gives the normal "can't be checked" dialog, which can be allowed once (README „Erster Start“). Real signing/notarization needs an Apple Developer account and could be added to the release workflow later.
- **GitHub Actions:** `ci.yml` runs clippy (`-D warnings`) and all tests on macOS 15 for pushes to main (only main since 2026-11-09, when the v2 branch was retired); skipped for documentation-only pushes (`*.md`, `docs/`, `assets/reference/`, `LICENSE`) and with `[skip ci]` in the commit message and for PRs. `release.yml` runs on tag `v*`: it checks that the tag matches the app version, tests, builds, and creates a **draft** release with the dmg attached. Not run yet (nothing pushed).
- **README** rewritten in German: install, first launch, app, CLI, tips, switching from 1.x, developer notes, how to release.

### v2.1: self-update (2026-11-09) ✅

- **Why:** colleagues never update anything by hand. Wolfgang has to bring new versions himself, and that also keeps a bundled ffmpeg (video/audio, next) from going stale.
- **How:** `tauri-plugin-updater`, driven from Rust (`check_update`, `install_update`; no JS permissions). On start the UI asks for an update; if there is one, Wolfgang offers it as soon as he's idle („Psst – es gibt 'nen neuen Wolfgang …“, *Ja, mach mal! / Später*). After „Später“ the offer stays in „Über Wolfgang“. The download shows in the progress bubble, then the app swaps itself and restarts. If `/Applications` isn't writable, the plugin asks for the admin password. Dev builds and `WOLFGANG_NO_UPDATE=1` never check.
- **Feed:** `https://github.com/k1tesurfen/wolfgang/releases/latest/download/latest.json`, so only *published* releases count (drafts stay invisible). The release workflow uploads `Wolfgang.app.tar.gz` + `latest.json` next to the dmg.
- **Signing:** updates are signed with a minisign key (not Apple). The private key is in `~/.tauri/wolfgang.key` (+ `.password`) and in the GitHub secrets `TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)`; the public key is in `tauri.conf.json`. `build-app.sh` uses the key when present, otherwise it builds without update files.
- **Tested:** a 2.0.9 build pointed at a local server updated itself to 2.1.0 and restarted (`WOLFGANG_SELFTEST=update` checks and installs without asking). The swapped app passes `codesign --verify`, and the CLI inside it reports 2.1.0. UI screenshots via the mock (`?update`, `?update=install`).
- **Limit:** 2.0.0 has no updater, so anyone on 2.0.0 installs 2.1 once by hand.

### Milestone 9: video + audio (v2.2) — decisions (2026-11-09)

Product questions were answered by the user one by one; this is the result. Video and audio ship together as **2.2.0**.

**Bundled ffmpeg**
- ffmpeg + ffprobe are bundled in `Wolfgang.app/Contents/MacOS/` next to the CLI, never taken from Homebrew. The self-update keeps them fresh.
- **LGPL build, no GPL parts** (no x264/x265): Apple **VideoToolbox** for H.264, **libvpx** for VP9, Apple **AudioToolbox** (`aac_at`) for AAC, **LAME** for MP3, **Opus** for the sound in WebM, **zimg** for HDR → SDR tone mapping (iPhone videos are HDR/HLG by default and would look grey and flat as plain H.264). SVT-AV1 is left out because AV1 isn't offered (see below); adding it later is one block in the build script.
- Built by `scripts/build-ffmpeg.sh` (pinned versions + checksums) into `target/ffmpeg/`, cached in CI, used by `scripts/build-app.sh` and `install.sh`.
- Wolfgang finds the tools in: `$WOLFGANG_FFMPEG_DIR` → next to the running binary (symlinks resolved, so `/usr/local/bin/wolfgang` → the app works) → `../libexec/wolfgang/` (`install.sh`) → `target/ffmpeg/bin` of the source tree (development). If they're missing, Wolfgang says so and leaves videos/audio alone.

**Video inputs:** `mp4, mov, m4v, mkv, webm, avi, mts, m2ts, mpg, mpeg, wmv, 3gp`. Animated GIFs stay photos (first frame), as before.

**Video, always „fürs Web“** (no mode question, no HQ mode). Questions, each skipped when it doesn't apply:
1. **Format:** „MP4 – läuft überall“ (H.264 + AAC, default) / „WebM – kleiner, dauert aber länger“ (VP9 + Opus). One file per video. VP9 rather than AV1 because Safari only plays AV1 on devices with AV1 hardware (iPhone 15 Pro+, M3+).
2. **Size** (shorter side, never upscaled; a vertical 1080p video becomes 1080×1920): Klein (720p) – für Hintergrund- und Loop-Videos / **Normal (1080p)** / Originalgröße, plus a typed custom value.
3. **Quality:** Kleiner / **Ausgewogen** / Schöner, mapped per codec. No typed custom value.
4. **Sound:** „Mit Ton“ (default) / „Ohne Ton – für Hintergrund- und Loop-Videos“. Skipped when none of the videos has sound.
5. **Poster frame:** „Soll ich dir ein Vorschaubild mitmachen?“ **Ja** / Nein. Taken from about 10 % into the video (not the often black first frame), same size as the video, as WebP, named like the video (`clip.mp4` + `clip.webp`).
- In a mixed pile the photo questions come first, then „Jetzt die 3 Videos …“.
- **Names:** web-safe original names, no numbering, no questions: „Drohne Strand FINAL.MOV“ → `drohne-strand-final.mp4`.
- Always: rotation applied, HDR tone-mapped to SDR, frame rate capped at 60, metadata (incl. GPS) stripped, `faststart` MP4s so they start playing before they're fully loaded.

**Audio inputs:** `wav, aif, aiff, flac, m4a, mp3, aac, ogg, opus, caf, wma`.

**Audio questions:**
1. **Format:** „MP3 – läuft überall“ (default) / „AAC (.m4a) – klingt bei gleicher Größe besser“. No Opus (Apple Music and older devices).
2. **„Was ist drauf?“** Sprache (Podcast, Interview → mono, small) / **Musik** (stereo, good) / Musik, beste Qualität. Wolfgang picks the bitrates himself.
- **Names:** web-safe original names, like videos. Text tags (title, artist …) are kept, embedded cover pictures are dropped.

**Shared**
- „So wie letztes Mal“ and presets cover photos, videos and audio. The last run is remembered **per pile**: a run with only videos doesn't forget how the photos were done last time. Piles the template doesn't cover are asked as usual.
- Long videos report progress *within* the file (from ffmpeg's progress output), so the bar keeps moving. Cancelling stops a running ffmpeg right away; the half-written file is thrown away.
- Originals are never touched; output goes into `wolfgang_output/` (see 3.6) mirroring the folders, side by side with the photos. Same behaviour in the app and the CLI. CLI flags: `--video-format mp4|webm`, `--video-size 720|1080|original|N`, `--video-quality smaller|balanced|nicer`, `--no-sound`, `--no-poster`, `--audio-format mp3|aac`, `--audio speech|music|best`, and `--only images|videos|audio|fonts`.

### Milestone 9 result (2026-11-09) ✅

- **Piles:** `FileKind::Video` / `Audio`, `RunSettings.videos` / `.audio`, `Task::Video` / `Audio`, all in `src/media.rs` — the „Extending with new converters“ recipe held up. Old settings files and run logs load unchanged (new fields default to `None`).
- **ffmpeg build:** ffmpeg 9.0.2 with libvpx 1.17.0, Opus 1.6.1, LAME 3.100, zimg 3.0.6: ffmpeg 21 MB + ffprobe 20 MB, linked only against macOS. Encoders/muxers are limited to what Wolfgang writes; all decoders, demuxers and filters are in, so anything can be read. The script refuses to finish if the result isn't LGPL or links anything outside macOS. App 72 MB, **dmg 31 MB** (was 9.9 MB).
- **Calibration** (1080p excerpt of macOS's „Tahoe Day“ wallpaper video, SSIM against the lossless source): H.264 VideoToolbox q 50/58/65 ≈ 2.8/4.4/6.7 Mbit/s (SSIM 0.946/0.962/0.973); VP9 CRF 40/36/31 is about 40 % smaller at better SSIM, but about 3× slower than hardware H.264. 60 fps footage uses q−4 (≈ 20 % bigger than 30 fps instead of 55 %). A bitrate cap was tried and dropped: with `maxrate` set, VideoToolbox *raises* the bitrate of simple clips towards the cap.
- **Software fallback:** Apple's software H.264 encoder (no free hardware encoder, e.g. VMs/CI) refuses constant quality; Wolfgang then retries once with a bitrate derived from size and quality step.
- **HDR:** HLG/PQ is tone-mapped with zimg + `tonemap=mobius` at 203 nits reference white (BT.2408). On a synthetic HLG clip it gets back to the SDR original's brightness and saturation (saturation 30 vs 31; without tone mapping 15, i.e. grey and flat). Everything is tagged BT.709.
- **Real-world:** the 2-minute 4K 240 fps HEVC wallpaper → 1080p60 MP4 + poster in 90 s (175 MB → 85 MB, M2 Pro).
- **Conversation:** a `piles` node routes photos → videos → audio → re-run check; each pile has a `*_done` marker, so templates and CLI flags can answer whole piles. „Nochmal ändern“ (`restart`) asks every pile again with the previous answers marked. The greeting lists all piles („3 Fotos, ein Video, eine Audiodatei und eine Schrift“). If ffmpeg is missing, Wolfgang says so and leaves videos/audio alone.
- **Tests:** 7 end-to-end media tests on clips generated with lavfi (MP4 + poster, vertical WebM without sound, HDR + 120 fps → SDR 60 fps without upscaling, MP3/AAC, name collisions with photos, progress, cancelling within a second, broken file), 3 new conversation tests plus video/audio drops in the 300 random conversations, CLI flags and an end-to-end CLI run. They're skipped with a note when ffmpeg isn't built; CI builds and caches it.
- **Still open:** real iPhone footage (HDR, Dolby Vision, rotation) from colleagues to confirm the tone mapping beyond the synthetic test; whether CI's virtual Macs take the software-fallback path (the tests would show it).

### Milestone 10: polish after 2.2 (2026-11-09) ✅

Decided with the user: no paid Apple Developer account, ever (internal tool; colleagues approve it once). Releases are batched, not one per feature, and built on the user's Mac.

- **Settings:** default video format, video size, video quality and audio format, in the app's settings panel (10 rows, still fits) and `wolfgang settings set video-format|video-size|video-quality|audio-format`. They pre-mark the answers in the conversation.
- **Time estimate:** before the run, Wolfgang says how long the videos take when it's a minute or more („Das dauert etwa 9 Minuten. Zeit für 'nen Kaffee.“). Model from measurements on an M2 Pro: reading an HEVC source ≈ 1.3 Gpx/s, VP9 encoding ≈ 95 Mpx/s, H.264 hardware ≈ 1.5 Gpx/s; MP4 = max(read, encode), WebM = read + encode; several videos ÷ 1.3. 30 s of 4K30 → 1080p: measured 5.5 s (MP4) / 25 s (WebM).
- **Bigger than before (user's choice: „take the original if possible“):** if the converted video is bigger and the original is already web-ready for the chosen format (MP4: H.264 + AAC; WebM: VP9/VP8 + Opus/Vorbis; 8-bit 4:2:0, no HDR, ≤ 60 fps, not bigger than the chosen size), the original is remuxed (`-c copy`, no metadata, faststart) and taken. Otherwise the bigger file stays and Wolfgang says so. Notes after a run are now typed (`run::Note`).
- **Animated GIFs → videos** (reversed from 2.2, user's request): GIFs with more than one frame go into the video pile (our own small GIF block reader, `gif.rs`, also finds transparency). For transparent ones Wolfgang asks „Weiß“ / „Schwarz“ (user's choice; MP4 for compatibility). Flattening keeps the GIF's timing: an opaque filled copy of each frame under the frame (`split` + `drawbox replace=1` + `overlay`; both `split` branches must be RGBA, otherwise the alpha is lost — caught by a test). After the run he reminds that web GIF-videos need `autoplay muted loop playsinline`.
- **Pictures from PDFs (user's choice: embedded pictures, as they are, no questions):** `pdf.rs` with lopdf walks every page including form XObjects (layout programs nest images there). JPEGs are copied byte for byte; CMYK JPEGs, JPEGs with a soft mask and JPEG 2000 go through ImageIO to PNG; raw images (Gray/RGB/CMYK/ICC/Indexed, 1–16 bit, soft masks, `/Decode [1 0]`) become PNG. Skipped: stencil masks, pictures under 48 px, the same object or content twice (logos on every page); JBIG2/CCITT scans and odd colour spaces are counted and mentioned. Output: `broschuere/broschuere-seite-01-1.jpg` … (user's idea: the PDF's web-safe name, cut at a word boundary at 40 characters, is the folder and the start of every picture's name, so pictures stay findable once copied elsewhere) PDFs don't count towards „gespart“; Wolfgang says how many pictures he got out.
- **Releases from the Mac:** `scripts/release.sh [--publish]` (checks, tests, builds, pushes commit + tag, uploads via `gh`). `release.yml` is now a manual fallback (`workflow_dispatch` with a tag). Release text in `.github/release-body.md`.
- **Keyboard and Finder dialog (user's request, 2026-11-09):** clicking the desk, ⌘O, `o` or the small „Dateien auswählen …“ answer on the empty desk opens a native `NSOpenPanel` (files *and* folders, several at once; Tauri's dialog only does one or the other). For everyone: ↑↓ Enter Esc Space as before, plus 1–9 (that answer), ⌘O, ⌘, (settings), ? (a „Tastatur“ panel with all shortcuts). For vim people, on letters that did nothing before: j k l h, g G, i or / (own answer), o s p, q, Ctrl-[ = Esc; none of it while typing in a field. Panels: j/k rows, h/l buttons, Enter presses; the focus outline only appears once the keyboard is used there, so mouse users see no change. Tested by driving the browser mock with real key events (headless Chrome over the DevTools protocol).
- **Desk mat (user's request):** a dark green felt mat under the keyboard (chosen over blue-grey from two mockups) gives the desk visible hover feedback: hovering anywhere over the desk makes the mat sparkle; clicking anywhere on the desk opens the Finder dialog, and drops light up the whole desk. The desk hotspot has two rects (mat first for the sparkle, then the whole desk). The mat is a trapezoid in perspective (6 px narrower per side at the back, user's choice over 3 px) with a visible front edge, so it lies on the desk like the papers do. Overlapping objects: the smallest one under the pointer wins, which also fixed the coffee mug (the desk used to swallow its clicks).
- **Wilma, the orange office cat (user's request):** lives under Wolfgang's desk in a round teal cushion. Hand-editable sprite `assets/pixel/src/wilma.sprite` (orange tabby, cream muzzle and paws, green eyes; frames: asleep ×2 + ear twitch, awake + blink, sitting + blink, washing ×2, walking ×4, stretching, cushion back/front so she lies *in* it). Her day is a small state machine in `app.js` (`Wilma`): mostly asleep (1–3 min, ear twitches), wakes and looks into the camera, sometimes sleeps on, otherwise stretches and wanders 1–3 times along the floor in front of the desk (`wilma_floor` in scene.json), sits, blinks, washes, then goes back to bed. Side views are mirrored when she walks left. Hover: sparkle and „Wilma (schläft)“; click: „mrrp“ (asleep, she lifts her head) or „miau“ (she sits down). She can be patted while Wolfgang works. With macOS „Bewegung reduzieren“ she only sleeps. Wolfgang mentions her in a few idle/working lines. Preview: `?wilma` wakes her right away.
- **Wilma, redrawn from the user's character study (`pixel-art/Gemini_Generated_Image_ox8a…`):** the study is on a clean 31.7 px grid, so the sleeping, sitting and walking poses were sampled cell by cell and snapped to her palette (dark eyes now, as in the study). She's about 1.6× bigger (sitting 22×23, walking 35×19, cushion 35 wide), which still fits the gap under the desk and the floor strip. User feedback: sitting kept as is; walking got a one-pixel snout; the bright mouth bar of the sleeping pose is hidden behind the cushion's front rim (she lies one row above its top). Derived by hand: breathing, ear twitch, eyes open, blinking, washing (tongue), a yawn instead of the stretch (no clean stretch in the study's style), three more walking frames.
- **App check:** settings panel and progress checked in the browser mock (headless screenshots); the app logic with a real video drop is a test in `session.rs`. Not done: clicking through the real app window (no desktop control in this environment) — a short manual check before the next release.

---

## 8. Milestones

1. ✅ **Spike: bundling.** Proved that one self-contained binary (ImageIO decoding + static encoders + WOFF2) converts HEIC/RAW → WebP/AVIF/JPG/PNG and TTF/OTF → WOFF2. See section 7.
2. ✅ **Core engine**: scanning, piles, image/font converters, naming, run log, re-run detection, settings/presets. 26 unit + 9 end-to-end tests.
3. ✅ **Dialogue engine + `dialogue/de.yaml`**: state machine, typed custom answers, written in Wolfgang's voice. 18 tests, including 300 random conversations.
4. ✅ **CLI**: flags, TUI conversation, `--yes`/`--json`, presets/settings commands, `legacy-reset`. Moved the Bash script to `legacy/`. 17 CLI tests; the interactive mode was driven in a pseudo-terminal.
5. ✅ **Pixel art**: pixelize tool for the references; office background from #10; mood portraits from the eight studies; animated desk sprite (idle/talk/type/sip/handover) drawn in code; bubbles, panels, object highlights; new-look CLI portrait. See *Milestone 5 result*.
6. ✅ **Tauri app**: scene, drag & drop on the desk, bubbles, progress, result, settings/preset panels, SFX, CLI install. See *Milestone 6 result*.
7. ✅ **Release**: `.dmg` build via GitHub Actions (macOS arm64 runner), README rewrite (German), first-launch instructions for the unsigned app. Released as v2.0.0. See *Milestone 7 result*.
8. ✅ **Self-update** (v2.1). See *v2.1: self-update*.
9. ✅ **Video + audio** (v2.2): MP4/WebM for the web, poster frames, MP3/AAC, on a bundled LGPL ffmpeg. See *Milestone 9* and *Milestone 9 result*.
10. ✅ **Polish** (next release): video/audio settings, time estimates, well-packed videos, animated GIFs → videos, pictures from PDFs, releases from the Mac. See *Milestone 10*.

---

## 9. Open points

- Exact default quality values per codec (calibrated during the spike so that WebP/AVIF at „Ausgewogen“ look the same)
- Final dialogue texts: I write the first draft in `dialogue/de.yaml` for you to review
- App icon: Wolfgang's face in pixel art
