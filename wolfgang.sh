#!/bin/bash

# Default values
MAX_SIZE=1400
ADD_OLD_FILENAME="n"
KEYWORDS=""
KEYWORDS_FILE=""
INPUT_PATH="."
CUSTOM_BASE="wb"
DEBUG="false"

# Debug function
debug() {
  if [[ "$DEBUG" == "true" ]]; then
    echo "[DEBUG] $*" >&2
  fi
}
# Process keywords if file is provided
process_keywords() {
  local keywords_file="$1"
  KEYWORDS=""
  if [ -f "$keywords_file" ] && [ -r "$keywords_file" ]; then
    while IFS= read -r line; do
      trimmed=$(echo "$line" | xargs)
      if [ -n "$trimmed" ]; then
        KEYWORDS+="$trimmed-"
      fi
    done <"$keywords_file"
    KEYWORDS=${KEYWORDS%?} # Remove trailing hyphen
  fi
}

convert() {
  # Ensure input path is absolute
  INPUT_PATH=$(readlink -f "$INPUT_PATH")

  # Process keywords if file is provided
  if [[ -n "$KEYWORDS_FILE" ]]; then
    process_keywords "$KEYWORDS_FILE"
  fi

  # Resize and convert
  echo "Verarbeitung startet mit Basisnamen '$CUSTOM_BASE' und maximaler Seitenlänge von $MAX_SIZE Pixel..."
  echo "---------------------------------------"

  # Ensure output directories exist
  mkdir -p resized_jpg resized_webp

  # Find and process images
  find "$INPUT_PATH" \( -path "$INPUT_PATH/resized_jpg" -o -path "$INPUT_PATH/resized_webp" \) -prune -o \-type f \( -iname "*.jpg" -o -iname "*.jpeg" -o -iname "*.png" -o -iname "*.gif" -o -iname "*.tif" \) | while read -r img; do
    # Validate image
    if ! file --mime-type "$img" | grep -qE 'image/'; then
      continue
    fi

    # Prepare filenames
    REL_PATH="${img#$INPUT_PATH/}"
    DIR_PATH=$(dirname "$REL_PATH")
    FILE_NAME="${img##*/}"

    if [[ "$ADD_OLD_FILENAME" == "y" ]]; then
      FILE_BASE="${FILE_NAME%.*}"
    else
      FILE_BASE=""
    fi

    # Create output directories
    mkdir -p "resized_jpg/$DIR_PATH"
    mkdir -p "resized_webp/$DIR_PATH"

    # Determine image dimensions
    width=$(identify -format "%w" "$img")
    height=$(identify -format "%h" "$img")

    # Generate timestamp (hhmmss)
    TIMESTAMP=$(date +"%H%M%S")

    # Resize command based on aspect ratio
    if [ "$width" -gt "$height" ]; then
      convert_cmd="-resize ${MAX_SIZE}x\> -quality 90"
    else
      convert_cmd="-resize x${MAX_SIZE}\> -quality 90"
    fi

    # Convert images
    magick "$img" $convert_cmd "resized_jpg/$DIR_PATH/${CUSTOM_BASE}-${KEYWORDS}-${TIMESTAMP}-${FILE_BASE}.jpg"
    magick "$img" $convert_cmd "resized_webp/$DIR_PATH/${CUSTOM_BASE}-${KEYWORDS}-${TIMESTAMP}-${FILE_BASE}.webp"

    echo "✔ Verarbeitet: $REL_PATH → resized_jpg/$DIR_PATH/${CUSTOM_BASE}-${KEYWORDS}${FILE_BASE}.jpg & resized_webp/$DIR_PATH/${CUSTOM_BASE}-${KEYWORDS}${FILE_BASE}.webp"
  done

  echo "---------------------------------------"
  echo "✅ Alle Bilder wurden erfolgreich verarbeitet!"

}

# Placeholder for wizard mode (define it properly)
wizard_mode() {
  echo "Willkommen zum Bildoptimierer für's Web!"
  echo "---------------------------------------"

  # Benutzer fragt nach dem Basisnamen für die Dateien
  read -rp "Gib den Basisnamen für die Ausgabedateien ein (z. B. 'artismedia'): " CUSTOM_BASE
  if [[ -z "$CUSTOM_BASE" ]]; then
    echo "Fehler: Basisname darf nicht leer sein!" >&2
    exit 1
  fi

  # Benutzer fragt nach der maximalen Seitenlänge (nur Zahlen erlauben)
  echo "---------------------------------------"
  while true; do
    read -rp "Gib nun die maximale Seitenlänge(quer oder hoch) der Bilder in Pixel ein (z. B. 1400): " MAX_SIZE
    if [[ "$MAX_SIZE" =~ ^[0-9]+$ ]]; then
      break
    else
      echo "Fehler: Bitte geben Sie eine gültige Zahl ein!"
    fi
  done

  echo "---------------------------------------"
  # Benutzer gibt den Pfad zur Markdown-Datei ein
  while true; do
    read -rp "Gib den Pfad zur Keywords-Datei (.md) ein, leer lassen falls unerwünscht:" KEYWORD_FILE

    # Prüfen, ob die Eingabe leer ist
    if [[ -z "$KEYWORD_FILE" ]]; then
      break
    fi

    # Prüfen, ob die Datei existiert und eine .md Datei ist
    if [[ ! -f "$KEYWORD_FILE" || "${KEYWORD_FILE##*.}" != "md" ]]; then
      echo "Fehler: Die Datei existiert nicht oder ist keine Markdown-Datei (.md)!" >&2
      continue
    fi

    # Falls alles passt, aus der Schleife ausbrechen
    break
  done

  echo "---------------------------------------"
  # Loop until the user enters 'y' or 'n'
  while true; do
    echo "Soll der alte Dateiname am Ende des neuen Dateinamens eingefügt werden? (y/n)"
    read -n 1 ADD_OLD_FILENAME # Read one character without waiting for Enter
    echo                       # Print a newline

    case "$ADD_OLD_FILENAME" in
    [Yy])
      break
      ;;
    [Nn])
      break
      ;;
    *)
      echo "Ungültige Eingabe. Bitte 'Y' oder 'N' eingeben."
      ;;
    esac
  done

  convert
  exit 0
}

# Help function
show_help() {
  echo "WOLFGANG - Web Optimized Lightweight Fast Graphics Analyser and Generator"
  echo ""
  echo "USAGE: wolfgang [OPTIONS] [INPUT_PATH]"
  echo ""
  echo "OPTIONS:"
  echo "  -n, --name                Custom Präfix für Dateinamen"
  echo "  -h, --help, -man, --man   Hilfe-Nachricht anzeigen"
  echo "  -d, --dimension PIXEL     Maximale Seitenlänge in Pixel (Standard: 1400)"
  echo "  -k, --keywords FILE       Pfad zur Keyword Datei."
  echo "                            (Falls Keyword Datei in deinem aktuellen Verzeichnis ist,"
  echo "                            dann reicht einfach nur dateiname.md)"
  echo "  -a, --append              Ursprünglichen Dateinamen anhängen"
  echo "  --debug                   Detaillierte Debug-Ausgaben aktivieren"
  echo ""
  echo "ARGUMENTE:"
  echo "  INPUT_PATH                Verzeichnis mit zu optimierenden Bildern"
  echo "                            (Standard: Aktuelles Verzeichnis)"
  exit 0
}

# Check if no arguments are given, start wizard mode
if [[ $# -eq 0 ]]; then
  wizard_mode
  exit 0
fi

# Positional arguments array
POSITIONAL_ARGS=()

# Parse arguments
while [[ $# -gt 0 ]]; do
  case "$1" in
  -h | --help | -man | --man)
    show_help
    ;;
  -d | --dimension)
    MAX_SIZE="$2"
    shift 2
    ;;
  -n | --name)
    CUSTOM_BASE="$2"
    shift 2
    ;;
  -k | --keywords)
    KEYWORDS_FILE="$2"
    shift 2
    ;;
  -a | --append)
    ADD_OLD_FILENAME="y"
    shift
    ;;
  --debug)
    DEBUG="true"
    shift
    ;;
  -*)
    echo "❌ Unbekannte Option: $1"
    exit 1
    ;;
  *)
    POSITIONAL_ARGS+=("$1")
    shift
    ;;
  esac
done

# Restore positional parameters
set -- "${POSITIONAL_ARGS[@]}"

# Handle input path
if [[ ${#POSITIONAL_ARGS[@]} -gt 1 ]]; then
  echo "❌ Zu viele Argumente. Verwenden Sie --help für Nutzungsinformationen."
  exit 1
elif [[ ${#POSITIONAL_ARGS[@]} -eq 1 ]]; then
  INPUT_PATH="${POSITIONAL_ARGS[0]}"
fi

# Debug final configuration
debug "Final Configuration:"
debug "  MAX_SIZE: $MAX_SIZE"
debug "  KEYWORDS_FILE: $KEYWORDS_FILE"
debug "  ADD_OLD_FILENAME: $ADD_OLD_FILENAME"
debug "  INPUT_PATH: $INPUT_PATH"
debug "  CUSTOM_BASE: $CUSTOM_BASE"
debug "  DEBUG: $DEBUG"

convert
