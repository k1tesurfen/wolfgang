#!/bin/bash

# Default values
MAX_SIZE=1400
ADD_OLD_FILENAME="n"
KEYWORDS=""
KEYWORDS_FILE=""
INPUT_PATH="."
CUSTOM_BASE="image"
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
  echo "Start converting images - Prefix: '$CUSTOM_BASE', longer image side: $MAX_SIZE px..."
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

    echo "✔ Converted: $REL_PATH → resized_jpg/$DIR_PATH/${CUSTOM_BASE}-${KEYWORDS}${FILE_BASE}.jpg & resized_webp/$DIR_PATH/${CUSTOM_BASE}-${KEYWORDS}${FILE_BASE}.webp"
  done

  echo "---------------------------------------"
  echo "✅ All images converted successfully!"

}

# Placeholder for wizard mode (define it properly)
wizard_mode() {
  echo "Welcome! Let's prepare some images for the web"
  echo "---------------------------------------"

  # Benutzer fragt nach dem Basisnamen für die Dateien
  read -rp "Enter the base name for your output files (e. g. 'converted_'): " CUSTOM_BASE
  if [[ -z "$CUSTOM_BASE" ]]; then
    echo "ERROR: Basename is required." >&2
    exit 1
  fi

  echo "---------------------------------------"
  while true; do
    read -rp "Enter pixel value for the longer side of the image (e.g. 1400): " MAX_SIZE
    if [[ "$MAX_SIZE" =~ ^[0-9]+$ ]]; then
      break
    else
      echo "ERROR: Please enter a valid number!"
    fi
  done

  echo "---------------------------------------"
  # Benutzer gibt den Pfad zur Markdown-Datei ein
  while true; do
    read -rp "Input the path to the markdown file (.md), leave empty to skip:" KEYWORD_FILE

    # Prüfen, ob die Eingabe leer ist
    if [[ -z "$KEYWORD_FILE" ]]; then
      break
    fi

    #Check if file is md file
    if [[ ! -f "$KEYWORD_FILE" || "${KEYWORD_FILE##*.}" != "md" ]]; then
      echo "ERROR: File could not be recognised as markdown file (.md)!" >&2
      continue
    fi

    break
  done

  echo "---------------------------------------"
  # Loop until the user enters 'y' or 'n'
  while true; do
    echo "Should the old filename be appended to the new one? (y/n)"
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
      echo "Please use 'Y' or 'N' to proceed."
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
  echo "  -n, --name                Custom prefix for resulting files"
  echo "  -h, --help, -man, --man   This help message"
  echo "  -d, --dimension PIXEL     Longest side in pixels (Default: 1400)"
  echo "  -k, --keywords FILE       Path to keywords file"
  echo "  -a, --append              Ursprünglichen Dateinamen anhängen"
  echo "  --debug                   Detaillierte Debug-Ausgaben aktivieren"
  echo ""
  echo "ARGUMENTS:"
  echo "  INPUT_PATH                Directory that contains the images to convert"
  echo "                            (Default: Current directory)"
  exit 0
}

# Check if no arguments are given, start wizard mode
if [[ $# -eq 0 ]]; then
  wizard_mode
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
    echo "❌ unknown option: $1"
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
  echo "❌ Too many arguments. See --help for all possible options."
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
