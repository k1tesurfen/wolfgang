# Wolfgang

**Wolfgang wandelt um, damit du's nicht musst.**

Wolfgang sitzt gemütlich in seinem Büro, trinkt Kaffee und macht aus deinen Dateien,
was du fürs Web oder fürs Archiv brauchst. Du legst ihm was auf den Tisch, er fragt kurz nach –
fertig.

- **Bilder fürs Web:** WebP oder AVIF, verkleinert, ohne Metadaten (auch kein GPS),
  mit suchmaschinenfreundlichen Dateinamen wie `hotel-seeblick-terrasse-0001.webp`
- **Bilder hochwertig behalten:** nur ein anderes Format (JPG, WebP, AVIF, PNG), Originalgröße,
  Farbprofil bleibt erhalten
- **Videos fürs Web:** MP4 (läuft überall) oder WebM (kleiner), 720p, 1080p oder Originalgröße,
  auf Wunsch ohne Ton und mit Vorschaubild. iPhone-Videos in HDR sehen danach nicht flau aus.
- **Audio:** MP3 oder AAC (.m4a), passend für Sprache (Podcast, Interview) oder Musik
- **Schriften:** TTF, OTF und WOFF werden zu WOFF2 – eins zu eins, nichts wird weggelassen
- **Liest fast alles:** JPG, PNG, HEIC, AVIF, WebP, GIF, TIFF und Kamera-RAW (CR2, CR3, NEF, ARW, DNG);
  MP4, MOV, M4V, MKV, WebM, AVI, MTS, MPG, WMV, 3GP; WAV, AIFF, FLAC, M4A, MP3, AAC, OGG, Opus, CAF, WMA

Wolfgang gibt es als **App** (ein kleines Pixel-Büro, in dem du mit ihm redest)
und als **Kommandozeile** (`wolfgang`) für Skripte und Terminal-Fans.

Läuft auf Macs mit Apple-Chip ab macOS 13. Es muss nichts zusätzlich installiert werden –
auch kein ffmpeg, Wolfgang bringt sein eigenes mit.

---

## Installieren

1. Die neueste `Wolfgang_….dmg` von der [Releases-Seite](../../releases) laden.
2. Öffnen und **Wolfgang** in den Ordner **Programme** ziehen.
3. Wolfgang starten – beim ersten Mal siehe unten.

### Erster Start

Wolfgang ist (noch) nicht von Apple beglaubigt. macOS sagt deshalb beim ersten Start etwas wie
„Wolfgang kann nicht geöffnet werden, da Apple nicht überprüfen kann …“. So geht's trotzdem:

1. Den Hinweis mit **Fertig** schließen.
2. **Systemeinstellungen → Datenschutz & Sicherheit** öffnen, ganz nach unten scrollen.
3. Bei „Wolfgang wurde blockiert …“ auf **Dennoch öffnen** klicken und bestätigen.

Das muss man nur einmal machen. Wer lieber das Terminal nimmt:

```bash
xattr -dr com.apple.quarantine /Applications/Wolfgang.app
```

### Neue Versionen

Wolfgang schaut beim Start selbst nach, ob es eine neue Version gibt, und fragt dann:
„Soll ich mich mal eben auffrischen?“ – ein Klick, kurz warten, fertig. Wer „Später“ sagt,
findet das Angebot auch noch, wenn man auf Wolfgang klickt. (Das gibt's ab Version 2.1 –
von 2.0 aus einmal die neue `.dmg` von Hand installieren.)

### Kommandozeile

In der App auf **Wolfgang selbst** klicken → **Installieren**. macOS fragt einmal nach deinem
Passwort, danach gibt es den Befehl `wolfgang` in jedem Terminal.

---

## Die App

| Was | Wie |
|---|---|
| Dateien umwandeln | Dateien oder Ordner ins Fenster (auf den Schreibtisch) ziehen – oder aufs Dock-Symbol |
| Antworten | anklicken, oder **↑ ↓** und **Enter**. **Esc** = zurück, **Leertaste** = Wolfgangs Text sofort ganz zeigen |
| Eigene Antwort | „✎ Selbst eintippen …“, z. B. eine eigene Größe in Pixeln |
| Einstellungen | auf den **Computer** klicken (Lieblingsformate, Größe, Qualität, Schreibweise, Geräusche) |
| Vorlagen | auf das **Klemmbrett** klicken |
| Über Wolfgang / Kommandozeile | auf **Wolfgang** klicken |
| Größer / kleiner | **⌘+** / **⌘−** (Wolfgang merkt sich die Größe), **⌘0** = Standard |

Nach getaner Arbeit liegt alles in einem Ordner `…-wolfgang` neben dem, was du ihm gegeben hast.
„📂 Im Finder zeigen“ bringt dich direkt hin. Deine Originale fasst Wolfgang nie an.

---

## Die Kommandozeile

Im Terminal führt Wolfgang dasselbe Gespräch wie in der App, nur mit Pfeiltasten:

```bash
wolfgang ~/Bilder/Urlaub          # Wolfgang fragt, was du willst
wolfgang                          # Ordner ins Fenster ziehen oder Enter für den aktuellen
```

Optionen beantworten seine Fragen vorab. Mit `--yes` (oder in Skripten) fragt er gar nicht:

```bash
wolfgang --web -f avif -d 1600 -n "Hotel Seeblick" ~/Bilder/Hotel
wolfgang --hq -f png ~/Scans
wolfgang --only fonts ~/Projekt
wolfgang --only videos --video-format webm --video-size 720 --no-sound ~/Filme/Loop
wolfgang --audio speech ~/Podcast
wolfgang -p "Blog-Bilder" --yes ~/Bilder/Neu
wolfgang --json --yes ~/Bilder | jq .        # eine JSON-Zeile pro Ereignis
```

| Option | Bedeutung |
|---|---|
| `--web` / `--hq` | fürs Web vorbereiten / hochwertig behalten |
| `-f, --format` | `webp`, `avif` (Web) oder `jpg`, `png`, `webp`, `avif` (hochwertig) |
| `-d, --size` | längste Seite in Pixeln (Web, Standard 1600) |
| `-q, --quality` | 1–100 (Web, Standard 80) |
| `-n, --name` | Vorsilbe für die Dateinamen |
| `-k, --keywords` | Stichworte für Ordner ohne Stichwortzettel |
| `-s, --separator` | `dash` (bild-name), `underscore` (bild_name), `caps` (BildName) |
| `-a, --append` | alten Dateinamen mit in den neuen Namen |
| `--video-format` | `mp4` (Standard) oder `webm` |
| `--video-size` | `720`, `1080` (Standard), `original` oder die kürzere Seite in Pixeln |
| `--video-quality` | `smaller`, `balanced` (Standard), `nicer` |
| `--no-sound` / `--no-poster` | Videos ohne Ton / ohne Vorschaubild |
| `--audio-format` | `mp3` (Standard) oder `aac` |
| `--audio` | was drauf ist: `speech`, `music` (Standard), `best` |
| `-p, --preset` | gespeicherte Vorlage benutzen |
| `--only images\|videos\|audio\|fonts` | nur einen Stapel bearbeiten |
| `--all` | auch schon Umgewandeltes nochmal machen |
| `-y, --yes` | nicht fragen |
| `--json` | maschinenlesbare Ausgabe |

Weitere Befehle: `wolfgang presets`, `wolfgang settings`, `wolfgang settings set web-format avif`,
`wolfgang legacy-reset ORDNER` (siehe unten). Exit-Codes: 0 ok, 1 einzelne Dateien fehlgeschlagen,
2 falsche Bedienung.

---

## Gut zu wissen

**Dateinamen fürs Web** setzen sich zusammen aus `[Vorsilbe]-[Stichworte]-[alter Name]-[Nummer]`.
Umlaute werden zu `ae`, `oe`, `ue`, `ss`. Die Nummer zählt pro Ordner in Finder-Reihenfolge.

**Stichworte:** Liegt in einem Bilderordner eine `.txt`-Datei mit Stichworten (eins pro Zeile),
nimmt Wolfgang die. Sonst fragt er nach.

**Nochmal ablegen:** Gibst du Wolfgang denselben Ordner noch einmal, erkennt er, was schon fertig
ist, und macht auf Wunsch nur die neuen Dateien – mit fortlaufender Nummer.

**Videos und Audio** behalten ihren Namen, nur websicher: „Drohne Strand FINAL.MOV“ wird
`drohne-strand-final.mp4`, das Vorschaubild daneben `drohne-strand-final.webp`. Bei Videos
fliegen alle Metadaten raus (auch der Aufnahmeort), bei Audio bleiben Titel und Interpret drin.
WebM-Videos brauchen deutlich länger als MP4 – der Fortschrittsbalken läuft auch innerhalb
eines langen Videos weiter, und „Abbrechen“ hört sofort auf.

**Schriften** werden nach ihren eigenen Angaben benannt (`inter-700-italic.woff2`) und nach dem
Umwandeln Glyphe für Glyphe mit dem Original verglichen.

**Einstellungen und Vorlagen** liegen in `~/Library/Application Support/Wolfgang/settings.json`
und gelten für App und Kommandozeile gemeinsam.

### Vom alten Wolfgang (1.x) umsteigen

Der alte Wolfgang hat deine **Originaldateien umbenannt** (`DSC_4711.jpg` → `DSC_4711-0001.jpg`).
Das macht der neue nie. Alte Umbenennungen machst du so rückgängig:

```bash
wolfgang legacy-reset ~/Bilder/Urlaub          # zeigt erst, was passieren würde
wolfgang legacy-reset ~/Bilder/Urlaub --yes    # benennt zurück
```

Es werden nur Dateien zurückbenannt, die im alten Logbuch stehen. Der alte Bash-Wolfgang liegt in
[`legacy/`](legacy/).

---

## Für Entwickler

```bash
brew install cmake nasm pkg-config autoconf automake libtool   # nur zum Bauen der Codecs
scripts/build-ffmpeg.sh                          # Wolfgangs eigenes ffmpeg (~10 Min., nur einmal)
cargo test --workspace                           # alle Tests
cargo run --release -p wolfgang-cli -- ~/Bilder  # Kommandozeile
cargo run --release -p wolfgang-app              # App
cargo install tauri-cli --version "^2" --locked  # einmalig, für das Paket:
scripts/build-app.sh                             # → target/release/bundle/dmg/*.dmg
./install.sh                                     # nur die Kommandozeile nach /usr/local/bin
```

| Ordner | Was |
|---|---|
| `crates/wolfgang-core` | die Umwandlung: Ordner einlesen, Namen, Bilder (Apple ImageIO + mozjpeg/libwebp/rav1e), Schriften (Google woff2), Videos/Audio (ffmpeg), Logbuch, Einstellungen |
| `scripts/build-ffmpeg.sh` | baut ffmpeg + ffprobe nach `target/ffmpeg/bin` (siehe unten) |
| `crates/wolfgang-dialogue` | das Gespräch als Zustandsmaschine |
| `dialogue/de.yaml` | **alles, was Wolfgang sagt** – zum Ausprobieren ohne neu zu bauen: `WOLFGANG_DIALOGUE=dialogue/de.yaml wolfgang …` |
| `crates/wolfgang-cli` | der Befehl `wolfgang` |
| `crates/wolfgang-app` | die App (Tauri 2, Oberfläche in `ui/` als schlichtes HTML/CSS/JS) |
| `crates/wolfgang-art` | baut die Pixel-Art, siehe [`assets/pixel/README.md`](assets/pixel/README.md) |
| `assets/pixel/src` | Büro, Porträts und Sprites – von Hand gezeichnet, direkt bearbeiten |
| `PLAN.md` | der Plan und alle Entscheidungen |

Die Oberfläche lässt sich auch im normalen Browser ansehen (mit Platzhalter-Daten):
`cd crates/wolfgang-app/ui && python3 -m http.server`, dann z. B. `http://localhost:8000/?demo=confirm`.

**ffmpeg:** Wolfgang nimmt nie das ffmpeg aus Homebrew, sondern sein eigenes, das in der App
neben der Kommandozeile liegt (`./install.sh` legt es nach `/usr/local/libexec/wolfgang`).
Gebaut wird es von `scripts/build-ffmpeg.sh` mit festen Versionen und Prüfsummen, nur aus
LGPL-Teilen: Apples VideoToolbox (H.264) und AudioToolbox (AAC), libvpx (VP9), Opus, LAME (MP3)
und zimg (HDR → SDR) – kein x264/x265. Neue Versionen: Nummern und Prüfsummen im Skript ändern,
CI baut dann neu. Zum Testen mit einem anderen ffmpeg: `WOLFGANG_FFMPEG_DIR=/pfad/zum/ordner`.
Ohne gebautes ffmpeg werden die Video-/Audio-Tests übersprungen.

**Eine neue Version veröffentlichen:** Version in `Cargo.toml` (workspace) und
`crates/wolfgang-app/tauri.conf.json` anheben, committen, Tag `vX.Y.Z` pushen. GitHub baut die
`.dmg` und die signierten Update-Dateien (`Wolfgang.app.tar.gz`, `latest.json`) und legt ein
Release als Entwurf an. Sobald es veröffentlicht ist, bieten alle installierten Wolfgangs das
Update an.

**Update-Schlüssel:** Updates werden mit einem eigenen Schlüssel signiert (nicht Apple). Er liegt
als Secret `TAURI_SIGNING_PRIVATE_KEY` (+ `_PASSWORD`) im GitHub-Repo und lokal in
`~/.tauri/wolfgang.key`. **Gut aufheben:** Geht er verloren, nehmen installierte Wolfgangs keine
Updates mehr an und müssen einmal von Hand neu installiert werden. Der öffentliche Teil steht in
`tauri.conf.json`.

---

## Lizenz

MIT © k1te – siehe [LICENSE](LICENSE).
Schrift: [Pixelify Sans](https://github.com/eifetx/Pixelify-Sans), SIL Open Font License.
Videos und Audio: [FFmpeg](https://ffmpeg.org) unter der LGPL 2.1 (Lizenztext in der App unter
`Contents/Resources/LICENSE-ffmpeg.txt`, Quellen und Bauweise: `scripts/build-ffmpeg.sh`), mit
[libvpx](https://chromium.googlesource.com/webm/libvpx) (BSD), [Opus](https://opus-codec.org) (BSD),
[LAME](https://lame.sourceforge.io) (LGPL) und [zimg](https://github.com/sekrit-twc/zimg) (WTFPL).
