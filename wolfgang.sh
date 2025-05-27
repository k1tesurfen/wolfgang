#!/bin/bash

# Default values
MAX_SIZE=1400
ADD_OLD_FILENAME="n"
INPUT_PATH="."
CUSTOM_BASE="image"
DEBUG="false"
RESET_LOG_REQUESTED="false" # Flag for the new reset option

# Debug function
debug() {
  if [[ "$DEBUG" == "true" ]]; then
    echo "[DEBUG] $*" >&2
  fi
}

# Reads keywords from a file. Each line is a keyword.
# Words on a single line are joined by underscores.
# Returns a hyphen-separated string of all keywords.
# $1: path to the keyword file
# Output: hyphen-separated keyword string (echoed)
generate_keyword_string_from_file() {
  local file_path="$1"
  local keywords_output=""

  if ! [ -f "$file_path" ] || ! [ -r "$file_path" ]; then
    debug "Keyword file not found or not readable: $file_path"
    echo ""
    return
  fi

  debug "Processing keyword file: $file_path"
  while IFS= read -r line; do
    local trimmed_line
    trimmed_line=$(echo "$line" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')
    debug "Read line (trimmed): '$trimmed_line'"

    if [ -n "$trimmed_line" ]; then
      local processed_keyword
      processed_keyword=$(echo "$trimmed_line" | sed -E 's/[[:space:]]+/_/g')
      debug "Processed keyword: '$processed_keyword'"
      keywords_output+="$processed_keyword-"
    fi
  done < <(
    cat "$file_path"
    echo
  )

  if [[ -n "$keywords_output" ]]; then
    keywords_output=${keywords_output%?}
  fi
  debug "Generated keyword string from $file_path: '$keywords_output'"
  echo "$keywords_output"
}

# Function to reset/delete the Wolfgang run log file
# $1: The base input path where the log file is expected
reset_log() {
  local target_input_path="$1"
  local abs_target_input_path

  if [[ -z "$target_input_path" ]]; then
    target_input_path="." # Default to current directory if not specified
  fi

  if ! abs_target_input_path=$(readlink -f "$target_input_path"); then
    echo "❌ Error: Input path '$target_input_path' for reset is not valid or does not exist." >&2
    exit 1
  fi

  local log_file_name=".wolfgang_run_log.jsonl"
  local log_to_reset="${abs_target_input_path}/${log_file_name}"

  debug "Attempting to reset log file at: $log_to_reset"

  if [[ -f "$log_to_reset" ]]; then
    if rm "$log_to_reset"; then
      echo "✅ Log file '$log_to_reset' has been successfully deleted."
    else
      echo "❌ Error: Failed to delete log file '$log_to_reset'. Check permissions." >&2
      exit 1
    fi
  else
    echo "ℹ️ Log file '$log_to_reset' not found. Nothing to reset."
  fi
  exit 0
}

convert() {
  local abs_input_path
  abs_input_path=$(readlink -f "$INPUT_PATH")
  debug "Absolute input path: $abs_input_path"

  local CWD
  CWD=$(pwd)
  local output_id="wolfgang"

  local JQ_AVAILABLE="true"
  if ! command -v jq &>/dev/null; then
    echo "⚠️ Warning: jq command not found. Run logging and skipping previously logged files will be disabled." >&2
    JQ_AVAILABLE="false"
  fi

  local log_file_name=".wolfgang_run_log.jsonl"
  local log_file_path="${abs_input_path}/${log_file_name}"
  debug "Run log file path: $log_file_path"
  if [[ "$JQ_AVAILABLE" == "true" ]]; then
    touch "$log_file_path"
  fi

  local run_start_iso_time
  run_start_iso_time=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

  local global_keyword_file_path=""
  local GLOBAL_KEYWORDS_STRING=""

  if ls -t "$CWD"/*.txt >/dev/null 2>&1; then
    local temp_gk_path
    temp_gk_path=$(ls -t "$CWD"/*.txt | head -n 1)
    if [[ -n "$temp_gk_path" ]]; then
      global_keyword_file_path=$(readlink -f "$temp_gk_path")
    fi
  fi

  if [[ -n "$global_keyword_file_path" ]] && [[ -f "$global_keyword_file_path" ]]; then
    debug "Attempting to load global keywords from (last modified .txt in CWD): $global_keyword_file_path"
    GLOBAL_KEYWORDS_STRING=$(generate_keyword_string_from_file "$global_keyword_file_path")
    debug "Global keywords loaded: '$GLOBAL_KEYWORDS_STRING'"
  else
    global_keyword_file_path=""
    debug "No global keyword file found in $CWD or it was not a regular file."
  fi

  echo "Start converting images - Prefix: '$CUSTOM_BASE', longer image side: $MAX_SIZE px..."
  if [[ -n "$GLOBAL_KEYWORDS_STRING" ]]; then
    echo "Global keywords active (from $global_keyword_file_path): $GLOBAL_KEYWORDS_STRING"
  fi
  echo "---------------------------------------"

  local BASE_FOLDERTIMESTAMP
  BASE_FOLDERTIMESTAMP=$(date +"%Y-%m-%d_%H-%M-%S")
  local FOLDERTIMESTAMP_NAME="${BASE_FOLDERTIMESTAMP}-${output_id}"

  local output_folder_path="${CWD}/${FOLDERTIMESTAMP_NAME}"
  mkdir -p "$output_folder_path"
  debug "Created main output folder: $output_folder_path"

  local output_base_dir="$output_folder_path"
  mkdir -p "$output_base_dir/resized_jpg"
  mkdir -p "$output_base_dir/resized_webp"

  local processed_files_json_array="[]"

  while IFS= read -r -d $'\0' img_path_from_find; do
    local img_abs_path
    img_abs_path=$(readlink -f "$img_path_from_find")

    if [[ "$JQ_AVAILABLE" == "true" && -s "$log_file_path" ]]; then
      if jq -e --arg OPATH "$img_abs_path" 'select(.converted_files[]?.original_path == $OPATH)' "$log_file_path" >/dev/null; then
        debug "Skipping '$img_abs_path' as it is logged as previously converted (jq check)."
        echo "ℹ️ Already processed (logged): $img_abs_path"
        continue
      fi
    fi

    if ! file --mime-type "$img_abs_path" | grep -qE 'image/'; then
      debug "Skipping non-image file: $img_abs_path"
      continue
    fi

    local skip_current_image="false"
    local REL_PATH
    REL_PATH="${img_abs_path#$abs_input_path/}"
    if [[ "$img_abs_path" == "$abs_input_path" ]]; then
      REL_PATH=$(basename "$img_abs_path")
    fi
    debug "Source image: $img_abs_path, Relative path for output structure: $REL_PATH"

    local DIR_PATH_RAW
    DIR_PATH_RAW=$(dirname "$REL_PATH")
    local current_img_src_dir
    current_img_src_dir=$(dirname "$img_abs_path")
    local DIR_PATH_FOR_OUTPUT
    if [[ "$DIR_PATH_RAW" == "." ]]; then
      DIR_PATH_FOR_OUTPUT=""
    else
      DIR_PATH_FOR_OUTPUT="$DIR_PATH_RAW/"
    fi

    local FILE_NAME
    FILE_NAME=$(basename "$img_abs_path")
    local ORIGINAL_FILE_BASE="${FILE_NAME%.*}"

    mkdir -p "$output_base_dir/resized_jpg/${DIR_PATH_FOR_OUTPUT}"
    mkdir -p "$output_base_dir/resized_webp/${DIR_PATH_FOR_OUTPUT}"

    local local_keyword_file_path=""
    local LOCAL_KEYWORDS_STRING=""
    if ls -t "$current_img_src_dir"/*.txt >/dev/null 2>&1; then
      local temp_lk_path
      temp_lk_path=$(ls -t "$current_img_src_dir"/*.txt | head -n 1)
      if [[ -n "$temp_lk_path" ]]; then
        local_keyword_file_path=$(readlink -f "$temp_lk_path")
      fi
    fi

    if [[ -n "$local_keyword_file_path" ]] && [[ -f "$local_keyword_file_path" ]]; then
      debug "Attempting to load local keywords for image $FILE_NAME from: $local_keyword_file_path"
      if [[ -n "$global_keyword_file_path" && "$local_keyword_file_path" == "$global_keyword_file_path" ]]; then
        debug "Local keyword file is same as global. Not adding as separate local keywords."
      else
        LOCAL_KEYWORDS_STRING=$(generate_keyword_string_from_file "$local_keyword_file_path")
        debug "Local keywords loaded for $current_img_src_dir: '$LOCAL_KEYWORDS_STRING'"
      fi
    else
      local_keyword_file_path=""
      debug "No local keyword file found in $current_img_src_dir for $FILE_NAME."
    fi

    local combined_image_keywords=""
    if [[ -n "$GLOBAL_KEYWORDS_STRING" ]]; then
      combined_image_keywords="$GLOBAL_KEYWORDS_STRING"
    fi
    if [[ -n "$LOCAL_KEYWORDS_STRING" ]]; then
      if [[ -n "$combined_image_keywords" ]]; then
        if [[ -n "$GLOBAL_KEYWORDS_STRING" && -n "$LOCAL_KEYWORDS_STRING" ]]; then
          combined_image_keywords+="-"
        fi
        combined_image_keywords+="$LOCAL_KEYWORDS_STRING"
      else
        combined_image_keywords="$LOCAL_KEYWORDS_STRING"
      fi
    fi
    combined_image_keywords=$(echo "$combined_image_keywords" | sed -e 's/--\+/-/g' -e 's/^-//' -e 's/-$//')
    debug "Combined keywords for $FILE_NAME: '$combined_image_keywords'"

    width=$(identify -format "%w" "$img_abs_path")
    height=$(identify -format "%h" "$img_abs_path")
    if [[ -z "$width" || -z "$height" ]]; then
      echo "⚠️ Could not get dimensions for $img_abs_path. Skipping."
      continue
    fi

    local base_filetimestamp
    base_filetimestamp=$(date +"%H%M%S")
    local current_filetimestamp_suffix=""
    local iterator=1
    local new_filename_base

    while true; do
      local effective_timestamp="${base_filetimestamp}${current_filetimestamp_suffix}"
      local temp_filename_parts=()
      temp_filename_parts+=("$CUSTOM_BASE")
      [[ -n "$combined_image_keywords" ]] && temp_filename_parts+=("$combined_image_keywords")
      temp_filename_parts+=("$effective_timestamp")
      [[ "$ADD_OLD_FILENAME" == "y" && -n "$ORIGINAL_FILE_BASE" ]] && temp_filename_parts+=("$ORIGINAL_FILE_BASE")

      local candidate_base_name
      candidate_base_name=$(
        IFS='-'
        echo "${temp_filename_parts[*]}"
      )
      candidate_base_name=$(echo "$candidate_base_name" | sed -e 's/--\+/-/g' -e 's/^-//' -e 's/-$//' -e 's/[^a-zA-Z0-9_.-]/_/g')

      local check_path="$output_base_dir/resized_jpg/${DIR_PATH_FOR_OUTPUT}${candidate_base_name}.jpg"
      debug "Checking for potential output: $check_path (candidate base: $candidate_base_name)"

      if [[ ! -f "$check_path" ]]; then
        new_filename_base="$candidate_base_name"
        debug "Unique filename base selected: $new_filename_base (timestamp part: $effective_timestamp)"
        break
      fi

      iterator=$((iterator + 1))
      current_filetimestamp_suffix="-${iterator}"
      debug "Collision for ${candidate_base_name}.jpg. Attempting with suffix: $current_filetimestamp_suffix"

      if [[ "$iterator" -gt 100 ]]; then
        echo "⚠️ Critical: Could not find unique filename for '$img_abs_path' after $iterator attempts. Skipping." >&2
        skip_current_image="true"
        break
      fi
    done

    if [[ "$skip_current_image" == "true" ]]; then
      debug "Skipping conversion for $img_abs_path due to filename collision."
      continue
    fi

    local convert_cmd
    if [ "$width" -gt "$height" ]; then
      convert_cmd="-resize ${MAX_SIZE}x\> -quality 90"
    else
      convert_cmd="-resize x${MAX_SIZE}\> -quality 90"
    fi

    local output_jpg_rel_path="${DIR_PATH_FOR_OUTPUT}${new_filename_base}.jpg"
    local output_webp_rel_path="${DIR_PATH_FOR_OUTPUT}${new_filename_base}.webp"
    local output_jpg_abs_path="$output_base_dir/resized_jpg/${output_jpg_rel_path}"
    local output_webp_abs_path="$output_base_dir/resized_webp/${output_webp_rel_path}"
    local success_jpg=false
    local success_webp=false

    debug "Converting '$img_abs_path' to '$output_jpg_abs_path'"
    if magick "$img_abs_path" $convert_cmd "$output_jpg_abs_path"; then
      debug "Successfully converted '$img_abs_path' to '$output_jpg_abs_path'"
      success_jpg=true
    else
      echo "❌ Error converting '$img_abs_path' to JPG. magick exit code: $?" >&2
    fi

    debug "Converting '$img_abs_path' to '$output_webp_abs_path'"
    if magick "$img_abs_path" $convert_cmd "$output_webp_abs_path"; then
      debug "Successfully converted '$img_abs_path' to '$output_webp_abs_path'"
      success_webp=true
    else
      echo "❌ Error converting '$img_abs_path' to WebP. magick exit code: $?" >&2
    fi

    if [[ "$success_jpg" == true || "$success_webp" == true ]] && [[ "$JQ_AVAILABLE" == "true" ]]; then
      local file_entry_json
      file_entry_json=$(jq -n \
        --arg oap "$img_abs_path" \
        --arg orp "$REL_PATH" \
        --arg ajp "$output_jpg_abs_path" \
        --arg awp "$output_webp_abs_path" \
        '{original_path: $oap, original_relative_path: $orp, jpg_path: $ajp, webp_path: $awp}')

      processed_files_json_array=$(echo "$processed_files_json_array" | jq --argjson entry "$file_entry_json" '. + [$entry]')
      debug "Updated processed_files_json_array: $processed_files_json_array"
    fi

    local user_msg_jpg_path="resized_jpg/${output_jpg_rel_path}"
    local user_msg_webp_path="resized_webp/${output_webp_rel_path}"
    echo "✔ Converted: $REL_PATH → $FOLDERTIMESTAMP_NAME/$user_msg_jpg_path & $FOLDERTIMESTAMP_NAME/$user_msg_webp_path"
  done < <(find "$abs_input_path" \
    \( -type d -name "*-${output_id}" -prune \) -o \
    \( -type d -name ".git" -prune \) -o \
    \( -type d -name ".DS_Store" -prune \) -o \
    \( -type f \( -iname "*.jpg" -o -iname "*.jpeg" -o -iname "*.png" -o -iname "*.webp" -o -iname "*.heic" -o -iname "*.gif" -o -iname "*.tif" -o -iname "*.tiff" \) -print0 \))

  if [[ "$JQ_AVAILABLE" == "true" ]]; then
    debug "Final processed_files_json_array before logging run: $processed_files_json_array"
    local run_log_entry
    run_log_entry=$(jq -n \
      --arg rts "$run_start_iso_time" \
      --argjson ms "$MAX_SIZE" \
      --arg cb "$CUSTOM_BASE" \
      --arg aof "$ADD_OLD_FILENAME" \
      --arg ipath "$abs_input_path" \
      --arg gkfp_arg "${global_keyword_file_path:-}" \
      --arg gks_arg "${GLOBAL_KEYWORDS_STRING:-}" \
      --arg ofn "$FOLDERTIMESTAMP_NAME" \
      --argjson cf "$processed_files_json_array" \
      '{
        run_timestamp: $rts,
        settings: {
            max_size: $ms,
            custom_base: $cb,
            add_old_filename: $aof,
            input_path: $ipath,
            global_keywords_file: (if $gkfp_arg == "" then null else $gkfp_arg end),
            global_keywords_string: (if $gks_arg == "" then null else $gks_arg end)
        },
        output_folder_name: $ofn,
        converted_files: $cf
    }')

    echo "$run_log_entry" >>"$log_file_path"
    debug "Appended run information to log file: $log_file_path"
  fi

  echo "---------------------------------------"
  echo "✅ All images converted successfully! Output is in '$output_folder_path'."
  if [[ "$JQ_AVAILABLE" == "true" ]]; then
    echo "ℹ️ Run details logged to: $log_file_path"
  fi
}

wizard_mode() {
  echo "Welcome! Let's prepare some images for the web"

  local current_input_path_for_log
  current_input_path_for_log=$(readlink -f "$INPUT_PATH")
  local log_file_name=".wolfgang_run_log.jsonl"
  local log_file_path_for_wizard="${current_input_path_for_log}/${log_file_name}"
  local settings_from_preset="false"
  local JQ_WIZARD_AVAILABLE="true"

  if ! command -v jq &>/dev/null; then
    JQ_WIZARD_AVAILABLE="false"
    debug "Wizard: jq not found. Presets will not be offered."
  fi

  if [[ "$JQ_WIZARD_AVAILABLE" == "true" && -f "$log_file_path_for_wizard" && -s "$log_file_path_for_wizard" ]]; then
    debug "Wizard: Log file found at $log_file_path_for_wizard, attempting to read presets."
    local presets_json
    presets_json=$(jq -s 'map(select(.settings.custom_base and .settings.max_size and .settings.add_old_filename) | .settings | {custom_base, max_size, add_old_filename}) | reverse | unique | .[0:5] // []' "$log_file_path_for_wizard" 2>/dev/null || echo "[]")

    local preset_count
    preset_count=$(echo "$presets_json" | jq 'length')
    debug "Wizard: Found $preset_count unique presets."

    if [[ "$preset_count" -gt 0 ]]; then
      echo "---------------------------------------"
      echo "You ran wolfgang previously in this directory with the following settings:"
      declare -a displayed_options_cb
      declare -a displayed_options_ms
      declare -a displayed_options_aof

      for i in $(seq 0 $((preset_count - 1))); do
        local cb ms aof
        cb=$(echo "$presets_json" | jq -r ".[$i].custom_base")
        ms=$(echo "$presets_json" | jq -r ".[$i].max_size")
        aof=$(echo "$presets_json" | jq -r ".[$i].add_old_filename")

        displayed_options_cb+=("$cb")
        displayed_options_ms+=("$ms")
        displayed_options_aof+=("$aof")

        local aof_display="n"
        if [[ "$aof" == "y" ]]; then aof_display="y"; fi
        echo "$((i + 1))) Prefix: '$cb', Dimension: $ms px, Append Filename: $aof_display"
      done

      while true; do
        read -rp "Select one of these presets (1-$preset_count) or skip with (N)o: " user_choice
        if [[ "$user_choice" =~ ^[Nn]$ ]]; then
          echo "Proceeding with manual settings input."
          settings_from_preset="false"
          break
        elif [[ "$user_choice" =~ ^[0-9]+$ && "$user_choice" -ge 1 && "$user_choice" -le "$preset_count" ]]; then
          local selected_index=$((user_choice - 1))
          CUSTOM_BASE="${displayed_options_cb[$selected_index]}"
          MAX_SIZE="${displayed_options_ms[$selected_index]}"
          ADD_OLD_FILENAME="${displayed_options_aof[$selected_index]}"
          echo "Using preset: Prefix: '$CUSTOM_BASE', Dimension: $MAX_SIZE px, Append Filename: $ADD_OLD_FILENAME"
          settings_from_preset="true"
          break
        else
          echo "Invalid selection. Please enter a number between 1 and $preset_count, or N."
        fi
      done
    fi
  else
    debug "Wizard: No log file found, log is empty, or jq not available. Skipping presets."
  fi

  echo "---------------------------------------"

  if [[ "$settings_from_preset" != "true" ]]; then
    read -rp "Enter the base name for your output files (e. g. 'converted_'): " CUSTOM_BASE
    if [[ -z "$CUSTOM_BASE" ]]; then
      echo "ERROR: Basename is required." >&2
      exit 1
    fi
    echo "---------------------------------------"

    while true; do
      read -rp "Enter pixel value for the longer side of the image (e.g. 1400): " MAX_SIZE
      if [[ "$MAX_SIZE" =~ ^[0-9]+$ && "$MAX_SIZE" -gt 0 ]]; then break; else echo "ERROR: Please enter a valid positive number!"; fi
    done
    echo "---------------------------------------"

    while true; do
      echo "Should the old filename be appended to the new one? (y/n)"
      read -n 1 -r user_aof_choice
      echo
      case "$user_aof_choice" in
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
  fi

  echo "---------------------------------------"
  echo "Keywords will be automatically detected:"
  echo "  - Global keywords: From the last modified *.txt file in the current directory (where script is run)."
  echo "  - Local keywords: From the last modified *.txt file in each image's own directory."
  echo "---------------------------------------"

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
  echo "  -a, --append               Append the original filename (without extension) to the new name."
  echo "  -r, --reset                Deletes the .wolfgang_run_log.jsonl in the INPUT_PATH (or current dir)." # New help text
  echo "  --debug                    Enable detailed debug output."
  echo ""
  echo "KEYWORD DETECTION:"
  echo "  Keywords are automatically detected from '*.txt' files:"
  echo "  1. Global Keywords: The last modified '*.txt' file found in the directory"
  echo "     where the script is run provides global keywords for all images."
  echo "  2. Local Keywords: For each image, the last modified '*.txt' file in its"
  echo "     own directory provides local keywords for that image and others in the same directory."
  echo "     (If this file is the same as the global keyword file, it's not re-added)."
  echo "  Keywords within these files should be one per line. Words on a single line will be joined by underscores."
  echo ""
  echo "OUTPUT FOLDER:"
  echo "  Converted images are saved in a directory named 'YYYY-MM-DD_HH-M-SS-wolfgang' (created in current dir)."
  echo "  The script will automatically skip any subdirectories ending with '-wolfgang' to avoid reprocessing."
  echo ""
  echo "RUN LOGGING (Requires 'jq' command-line JSON processor):"
  echo "  If 'jq' is installed, each run's settings and processed files are logged to '.wolfgang_run_log.jsonl'"
  echo "  in the root of the INPUT_PATH. The script will use this log to skip already converted original files."
  echo "  Wizard mode will offer to reuse settings from previous runs in the current directory if available."
  echo ""
  echo "ARGUMENTS:"
  echo "  INPUT_PATH                 Directory that contains the images to convert"
  echo "                             (Default: Current directory)"
  exit 0
}

# Argument parsing starts here
# Store original number of arguments to distinguish between no args (wizard) and args processed to zero
original_arg_count=$#

POSITIONAL_ARGS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
  -h | --help | -man | --man) show_help ;;
  -r | --reset) # New option
    RESET_LOG_REQUESTED="true"
    shift
    ;;
  -d | --dimension)
    if [[ -z "$2" || "$2" == -* ]]; then
      echo "❌ Error: Missing value for $1 option." >&2
      exit 1
    fi
    if ! [[ "$2" =~ ^[0-9]+$ && "$2" -gt 0 ]]; then
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

# Determine INPUT_PATH from positional arguments or default
if [[ ${#POSITIONAL_ARGS[@]} -eq 1 ]]; then
  INPUT_PATH="${POSITIONAL_ARGS[0]}"
elif [[ ${#POSITIONAL_ARGS[@]} -gt 1 ]]; then
  echo "❌ Too many arguments. Only one INPUT_PATH is allowed. See --help."
  exit 1
else
  # If no positional args, INPUT_PATH remains its default "."
  # This is fine for -reset and wizard_mode if no path is given.
  : # No-op, INPUT_PATH is already "." if not overridden
fi

# --- Execution Control ---

# Handle -reset first if requested
if [[ "$RESET_LOG_REQUESTED" == "true" ]]; then
  debug "Reset log requested for INPUT_PATH: $INPUT_PATH"
  reset_log "$INPUT_PATH" # reset_log will exit
fi

debug "Initial Configuration (after potential wizard/args):"
debug "  MAX_SIZE: $MAX_SIZE"
debug "  ADD_OLD_FILENAME: $ADD_OLD_FILENAME"
debug "  INPUT_PATH: $INPUT_PATH"
debug "  CUSTOM_BASE: $CUSTOM_BASE"
debug "  DEBUG: $DEBUG"
debug "  CWD for global keywords & output folder: $(pwd)"

# Decide whether to run wizard or convert based on original argument count
if [[ "$original_arg_count" -eq 0 ]]; then
  # No arguments were passed initially, run wizard mode
  wizard_mode # wizard_mode calls convert and exits
else
  # Arguments were passed (and it wasn't just -reset, as that would have exited)
  # Validate INPUT_PATH if it was set from an argument
  if [[ "${#POSITIONAL_ARGS[@]}" -eq 1 || (-n "$INPUT_PATH" && "$INPUT_PATH" != ".") ]]; then # Check if INPUT_PATH was explicitly set
    if ! temp_abs_input_path=$(readlink -f "$INPUT_PATH"); then
      echo "❌ Error: Input path '$INPUT_PATH' is not a valid directory or does not exist." >&2
      exit 1
    elif [[ ! -d "$temp_abs_input_path" ]]; then
      echo "❌ Error: Input path '$INPUT_PATH' ('$temp_abs_input_path') is not a directory." >&2
      exit 1
    fi
  fi
  convert
fi
