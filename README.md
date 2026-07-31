# WOLFGANG

**W**eb **O**ptimized **L**ightweight **F**ast **G**raphics **A**nalyser a**n**d **G**enerator

A small shell tool that turns a folder of photos into web-ready, SEO-friendly JPG and WebP images — resized, compressed, and renamed from keyword files.

![Shell](https://img.shields.io/badge/shell-bash-4EAA25?logo=gnubash&logoColor=white)
![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey)
![License](https://img.shields.io/badge/license-MIT-blue)

---

## What it does

- **Resizes** every image so its longest side matches your target (default `1400px`, never upscales)
- **Compresses** with a quality setting you control (default `80`)
- **Exports twice** — a `.jpg` and a `.webp` of every source image
- **Renames for SEO** using keywords read from a `.txt` file next to your images
- **Indexes persistently** so a second run continues at `0002`, `0003`, … instead of overwriting
- **Remembers settings** — the wizard offers your last five configurations as presets
- **Undoes itself** with `--reset`

Recursively walks subdirectories and skips its own `*-wolfgang` output folders.

## Requirements

| Tool                                               | Purpose                                          |
| -------------------------------------------------- | ------------------------------------------------ |
| [ImageMagick](https://imagemagick.org/) (`magick`) | resizing and conversion                          |
| [`jq`](https://jqlang.github.io/jq/)               | run logging, persistent indexing, wizard presets |

`jq` is optional — without it Wolfgang still converts, but loses indexing, dedupe, and presets.

## Installation

Clone the repo and run the installer. It installs Homebrew (if missing), then `imagemagick` and `jq`, and copies the script to `/usr/local/bin/wolfgang`.

```bash
git clone https://github.com/k1tesurfen/wolfgang.git
cd wolfgang
./install.sh          # English
./install-ger.sh      # Deutsch
```

Both installers register the same `wolfgang` command — pick one language.

## Usage

```bash
wolfgang                        # interactive wizard
wolfgang ~/Pictures/trip        # wizard-free, all defaults
wolfgang -n holiday -d 1600 -q 85 ~/Pictures/trip
```

Running `wolfgang` with no arguments at all starts the wizard, which walks you through prefix, dimension, quality, separator style, and filename appending.

### Options

| Option                  | Description                                      | Default |
| ----------------------- | ------------------------------------------------ | ------- |
| `-n, --name PREFIX`     | Filename prefix                                  | `image` |
| `-d, --dimension PIXEL` | Longest side in pixels                           | `1400`  |
| `-q, --quality 1-100`   | Compression quality                              | `80`    |
| `-s, --separator STYLE` | `dash`, `underscore`, or `caps`                  | `dash`  |
| `-a, --append`          | Append the original filename (without extension) | off     |
| `-r, --reset`           | Delete the run log and rename originals back     | —       |
| `--debug`               | Verbose diagnostic output                        | off     |
| `-h, --help`            | Show help                                        | —       |

**Argument:** `INPUT_PATH` — directory containing the images (default: current directory).

**Supported input formats:** `jpg`, `jpeg`, `png`, `webp`, `heic`, `gif`, `tif`, `tiff`.

## Output

```
./2026-07-31_14-22-05-wolfgang/
├── resized_jpg/
│   └── holiday-beach_sunset-algarve-0001.jpg
└── resized_webp/
    └── holiday-beach_sunset-algarve-0001.webp
```

The output folder is created in your **current** directory and timestamped, so runs never clobber each other. Subdirectory structure from the input path is preserved inside both `resized_jpg/` and `resized_webp/`.

### Filename structure

```
[PREFIX]-[LOCAL_KEYWORDS]-[ORIGINAL_FILENAME]-[INDEX].ext
                              ↑ only with -a
```

| Part                | Source                                                          |
| ------------------- | --------------------------------------------------------------- |
| `PREFIX`            | `-n` or the wizard                                              |
| `LOCAL_KEYWORDS`    | the most recently modified `*.txt` in the image's own directory |
| `ORIGINAL_FILENAME` | the source filename, only when `-a` is given                    |
| `INDEX`             | persistent 4-digit counter, unique per input directory          |

### Separator styles

| Style        | Result                      |
| ------------ | --------------------------- |
| `dash`       | `holiday-beach_sunset-0001` |
| `underscore` | `holiday_beach_sunset_0001` |
| `caps`       | `HolidayBeachSunset0001`    |

## SEO keywords

Drop a `.txt` file into a directory of images. Wolfgang picks the **most recently modified** `*.txt` in that directory and uses it for every image there:

```text
beach sunset
algarve
golden hour
```

- One keyword per line
- Multiple words on a line are joined with `_` → `beach_sunset`
- Lines are joined with `-` → `beach_sunset-algarve-golden_hour`

Each subdirectory can carry its own keyword file. A `*.txt` in the directory you _run_ from is recorded in the log as a global keyword set but is **not** used in filenames.

## Run log

With `jq` installed, every run appends its settings and processed files to `.wolfgang_run_log.jsonl` in the root of `INPUT_PATH`. Wolfgang uses it to:

- skip images that were already converted
- continue the index counter across runs
- offer your last five configurations as wizard presets

## Reset

```bash
wolfgang --reset ~/Pictures/trip
```

Deletes `.wolfgang_run_log.jsonl` and strips the `-0001` style index back off your **original** files. Generated `*-wolfgang` output folders are left untouched — delete those yourself.

## Notes

- Source files are renamed in place with their index (`DSC_4711.jpg` → `DSC_4711-0001.jpg`). `--reset` reverses this.
- Aspect ratio is always preserved; images smaller than the target are left at their original size.

## License

MIT © k1te — see [LICENSE](LICENSE).
