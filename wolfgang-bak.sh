#!/bin/bash

# Default values
MAX_SIZE=1400
ADD_OLD_FILENAME="n"
# KEYWORDS="" # Removed, will be dynamic
# KEYWORDS_FILE="" # Removed
INPUT_PATH="."
CUSTOM_BASE="image"
DEBUG="false"

# Debug function
debug() {
  if [[ "$DEBUG" == "true" ]]; then
    echo "[DEBUG] $*" >&2
  fi
}

# Reads keywords from a file, one per line, trims them, and returns a hyphen-separated string.
# $1: path to the keyword file
# Output: hyphen-separated keyword string (echoed)
generate_keyword_string_from_file() {
  local file_path="$1"
  local keywords_output=""
  if [ -f "$file_path" ] && [ -r "$file_path" ]; then
    while IFS= read -r line; do
      # Trim leading/trailing whitespace from the line
      trimmed_line=$(echo "$line" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')
      if [ -n "$trimmed_line" ]; then
        keywords_output+="$trimmed_line-"
      fi
    done <"$file_path"
    keywords_output=${keywords_output%?} # Remove trailing hyphen if any keywords were added
  fi
  echo "$keywords_output"
}

convert() {
  # Ensure input path is absolute
  INPUT_PATH=$(readlink -f "$INPUT_PATH")
  local CWD
  CWD=$(pwd) # Store current working directory

  # 1. Determine Global Keywords from CWD
  local global_keyword_file_path
  # Find the first .txt file alphabetically in the CWD
  global_keyword_file_path=$(find "$CWD" -maxdepth 1 -type f -name "*.txt" -print0 | xargs -0 -r ls | head -n 1)
  # If using a system without -print0 or xargs -0 -r, a safer find might be:
  # global_keyword_file_path=$(find "$CWD" -maxdepth 1 -type f -name "*.txt" -exec basename {} \; | sort | head -n 1)
  # if [[ -n "$global_keyword_file_path" ]]; then global_keyword_file_path="$CWD/$global_keyword_file_path"; fi

  local GLOBAL_KEYWORDS_STRING=""
  if [[ -n "$global_keyword_file_path" ]] && [[ -f "$global_keyword_file_path" ]]; then # Ensure it's a file
    debug "Found global keyword file: $global_keyword_file_path"
    GLOBAL_KEYWORDS_STRING=$(generate_keyword_string_from_file "$global_keyword_file_path")
    debug "Global keywords: $GLOBAL_KEYWORDS_STRING"
  else
    global_keyword_file_path="" # Ensure it's empty if not found or not a file
    debug "No global keyword file found in $CWD or it was not a regular file."
  fi

  # Resize and convert
  echo "Start converting images - Prefix: '$CUSTOM_BASE', longer image side: $MAX_SIZE px..."
  if [[ -n "$GLOBAL_KEYWORDS_STRING" ]]; then
    echo "Global keywords active: $GLOBAL_KEYWORDS_STRING"
  fi
  echo "---------------------------------------"

  FOLDERTIMESTAMP=$(date +"%Y-%m-%d_%H-%M")

  # Create the main timestamped output folder
  mkdir -p "$FOLDERTIMESTAMP"
  debug "Created main output folder: $FOLDERTIMESTAMP"

  local output_base_dir="$FOLDERTIMESTAMP"
  mkdir -p "$output_base_dir/resized_jpg"
  mkdir -p "$output_base_dir/resized_webp"
  debug "Created subdirectory: $output_base_dir/resized_jpg"
  debug "Created subdirectory: $output_base_dir/resized_webp"

  find "$INPUT_PATH" -path "$INPUT_PATH/$FOLDERTIMESTAMP" -prune -o \
    -type f \( -iname "*.jpg" -o -iname "*.jpeg" -o -iname "*.png" -o -iname "*.gif" -o -iname "*.tif" -o -iname "*.tiff" \) | while read -r img; do
    if ! file --mime-type "$img" | grep -qE 'image/'; then
      debug "Skipping non-image file: $img"
      continue
    fi

    REL_PATH="${img#$INPUT_PATH/}"
    DIR_PATH_RAW=$(dirname "$REL_PATH")
    local current_img_dir # Absolute path to current image's directory
    current_img_dir=$(dirname "$(readlink -f "$img")")

    local DIR_PATH_FOR_OUTPUT # Relative path for output structure
    if [[ "$DIR_PATH_RAW" == "." ]]; then
      DIR_PATH_FOR_OUTPUT=""
    else
      DIR_PATH_FOR_OUTPUT="$DIR_PATH_RAW/"
    fi

    FILE_NAME="${img##*/}"
    ORIGINAL_FILE_BASE="${FILE_NAME%.*}"

    local current_file_base=""
    if [[ "$ADD_OLD_FILENAME" == "y" ]]; then
      current_file_base="$ORIGINAL_FILE_BASE"
    fi

    mkdir -p "$output_base_dir/resized_jpg/${DIR_PATH_FOR_OUTPUT}"
    mkdir -p "$output_base_dir/resized_webp/${DIR_PATH_FOR_OUTPUT}"

    # 2. Determine Local Keywords for the current image's directory
    local local_keyword_file_path
    local_keyword_file_path=$(find "$current_img_dir" -maxdepth 1 -type f -name "*.txt" -print0 | xargs -0 -r ls | head -n 1)
    # if [[ -n "$local_keyword_file_path" ]]; then local_keyword_file_path="$current_img_dir/$local_keyword_file_path"; fi

    local LOCAL_KEYWORDS_STRING=""
    if [[ -n "$local_keyword_file_path" ]] && [[ -f "$local_keyword_file_path" ]]; then
      debug "Found potential local keyword file: $local_keyword_file_path for image $FILE_NAME in $current_img_dir"
      # Check if this local file is the same as the global file already processed
      if [[ -n "$global_keyword_file_path" && "$local_keyword_file_path" == "$global_keyword_file_path" ]]; then
        debug "Local keyword file '$local_keyword_file_path' is the same as the global keyword file. Not adding as separate local keywords."
        LOCAL_KEYWORDS_STRING=""
      else
        LOCAL_KEYWORDS_STRING=$(generate_keyword_string_from_file "$local_keyword_file_path")
        debug "Local keywords for $current_img_dir: $LOCAL_KEYWORDS_STRING"
      fi
    else
      debug "No local keyword file found in $current_img_dir for $FILE_NAME or it was not a regular file."
    fi

    # 3. Combine Global and Local Keywords
    local combined_image_keywords=""
    if [[ -n "$GLOBAL_KEYWORDS_STRING" ]]; then
      combined_image_keywords="$GLOBAL_KEYWORDS_STRING"
    fi
    if [[ -n "$LOCAL_KEYWORDS_STRING" ]]; then
      if [[ -n "$combined_image_keywords" ]]; then
        combined_image_keywords+="-$LOCAL_KEYWORDS_STRING"
      else
        combined_image_keywords="$LOCAL_KEYWORDS_STRING"
      fi
    fi

    width=$(identify -format "%w" "$img")
    height=$(identify -format "%h" "$img")

    if [[ -z "$width" || -z "$height" ]]; then
      echo "⚠️ Could not get dimensions for $img. Skipping."
      continue
    fi

    FILETIMESTAMP=$(date +"%H%M%S")

    local new_filename_parts=()
    new_filename_parts+=("$CUSTOM_BASE")
    # Only add combined_image_keywords if it's not empty
    [[ -n "$combined_image_keywords" ]] && new_filename_parts+=("$combined_image_keywords")
    new_filename_parts+=("$FILETIMESTAMP")
    [[ "$ADD_OLD_FILENAME" == "y" && -n "$ORIGINAL_FILE_BASE" ]] && new_filename_parts+=("$ORIGINAL_FILE_BASE")

    local new_filename_base
    new_filename_base=$(
      IFS='-'
      echo "${new_filename_parts[*]}"
    )
    # Clean up potential multiple hyphens or leading/trailing hyphens from empty parts
    new_filename_base=$(echo "$new_filename_base" | sed 's/--\+/-/g; s/^-//; s/-$//')

    local convert_cmd
    if [ "$width" -gt "$height" ]; then
      convert_cmd="-resize ${MAX_SIZE}x\> -quality 90"
    else
      convert_cmd="-resize x${MAX_SIZE}\> -quality 90"
    fi

    local output_jpg_path="$output_base_dir/resized_jpg/${DIR_PATH_FOR_OUTPUT}${new_filename_base}.jpg"
    local output_webp_path="$output_base_dir/resized_webp/${DIR_PATH_FOR_OUTPUT}${new_filename_base}.webp"

    debug "Converting '$img' to '$output_jpg_path'"
    if magick "$img" $convert_cmd "$output_jpg_path"; then
      debug "Successfully converted '$img' to '$output_jpg_path'"
    else
      echo "❌ Error converting '$img' to JPG. magick exit code: $?" >&2
    fi

    debug "Converting '$img' to '$output_webp_path'"
    if magick "$img" $convert_cmd "$output_webp_path"; then
      debug "Successfully converted '$img' to '$output_webp_path'"
    else
      echo "❌ Error converting '$img' to WebP. magick exit code: $?" >&2
    fi

    echo "✔ Converted: $REL_PATH → ${output_jpg_path#$output_base_dir/} & ${output_webp_path#$output_base_dir/}"
  done

  echo "---------------------------------------"
  echo "✅ All images converted successfully! Output is in '$FOLDERTIMESTAMP' directory."
}

wizard_mode() {
  echo "Welcome! Let's prepare some images for the web"
  echo "---------------------------------------"

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

  # Removed prompt for KEYWORDS_FILE
  echo "---------------------------------------"
  echo "Keywords will be automatically detected:"
  echo "  - Global keywords: From the first *.txt file (alphabetically) in the current directory."
  echo "  - Local keywords: From the first *.txt file (alphabetically) in each image's own directory."
  echo "---------------------------------------"

  while true; do
    echo "Should the old filename be appended to the new one? (y/n)"
    read -n 1 -r ADD_OLD_FILENAME
    echo
    case "$ADD_OLD_FILENAME" in
    [Yy])
      ADD_OLD_FILENAME="y"
      break
      ;;
    [Nn])
      ADD_OLD_FILENAME="n"
      break
      ;;
    *) echo "Please use 'Y' or 'N' to proceed." ;;
    esac
  done

  convert
  exit 0
}

show_help() {
  echo "WOLFGANG - Web Optimized Lightweight Fast Graphics Analyser and Generator"
  echo ""
  echo "USAGE: wolfgang [OPTIONS] [INPUT_PATH]"
  echo ""
  echo "OPTIONS:"
  echo "  -n, --name BASENAME        Custom prefix for resulting files (Default: '$CUSTOM_BASE')"
  echo "  -h, --help, -man, --man    This help message"
  echo "  -d, --dimension PIXEL      Longest side in pixels (Default: $MAX_SIZE)"
  # echo "  -k, --keywords FILE        Path to keywords file (one keyword per line)" # Removed
  echo "  -a, --append               Append the original filename (without extension) to the new name."
  echo "  --debug                    Enable detailed debug output."
  echo ""
  echo "KEYWORD DETECTION:"
  echo "  Keywords are automatically detected from '*.txt' files:"
  echo "  1. Global Keywords: The first alphabetical '*.txt' file found in the directory"
  echo "     where the script is run provides global keywords for all images."
  echo "  2. Local Keywords: For each image, the first alphabetical '*.txt' file in its"
  echo "     own directory provides local keywords for that image and others in the same directory."
  echo "     (If this file is the same as the global keyword file, it's not re-added)."
  echo "  Keywords within these files should be one per line."
  echo ""
  echo "ARGUMENTS:"
  echo "  INPUT_PATH                 Directory that contains the images to convert"
  echo "                             (Default: Current directory)"
  exit 0
}

# Check if no arguments are given, start wizard mode
if [[ $# -eq 0 ]]; then
  wizard_mode
fi

POSITIONAL_ARGS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
  -h | --help | -man | --man) show_help ;;
  -d | --dimension)
    if [[ -z "$2" || "$2" == -* ]]; then
      echo "❌ Error: Missing value for $1 option." >&2
      exit 1
    fi
    if ! [[ "$2" =~ ^[0-9]+$ ]]; then
      echo "❌ Error: Dimension value for $1 must be a positive integer. Got: '$2'" >&2
      exit 1
    fi
    MAX_SIZE="$2"
    shift 2
    ;;
  -n | --name)
    if [[ -z "$2" || "$2" == -* ]]; then
      echo "❌ Error: Missing value for $1 option." >&2
      exit 1
    fi
    CUSTOM_BASE="$2"
    shift 2
    ;;
  # -k | --keywords) # Removed
  #   if [[ -z "$2" || "$2" == -* ]]; then echo "❌ Error: Missing value for $1 option." >&2; exit 1; fi
  #   KEYWORDS_FILE="$2" # Removed
  #   if [[ ! -f "$KEYWORDS_FILE" || ! -r "$KEYWORDS_FILE" ]]; then echo "❌ Error: Keywords file '$KEYWORDS_FILE' not found or not readable." >&2; exit 1; fi # Removed
  #   shift 2 ;; # Removed
  -a | --append)
    ADD_OLD_FILENAME="y"
    shift
    ;;
  --debug)
    DEBUG="true"
    shift
    ;;
  -*)
    echo "❌ Unknown option: $1"
    show_help
    exit 1
    ;;
  *)
    POSITIONAL_ARGS+=("$1")
    shift
    ;;
  esac
done

set -- "${POSITIONAL_ARGS[@]}"
if [[ ${#POSITIONAL_ARGS[@]} -gt 1 ]]; then
  echo "❌ Too many arguments. Only one INPUT_PATH is allowed. See --help."
  exit 1
elif [[ ${#POSITIONAL_ARGS[@]} -eq 1 ]]; then
  INPUT_PATH="${POSITIONAL_ARGS[0]}"
  if [[ ! -d "$INPUT_PATH" ]]; then
    echo "❌ Error: Input path '$INPUT_PATH' is not a directory or does not exist." >&2
    exit 1
  fi
fi

debug "Final Configuration:"
debug "  MAX_SIZE: $MAX_SIZE"
# debug "  KEYWORDS_FILE: $KEYWORDS_FILE" # Removed
debug "  ADD_OLD_FILENAME: $ADD_OLD_FILENAME"
debug "  INPUT_PATH: $INPUT_PATH"
debug "  CUSTOM_BASE: $CUSTOM_BASE"
debug "  DEBUG: $DEBUG"
debug "  CWD for global keywords: $(pwd)"

convert
