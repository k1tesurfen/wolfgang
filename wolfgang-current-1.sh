#!/bin/bash

# Default values
MAX_SIZE=1400
ADD_OLD_FILENAME="n"
INPUT_PATH="."
CUSTOM_BASE="image"
DEBUG="false"
WOLFGANG_SUFFIX="-wolfgang" # Define the suffix for output folders

# Debug function
debug() {
  if [[ "$DEBUG" == "true" ]]; then
    echo "[DEBUG] $*" >&2
  fi
}

# Gets the absolute path of a given existing file or directory.
# Resolves '..' and '.' components. For directories, resolves symlinks in the path using pwd -P.
# For files that are symlinks, it returns the absolute path to the symlink itself.
# $1: path to resolve (must exist)
# Output: absolute path (echoed), or empty string on error with a message to stderr.
get_abs_path() {
  local path_to_resolve="$1"
  local abs_path=""
  local dir_part
  local base_part

  if [ ! -e "$path_to_resolve" ]; then
    echo "Error: Path '$path_to_resolve' does not exist. Cannot make absolute." >&2
    echo ""
    return 1
  fi

  if [ -d "$path_to_resolve" ]; then
    # It's a directory (or a symlink to a directory)
    if ! abs_path=$(cd "$path_to_resolve" 2>/dev/null && pwd -P); then
      echo "Error: Could not determine absolute path for directory '$path_to_resolve' (cd or pwd failed)." >&2
      echo ""
      return 1
    fi
  elif [ -f "$path_to_resolve" ] || [ -L "$path_to_resolve" ]; then # It's a file or a symlink
    dir_part=$(dirname "$path_to_resolve")
    base_part=$(basename "$path_to_resolve")
    local abs_dir_part
    if ! abs_dir_part=$(cd "$dir_part" 2>/dev/null && pwd -P); then
      echo "Error: Could not determine absolute path for directory part of '$path_to_resolve' (cd or pwd failed)." >&2
      echo ""
      return 1
    fi
    abs_path="$abs_dir_part/$base_part"
  else
    # Should not be reached if initial -e check is comprehensive,
    # but as a fallback if it's some other type of existing path not handled above.
    echo "Error: Path '$path_to_resolve' is of an unsupported type for absolute path resolution." >&2
    echo ""
    return 1
  fi

  if [ -z "$abs_path" ]; then
    echo "Error: Failed to determine absolute path for '$path_to_resolve' (unexpected empty result)." >&2
    echo ""
    return 1
  fi

  echo "$abs_path"
  return 0
}

# Finds the first *.txt file alphabetically in the specified directory.
# $1: directory path
# Output: full path to the file (echoed), or empty string if not found or not a regular file.
find_first_txt_file_in_dir() {
  local dir_path="$1"
  local first_file=""

  # Find files, sort them, and take the first one.
  # Suppress "find: ...: No such file or directory" errors from find if dir_path is invalid,
  # though dir_path should be valid when this is called.
  # This assumes filenames do not contain newline characters.
  first_file=$(find "$dir_path" -maxdepth 1 -type f -name "*.txt" -print 2>/dev/null | sort | head -n 1)

  # Check if a file was actually found and is a regular file
  if [[ -n "$first_file" ]] && [[ -f "$first_file" ]]; then
    echo "$first_file"
  else
    echo "" # Return empty string if no suitable file is found
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
  local CWD_ABS # Absolute path of current working directory
  if ! CWD_ABS=$(get_abs_path "$(pwd)"); then
    echo "Critical Error: Could not determine absolute path for current working directory." >&2
    return 1
  fi
  debug "Absolute CWD: $CWD_ABS"

  # Ensure INPUT_PATH is absolute. The argument parsing already ensures it's a directory.
  local INPUT_PATH_ABS
  if ! INPUT_PATH_ABS=$(get_abs_path "$INPUT_PATH"); then
    echo "Critical Error: Could not resolve absolute path for INPUT_PATH: '$INPUT_PATH'." >&2
    return 1
  fi
  INPUT_PATH="$INPUT_PATH_ABS" # Update INPUT_PATH to its absolute form
  debug "Processing with absolute INPUT_PATH: $INPUT_PATH"

  # 1. Determine Global Keywords from CWD_ABS
  local global_keyword_file_path
  global_keyword_file_path=$(find_first_txt_file_in_dir "$CWD_ABS")

  local GLOBAL_KEYWORDS_STRING=""
  if [[ -n "$global_keyword_file_path" ]] && [[ -f "$global_keyword_file_path" ]]; then # Ensure it's a file
    debug "Found global keyword file: $global_keyword_file_path"
    GLOBAL_KEYWORDS_STRING=$(generate_keyword_string_from_file "$global_keyword_file_path")
    debug "Global keywords: $GLOBAL_KEYWORDS_STRING"
  else
    global_keyword_file_path="" # Ensure it's empty if not found or not a file
    debug "No global keyword file found in $CWD_ABS or it was not a regular file."
  fi

  # Resize and convert
  echo "Start converting images - Prefix: '$CUSTOM_BASE', longer image side: $MAX_SIZE px..."
  if [[ -n "$GLOBAL_KEYWORDS_STRING" ]]; then
    echo "Global keywords active: $GLOBAL_KEYWORDS_STRING"
  fi
  echo "---------------------------------------"

  local folder_timestamp_base
  folder_timestamp_base=$(date +"%Y-%m-%d_%H-%M-%S")
  local output_folder_name="${folder_timestamp_base}${WOLFGANG_SUFFIX}"

  # Create the main timestamped output folder relative to CWD_ABS
  # Output folder will be in the directory where the script was run.
  local actual_output_base_dir="$CWD_ABS/$output_folder_name"

  mkdir -p "$actual_output_base_dir"
  debug "Created main output folder: $actual_output_base_dir (name includes $WOLFGANG_SUFFIX)"

  mkdir -p "$actual_output_base_dir/resized_jpg"
  mkdir -p "$actual_output_base_dir/resized_webp"
  debug "Created subdirectory: $actual_output_base_dir/resized_jpg"
  debug "Created subdirectory: $actual_output_base_dir/resized_webp"

  local find_cmd_array=()
  find_cmd_array+=("find")
  find_cmd_array+=("$INPUT_PATH")

  # Group 1: Prune any directory ending with WOLFGANG_SUFFIX (e.g., "*-wolfgang")
  find_cmd_array+=("(")
  find_cmd_array+=("-type")
  find_cmd_array+=("d")
  find_cmd_array+=("-name")
  find_cmd_array+=("*${WOLFGANG_SUFFIX}")
  find_cmd_array+=("-prune")
  find_cmd_array+=(")")
  debug "Added pruning for directories named: *${WOLFGANG_SUFFIX}"

  # OR operator: if not pruned, then proceed with other conditions
  find_cmd_array+=("-o")

  # Group 2: Finding files of specific types
  find_cmd_array+=("(") # Start of file finding group
  find_cmd_array+=("-type")
  find_cmd_array+=("f")
  # Sub-group for extensions
  find_cmd_array+=("(") # Start of extension group
  find_cmd_array+=("-iname")
  find_cmd_array+=("*.jpg")
  find_cmd_array+=("-o")
  find_cmd_array+=("-iname")
  find_cmd_array+=("*.jpeg")
  find_cmd_array+=("-o")
  find_cmd_array+=("-iname")
  find_cmd_array+=("*.png")
  find_cmd_array+=("-o")
  find_cmd_array+=("-iname")
  find_cmd_array+=("*.gif")
  find_cmd_array+=("-o")
  find_cmd_array+=("-iname")
  find_cmd_array+=("*.tif")
  find_cmd_array+=("-o")
  find_cmd_array+=("-iname")
  find_cmd_array+=("*.tiff")
  find_cmd_array+=(")")      # End of extension group
  find_cmd_array+=("-print") # Print the found file path
  find_cmd_array+=(")")      # End of file finding group

  debug "Find command: ${find_cmd_array[*]}"

  "${find_cmd_array[@]}" | while IFS= read -r img_raw; do
    # Resolve img_raw to an absolute path because 'find' might return relative paths (e.g. if INPUT_PATH was ".")
    local img
    if ! img=$(get_abs_path "$img_raw"); then
      echo "⚠️ Could not get absolute path for '$img_raw'. Skipping." >&2
      continue
    fi
    debug "Processing (absolute path): $img"

    # Double check it's not inside a wolfgang-generated directory (should be caught by find -prune, but as a safeguard)
    if [[ "$img" == *"${WOLFGANG_SUFFIX}"/* ]]; then
      debug "Skipping image '$img' as it appears to be inside an already processed '${WOLFGANG_SUFFIX}' directory (safeguard)."
      continue
    fi

    if ! file --mime-type "$img" | grep -qE 'image/'; then
      debug "Skipping non-image file: $img (MIME type check)"
      continue
    fi

    # REL_PATH is relative to the original INPUT_PATH for output structure
    local REL_PATH
    # Ensure INPUT_PATH has a trailing slash for robust prefix removal, unless it's "/"
    local temp_input_path_for_rel="$INPUT_PATH"
    if [[ "$temp_input_path_for_rel" != "/" ]]; then
      temp_input_path_for_rel+="/"
    fi

    if [[ "$img" == "$temp_input_path_for_rel"* ]]; then
      REL_PATH="${img#$temp_input_path_for_rel}"
    elif [[ "$img" == "$INPUT_PATH/"* ]]; then # Fallback for root or similar edge cases
      REL_PATH="${img#$INPUT_PATH/}"
    else
      REL_PATH=$(basename "$img")
      debug "Warning: Image '$img' not directly under '$INPUT_PATH'. Using basename for REL_PATH: '$REL_PATH'"
    fi

    local DIR_PATH_RAW
    DIR_PATH_RAW=$(dirname "$REL_PATH")

    local current_img_dir             # Absolute path to current image's directory
    current_img_dir=$(dirname "$img") # $img is already absolute

    local DIR_PATH_FOR_OUTPUT # Relative path for output structure
    if [[ "$DIR_PATH_RAW" == "." ]]; then
      DIR_PATH_FOR_OUTPUT=""
    else
      DIR_PATH_FOR_OUTPUT="$DIR_PATH_RAW/"
    fi

    local FILE_NAME
    FILE_NAME=$(basename "$img")
    local ORIGINAL_FILE_BASE="${FILE_NAME%.*}"

    mkdir -p "$actual_output_base_dir/resized_jpg/${DIR_PATH_FOR_OUTPUT}"
    mkdir -p "$actual_output_base_dir/resized_webp/${DIR_PATH_FOR_OUTPUT}"

    # 2. Determine Local Keywords for the current image's directory
    local local_keyword_file_path
    local_keyword_file_path=$(find_first_txt_file_in_dir "$current_img_dir")

    local LOCAL_KEYWORDS_STRING=""
    if [[ -n "$local_keyword_file_path" ]] && [[ -f "$local_keyword_file_path" ]]; then
      debug "Found potential local keyword file: $local_keyword_file_path for image $FILE_NAME in $current_img_dir"
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

    local width height
    local identify_output
    identify_output=$(identify -format "%w %h" "$img" 2>/dev/null)
    read -r width height <<<"$identify_output"

    if [[ -z "$width" || -z "$height" || ! "$width" =~ ^[0-9]+$ || ! "$height" =~ ^[0-9]+$ ]]; then
      echo "⚠️ Could not get valid dimensions for $img (width='$width', height='$height'). Skipping." >&2
      continue
    fi

    local FILETIMESTAMP
    FILETIMESTAMP=$(date +"%H%M%S")

    local new_filename_parts=()
    new_filename_parts+=("$CUSTOM_BASE")
    [[ -n "$combined_image_keywords" ]] && new_filename_parts+=("$combined_image_keywords")
    new_filename_parts+=("$FILETIMESTAMP")
    [[ "$ADD_OLD_FILENAME" == "y" && -n "$ORIGINAL_FILE_BASE" ]] && new_filename_parts+=("$ORIGINAL_FILE_BASE")

    local new_filename_base
    new_filename_base=$(
      IFS='-'
      echo "${new_filename_parts[*]}"
    )
    new_filename_base=$(echo "$new_filename_base" | sed 's/--\+/-/g; s/^-//; s/-$//')

    local convert_cmd_args=()
    if [ "$width" -gt "$height" ]; then
      convert_cmd_args+=("-resize" "${MAX_SIZE}x>")
    else
      convert_cmd_args+=("-resize" "x${MAX_SIZE}>")
    fi
    convert_cmd_args+=("-quality" "90")

    local output_jpg_path="$actual_output_base_dir/resized_jpg/${DIR_PATH_FOR_OUTPUT}${new_filename_base}.jpg"
    local output_webp_path="$actual_output_base_dir/resized_webp/${DIR_PATH_FOR_OUTPUT}${new_filename_base}.webp"

    debug "Converting '$img' to '$output_jpg_path' with args: ${convert_cmd_args[*]}"
    if magick "$img" "${convert_cmd_args[@]}" "$output_jpg_path"; then
      debug "Successfully converted '$img' to '$output_jpg_path'"
    else
      echo "❌ Error converting '$img' to JPG. magick exit code: $?" >&2
    fi

    debug "Converting '$img' to '$output_webp_path' with args: ${convert_cmd_args[*]}"
    if magick "$img" "${convert_cmd_args[@]}" "$output_webp_path"; then
      debug "Successfully converted '$img' to '$output_webp_path'"
    else
      echo "❌ Error converting '$img' to WebP. magick exit code: $?" >&2
    fi

    local display_output_jpg_path="${output_jpg_path#$actual_output_base_dir/}"
    local display_output_webp_path="${output_webp_path#$actual_output_base_dir/}"
    echo "✔ Converted: $REL_PATH → $display_output_jpg_path & $display_output_webp_path"
  done

  echo "---------------------------------------"
  echo "✅ All images converted successfully! Output is in '$actual_output_base_dir' directory."
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
    if [[ "$MAX_SIZE" =~ ^[0-9]+$ && "$MAX_SIZE" -gt 0 ]]; then
      break
    else
      echo "ERROR: Please enter a valid positive number!"
    fi
  done

  echo "---------------------------------------"
  echo "Keywords will be automatically detected:"
  echo "  - Global keywords: From the first *.txt file (alphabetically) in the current directory (where script is run)."
  echo "  - Local keywords: From the first *.txt file (alphabetically) in each image's own directory."
  echo "Output directories will be named 'YYYY-MM-DD_HH-MM${WOLFGANG_SUFFIX}' to prevent reprocessing."
  echo "---------------------------------------"

  while true; do
    echo "Should the old filename be appended to the new one? (y/n)"
    read -n 1 -r ADD_OLD_FILENAME_INPUT
    echo
    case "$ADD_OLD_FILENAME_INPUT" in
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

  echo "---------------------------------------"
  while true; do
    read -rp "Enter the input directory containing images (Default: current directory '.'): " INPUT_PATH_WIZARD
    INPUT_PATH_WIZARD=${INPUT_PATH_WIZARD:-.}

    local temp_abs_path
    if ! temp_abs_path=$(get_abs_path "$INPUT_PATH_WIZARD"); then
      echo "Error: Input path '$INPUT_PATH_WIZARD' is not accessible or does not exist." >&2
      continue
    fi
    if [ ! -d "$temp_abs_path" ]; then
      echo "Error: Input path '$temp_abs_path' is not a directory." >&2
      continue
    fi
    INPUT_PATH="$INPUT_PATH_WIZARD"
    break
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
  echo "  -n, --name BASENAME      Custom prefix for resulting files (Default: '$CUSTOM_BASE')"
  echo "  -h, --help, -man, --man  This help message"
  echo "  -d, --dimension PIXEL    Longest side in pixels (Default: $MAX_SIZE)"
  echo "  -a, --append             Append the original filename (without extension) to the new name."
  echo "      --debug              Enable detailed debug output."
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
  echo "OUTPUT DIRECTORY NAMING:"
  echo "  Output directories are created in the current working directory and named using the pattern:"
  echo "  'YYYY-MM-DD_HH-MM${WOLFGANG_SUFFIX}' (e.g., 2023-10-27_15-30${WOLFGANG_SUFFIX})."
  echo "  This naming convention is used to prevent the script from reprocessing images within these directories."
  echo ""
  echo "ARGUMENTS:"
  echo "  INPUT_PATH               Directory that contains the images to convert"
  echo "                           (Default: Current directory './')"
  exit 0
}

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
      show_help
      exit 1
    fi
    if ! [[ "$2" =~ ^[0-9]+$ && "$2" -gt 0 ]]; then
      echo "❌ Error: Dimension value for $1 must be a positive integer. Got: '$2'" >&2
      show_help
      exit 1
    fi
    MAX_SIZE="$2"
    shift 2
    ;;
  -n | --name)
    if [[ -z "$2" || "$2" == -* ]]; then
      echo "❌ Error: Missing value for $1 option." >&2
      show_help
      exit 1
    fi
    CUSTOM_BASE="$2"
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
  echo "❌ Too many arguments. Only one INPUT_PATH is allowed. See --help." >&2
  show_help
  exit 1
elif [[ ${#POSITIONAL_ARGS[@]} -eq 1 ]]; then
  INPUT_PATH="${POSITIONAL_ARGS[0]}"
fi

resolved_input_path=$(get_abs_path "$INPUT_PATH")
if [[ -z "$resolved_input_path" ]]; then
  echo "❌ Error: Input path '$INPUT_PATH' could not be resolved or does not exist." >&2
  show_help
  exit 1
fi
if [[ ! -d "$resolved_input_path" ]]; then
  echo "❌ Error: Resolved input path '$resolved_input_path' is not a directory." >&2
  show_help
  exit 1
fi
INPUT_PATH="$resolved_input_path"

debug "Final Configuration:"
debug "  MAX_SIZE: $MAX_SIZE"
debug "  ADD_OLD_FILENAME: $ADD_OLD_FILENAME"
debug "  INPUT_PATH: $INPUT_PATH"
debug "  CUSTOM_BASE: $CUSTOM_BASE"
debug "  DEBUG: $DEBUG"
debug "  CWD for global keywords (and output): $(get_abs_path "$(pwd)")"
debug "  Output folder suffix: $WOLFGANG_SUFFIX"

if ! convert; then
  echo "An error occurred during conversion." >&2
  exit 1
fi

exit 0
