#!/bin/bash

# Default values
MAX_SIZE=1400
QUALITY=80
ADD_OLD_FILENAME="n"
INPUT_PATH="."
CUSTOM_BASE="image"
DEBUG="false"
RESET_LOG_REQUESTED="false"
SEPARATOR_STYLE="dash" # Default separator style: 'dash', 'underscore', or 'caps'

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

# Function to reset/delete the Wolfgang run log file AND revert original filenames
# $1: The base input path where the log file and indexed images are located
reset_log() {
  local target_input_path="$1"
  local abs_target_input_path
  local renamed_count=0
  local skipped_count=0
  local output_id="wolfgang" # Used to identify and ignore output folders

  if [[ -z "$target_input_path" ]]; then
    target_input_path="."
  fi

  if ! abs_target_input_path=$(readlink -f "$target_input_path"); then
    echo "❌ Error: Input path '$target_input_path' for reset is not valid or does not exist." >&2
    exit 1
  fi

  echo "ℹ️ Starting reset process in: $abs_target_input_path"
  echo "---------------------------------------"
  echo "🔎 Searching for original files with Wolfgang-generated indexes to rename back..."
  echo "ℹ️ Note: Directories ending in '-${output_id}' will be ignored."

  while IFS= read -r -d $'\0' indexed_file_path; do
    local original_file_path
    original_file_path=$(echo "$indexed_file_path" | sed -E 's/-[0-9]{4}(\.[^.]+)$/\1/')

    if [[ "$indexed_file_path" == "$original_file_path" ]]; then
      debug "Skipping '$indexed_file_path' as it does not match the specific '...-[0-9]{4}.ext' pattern."
      continue
    fi

    if [[ -e "$original_file_path" ]]; then
      echo "⚠️ Skipping rename of '$(basename "$indexed_file_path")': Target '$(basename "$original_file_path")' already exists." >&2
      skipped_count=$((skipped_count + 1))
    else
      if mv "$indexed_file_path" "$original_file_path"; then
        echo "✅ Renamed back: '$(basename "$indexed_file_path")' → '$(basename "$original_file_path")'"
        renamed_count=$((renamed_count + 1))
      else
        echo "❌ Error: Failed to rename '$(basename "$indexed_file_path")'. Check permissions." >&2
        skipped_count=$((skipped_count + 1))
      fi
    fi
  done < <(find "$abs_target_input_path" \
    \( -type d -name "*-${output_id}" -prune \) -o \
    \( -type f -name '*-*[0-9][0-9][0-9][0-9].*' -print0 \))

  if [[ "$renamed_count" -gt 0 || "$skipped_count" -gt 0 ]]; then
    echo "---------------------------------------"
    echo "✅ Filename cleanup complete. Renamed: $renamed_count. Skipped: $skipped_count."
  else
    echo "ℹ️ No indexed original files found to rename."
  fi
  echo "---------------------------------------"

  local log_file_name=".wolfgang_run_log.jsonl"
  local log_to_reset="${abs_target_input_path}/${log_file_name}"

  debug "Attempting to reset log file at: $log_to_reset"

  if [[ -f "$log_to_reset" ]]; then
    if rm "$log_to_reset"; then
      echo "✅ Log file '$log_to_reset' has been successfully deleted."
    else
      echo "❌ Error: Failed to delete log file '$log_to_reset'. Check permissions." >&2
    fi
  else
    echo "ℹ️ Log file '$log_to_reset' not found. Nothing to delete."
  fi

  exit 0
}

# Helper to capitalize first letter of a string (Bash 3.2+ compatible)
capitalize_first() {
  local str="$1"
  # Use sed to upper case the first character
  echo "$str" | sed 's/./\u&/'
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
    echo "⚠️ Warning: jq command not found. Run logging and persistent indexing disabled." >&2
    JQ_AVAILABLE="false"
  fi

  local log_file_name=".wolfgang_run_log.jsonl"
  local log_file_path="${abs_input_path}/${log_file_name}"

  if [[ "$JQ_AVAILABLE" == "true" ]]; then
    touch "$log_file_path"
  fi

  local NEXT_AVAILABLE_INDEX=1
  if [[ "$JQ_AVAILABLE" == "true" && -s "$log_file_path" ]]; then
    debug "Scanning log file for highest existing index..."
    local max_logged_index
    max_logged_index=$(jq -s '[inputs.converted_files[]? | (.jpg_path // .webp_path // "") | select(test("-[0-9]{4}\\.(jpg|jpeg|png|webp|gif|tif|tiff|heic)$")) | capture("(?<idx>[0-9]{4})\\.[^.]+$") | .idx | tonumber] | max // 0' "$log_file_path")
    if [[ "$max_logged_index" -ge 0 ]]; then
      NEXT_AVAILABLE_INDEX=$((max_logged_index + 1))
    fi
    debug "Next available index set to: $NEXT_AVAILABLE_INDEX"
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
    GLOBAL_KEYWORDS_STRING=$(generate_keyword_string_from_file "$global_keyword_file_path")
  fi

  echo "Start converting images..."
  echo "  Prefix: '$CUSTOM_BASE'"
  echo "  Max Size: $MAX_SIZE px"
  echo "  Quality: $QUALITY"
  echo "  Separator Style: '$SEPARATOR_STYLE'"
  echo "  Next Index: $(printf "%04d" "$NEXT_AVAILABLE_INDEX")"
  echo "---------------------------------------"

  local BASE_FOLDERTIMESTAMP
  BASE_FOLDERTIMESTAMP=$(date +"%Y-%m-%d_%H-%M-%S")
  local FOLDERTIMESTAMP_NAME="${BASE_FOLDERTIMESTAMP}-${output_id}"

  local output_folder_path="${CWD}/${FOLDERTIMESTAMP_NAME}"
  mkdir -p "$output_folder_path"
  local output_base_dir="$output_folder_path"
  mkdir -p "$output_base_dir/resized_jpg"
  mkdir -p "$output_base_dir/resized_webp"

  local processed_files_json_array="[]"

  while IFS= read -r -d $'\0' img_path_from_find; do
    local img_abs_path
    img_abs_path=$(readlink -f "$img_path_from_find")
    local initial_img_abs_path="$img_abs_path"

    if [[ "$JQ_AVAILABLE" == "true" && -s "$log_file_path" ]]; then
      if jq -e --arg OPATH "$initial_img_abs_path" 'select(.converted_files[]?.original_path == $OPATH)' "$log_file_path" >/dev/null; then
        debug "Skipping '$initial_img_abs_path' - already logged."
        echo "ℹ️ Already processed: $initial_img_abs_path"
        continue
      fi
    fi

    if ! file --mime-type "$initial_img_abs_path" | grep -qE 'image/'; then
      continue
    fi

    # Determine relative paths
    local REL_PATH_FOR_STRUCTURE="${initial_img_abs_path#$abs_input_path/}"
    if [[ "$initial_img_abs_path" == "$abs_input_path" ]]; then
      REL_PATH_FOR_STRUCTURE=$(basename "$initial_img_abs_path")
    fi
    local DIR_PATH_RAW=$(dirname "$REL_PATH_FOR_STRUCTURE")
    local current_img_src_dir=$(dirname "$initial_img_abs_path")
    local DIR_PATH_FOR_OUTPUT=""
    if [[ "$DIR_PATH_RAW" != "." ]]; then
      DIR_PATH_FOR_OUTPUT="$DIR_PATH_RAW/"
    fi

    local INITIAL_FILE_NAME=$(basename "$initial_img_abs_path")
    local ORIGINAL_FILE_BASE_FOR_APPEND="${INITIAL_FILE_NAME%.*}"
    local original_extension="${INITIAL_FILE_NAME##*.}"

    mkdir -p "$output_base_dir/resized_jpg/${DIR_PATH_FOR_OUTPUT}"
    mkdir -p "$output_base_dir/resized_webp/${DIR_PATH_FOR_OUTPUT}"

    # Load Local Keywords
    local local_keyword_file_path=""
    local LOCAL_KEYWORDS_STRING=""
    if ls -t "$current_img_src_dir"/*.txt >/dev/null 2>&1; then
      local temp_lk_path
      temp_lk_path=$(ls -t "$current_img_src_dir"/*.txt | head -n 1)
      if [[ -n "$temp_lk_path" ]]; then
        local_keyword_file_path=$(readlink -f "$temp_lk_path")
        LOCAL_KEYWORDS_STRING=$(generate_keyword_string_from_file "$local_keyword_file_path")
      fi
    fi
    # Clean hyphens for keywords immediately if necessary
    LOCAL_KEYWORDS_STRING=$(echo "$LOCAL_KEYWORDS_STRING" | sed -e 's/--\+/-/g' -e 's/^-//' -e 's/-$//')

    # Rename original file
    local current_index_for_file=$NEXT_AVAILABLE_INDEX
    local formatted_index=$(printf "%04d" "$current_index_for_file")
    local new_original_file_basename_indexed="${ORIGINAL_FILE_BASE_FOR_APPEND}-${formatted_index}"
    local new_original_filename_indexed="${new_original_file_basename_indexed}.${original_extension}"
    local new_original_img_abs_path="${current_img_src_dir}/${new_original_filename_indexed}"

    if mv -n "$initial_img_abs_path" "$new_original_img_abs_path"; then
      img_abs_path="$new_original_img_abs_path"
    else
      echo "❌ Error: Failed to rename '$initial_img_abs_path'. Skipping." >&2
      continue
    fi

    # Get dimensions
    width=$(magick identify -format "%w" "$img_abs_path")
    height=$(magick identify -format "%h" "$img_abs_path")
    if [[ -z "$width" || -z "$height" ]]; then
      echo "⚠️ Could not get dimensions. Skipping." >&2
      continue
    fi

    # --- NEW FILENAME ASSEMBLY LOGIC ---
    local temp_filename_parts=()
    temp_filename_parts+=("$CUSTOM_BASE")
    [[ -n "$LOCAL_KEYWORDS_STRING" ]] && temp_filename_parts+=("$LOCAL_KEYWORDS_STRING")
    [[ "$ADD_OLD_FILENAME" == "y" && -n "$ORIGINAL_FILE_BASE_FOR_APPEND" ]] && temp_filename_parts+=("$ORIGINAL_FILE_BASE_FOR_APPEND")
    temp_filename_parts+=("$formatted_index")

    local new_filename_base=""

    if [[ "$SEPARATOR_STYLE" == "caps" ]]; then
      # Camel/Pascal Case: Capitalize first letter of every part, join with empty string
      for part in "${temp_filename_parts[@]}"; do
        # Handle keywords that might be hyphen-separated inside the string
        # Split 'part' by hyphens, capitalize each chunk
        local caps_part_accum=""
        local old_ifs="$IFS"
        IFS='-' read -ra SUBPARTS <<<"$part"
        IFS="$old_ifs"
        for sub in "${SUBPARTS[@]}"; do
          local capped_sub
          capped_sub=$(capitalize_first "$sub")
          caps_part_accum+="${capped_sub}"
        done
        new_filename_base+="${caps_part_accum}"
      done
      # Remove non-alphanumeric chars for Caps style to ensure clean look
      new_filename_base=$(echo "$new_filename_base" | sed 's/[^a-zA-Z0-9]//g')

    elif [[ "$SEPARATOR_STYLE" == "underscore" ]]; then
      # Underscore style
      IFS='_'
      new_filename_base="${temp_filename_parts[*]}"
      IFS=' ' # Reset IFS
      # Convert remaining dashes (from keywords) to underscores and clean up
      new_filename_base=$(echo "$new_filename_base" | tr '-' '_' | sed -e 's/__\+/_/g')
      # Sanitize
      new_filename_base=$(echo "$new_filename_base" | sed -e 's/^_//' -e 's/_$//' -e 's/[^a-zA-Z0-9_]/_/g')

    else
      # Default: Dash style
      IFS='-'
      new_filename_base="${temp_filename_parts[*]}"
      IFS=' ' # Reset IFS
      # Convert underscores to dashes (optional, but cleaner)
      new_filename_base=$(echo "$new_filename_base" | tr '_' '-' | sed -e 's/--\+/-/g')
      # Sanitize
      new_filename_base=$(echo "$new_filename_base" | sed -e 's/^-//' -e 's/-$//' -e 's/[^a-zA-Z0-9.-]/-/g')
    fi

    debug "Generated Filename ($SEPARATOR_STYLE): $new_filename_base"
    # --- END NEW ASSEMBLY ---

    local check_path_jpg="$output_base_dir/resized_jpg/${DIR_PATH_FOR_OUTPUT}${new_filename_base}.jpg"
    if [[ -f "$check_path_jpg" ]]; then
      debug "Warning: Output file '$check_path_jpg' exists. Overwriting."
    fi

    local convert_cmd
    if [ "$width" -gt "$height" ]; then
      convert_cmd="-resize ${MAX_SIZE}x\> -quality ${QUALITY}"
    else
      convert_cmd="-resize x${MAX_SIZE}\> -quality ${QUALITY}"
    fi

    local output_jpg_rel_path="${DIR_PATH_FOR_OUTPUT}${new_filename_base}.jpg"
    local output_webp_rel_path="${DIR_PATH_FOR_OUTPUT}${new_filename_base}.webp"
    local output_jpg_abs_path="$output_base_dir/resized_jpg/${output_jpg_rel_path}"
    local output_webp_abs_path="$output_base_dir/resized_webp/${output_webp_rel_path}"
    local success_jpg=false
    local success_webp=false

    if magick "$img_abs_path" $convert_cmd "$output_jpg_abs_path"; then
      success_jpg=true
    else
      echo "❌ Error converting to JPG." >&2
    fi

    if magick "$img_abs_path" $convert_cmd "$output_webp_abs_path"; then
      success_webp=true
    else
      echo "❌ Error converting to WebP." >&2
    fi

    if [[ "$success_jpg" == true || "$success_webp" == true ]]; then
      NEXT_AVAILABLE_INDEX=$((NEXT_AVAILABLE_INDEX + 1))

      if [[ "$JQ_AVAILABLE" == "true" ]]; then
        local rel_path_of_renamed_original_for_log="${img_abs_path#$abs_input_path/}"
        if [[ "$img_abs_path" == "$abs_input_path" ]]; then
          rel_path_of_renamed_original_for_log=$(basename "$img_abs_path")
        fi

        local file_entry_json
        file_entry_json=$(jq -n \
          --arg oap "$img_abs_path" \
          --arg orp "$rel_path_of_renamed_original_for_log" \
          --arg ajp "$([[ "$success_jpg" == true ]] && echo "$output_jpg_abs_path" || echo null)" \
          --arg awp "$([[ "$success_webp" == true ]] && echo "$output_webp_abs_path" || echo null)" \
          --arg lkfp "${local_keyword_file_path:-}" \
          --arg lks "${LOCAL_KEYWORDS_STRING:-}" \
          --arg cidx "$formatted_index" \
          '{original_path: $oap, original_relative_path: $orp, jpg_path: (if $ajp == "null" then null else $ajp end), webp_path: (if $awp == "null" then null else $awp end), local_keyword_file: (if $lkfp == "" then null else $lkfp end), local_keywords_string: (if $lks == "" then null else $lks end), filename_index: $cidx}')

        processed_files_json_array=$(echo "$processed_files_json_array" | jq --argjson entry "$file_entry_json" '. + [$entry]')
      fi
    fi

    echo "✔ Converted: $(basename "$img_abs_path") → $new_filename_base (JPG/WebP)"

  done < <(find "$abs_input_path" \
    \( -type d -name "*-${output_id}" -prune \) -o \
    \( -type d -name ".git" -prune \) -o \
    \( -type d -name ".DS_Store" -prune \) -o \
    \( -type f \( -iname "*.jpg" -o -iname "*.jpeg" -o -iname "*.png" -o -iname "*.webp" -o -iname "*.heic" -o -iname "*.gif" -o -iname "*.tif" -o -iname "*.tiff" \) -print0 \))

  if [[ "$JQ_AVAILABLE" == "true" ]]; then
    local run_log_entry
    run_log_entry=$(jq -n \
      --arg rts "$run_start_iso_time" \
      --argjson ms "$MAX_SIZE" \
      --argjson q "$QUALITY" \
      --arg cb "$CUSTOM_BASE" \
      --arg sep "$SEPARATOR_STYLE" \
      --arg aof "$ADD_OLD_FILENAME" \
      --arg ipath "$abs_input_path" \
      --arg ofn "$FOLDERTIMESTAMP_NAME" \
      --argjson cf "$processed_files_json_array" \
      '{
        run_timestamp: $rts,
        settings: {
            max_size: $ms,
            quality: $q,
            custom_base: $cb,
            separator_style: $sep,
            add_old_filename: $aof,
            input_path: $ipath
        },
        output_folder_name: $ofn,
        converted_files: $cf
    }')
    echo "$run_log_entry" >>"$log_file_path"
  fi

  echo "---------------------------------------"
  echo "✅ All images converted successfully! Output in '$output_folder_path'."
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
    debug "Wizard: Reading presets..."
    # Now includes separator_style in preset logic
    local presets_json
    presets_json=$(jq -s 'map(select(.settings.custom_base) | .settings | {custom_base, max_size, quality, add_old_filename, separator_style}) | reverse | unique | .[0:5] // []' "$log_file_path_for_wizard" 2>/dev/null || echo "[]")

    local preset_count
    preset_count=$(echo "$presets_json" | jq 'length')

    if [[ "$preset_count" -gt 0 ]]; then
      echo "---------------------------------------"
      echo "Previous run presets found:"
      declare -a displayed_options_cb
      declare -a displayed_options_ms
      declare -a displayed_options_q
      declare -a displayed_options_aof
      declare -a displayed_options_sep

      for i in $(seq 0 $((preset_count - 1))); do
        local cb ms q aof sep
        cb=$(echo "$presets_json" | jq -r ".[$i].custom_base")
        ms=$(echo "$presets_json" | jq -r ".[$i].max_size")
        q=$(echo "$presets_json" | jq -r ".[$i].quality // 80")
        aof=$(echo "$presets_json" | jq -r ".[$i].add_old_filename")
        sep=$(echo "$presets_json" | jq -r ".[$i].separator_style // \"dash\"")

        displayed_options_cb+=("$cb")
        displayed_options_ms+=("$ms")
        displayed_options_q+=("$q")
        displayed_options_aof+=("$aof")
        displayed_options_sep+=("$sep")

        echo "$((i + 1))) Prefix: '$cb', Dim: $ms, Quality: $q, Style: $sep, Append Old: $aof"
      done

      while true; do
        read -rp "Select a preset (1-$preset_count) or (N)o: " user_choice
        if [[ "$user_choice" =~ ^[Nn]$ ]]; then
          settings_from_preset="false"
          break
        elif [[ "$user_choice" =~ ^[0-9]+$ && "$user_choice" -ge 1 && "$user_choice" -le "$preset_count" ]]; then
          local selected_index=$((user_choice - 1))
          CUSTOM_BASE="${displayed_options_cb[$selected_index]}"
          MAX_SIZE="${displayed_options_ms[$selected_index]}"
          QUALITY="${displayed_options_q[$selected_index]}"
          ADD_OLD_FILENAME="${displayed_options_aof[$selected_index]}"
          SEPARATOR_STYLE="${displayed_options_sep[$selected_index]}"
          echo "Used preset with style: $SEPARATOR_STYLE"
          settings_from_preset="true"
          break
        fi
      done
    fi
  fi

  echo "---------------------------------------"

  if [[ "$settings_from_preset" != "true" ]]; then
    read -rp "Enter the base name (prefix) (e.g., 'holiday'): " CUSTOM_BASE
    [[ -z "$CUSTOM_BASE" ]] && {
      echo "ERROR: Basename required."
      exit 1
    }

    echo "---------------------------------------"
    while true; do
      read -rp "Enter pixel value for the longer side (e.g. 1400): " MAX_SIZE
      [[ "$MAX_SIZE" =~ ^[0-9]+$ && "$MAX_SIZE" -gt 0 ]] && break
    done

    echo "---------------------------------------"
    while true; do
      read -rp "Enter compression quality 1-100 (Enter for 80): " user_quality
      if [[ -z "$user_quality" ]]; then
        QUALITY=80
        break
      elif [[ "$user_quality" =~ ^[0-9]+$ && "$user_quality" -ge 1 && "$user_quality" -le 100 ]]; then
        QUALITY="$user_quality"
        break
      fi
    done

    echo "---------------------------------------"
    echo "Choose a filename separator style:"
    echo "1) Dash (keyword-prefix-index) [Default]"
    echo "2) Underscore (keyword_prefix_index)"
    echo "3) Capitalize (KeywordPrefixIndex)"
    read -n 1 -r user_sep_choice
    echo ""
    case "$user_sep_choice" in
    2) SEPARATOR_STYLE="underscore" ;;
    3) SEPARATOR_STYLE="caps" ;;
    *) SEPARATOR_STYLE="dash" ;;
    esac
    echo "Selected Style: $SEPARATOR_STYLE"

    echo "---------------------------------------"
    while true; do
      echo "Append old filename? (y/n)"
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
      esac
    done
  fi

  echo "---------------------------------------"
  echo "Structure: [local-keywords][separator][prefix][separator][index].ext"
  echo "---------------------------------------"

  convert
  exit 0
}

show_help() {
  echo "WOLFGANG Usage:"
  echo "  wolfgang [OPTIONS] [INPUT_PATH]"
  echo ""
  echo "OPTIONS:"
  echo "  -n, --name PREFIX      Custom prefix (Default: '$CUSTOM_BASE')"
  echo "  -d, --dimension PIXEL  Longest side (Default: $MAX_SIZE)"
  echo "  -q, --quality 1-100    Compression quality (Default: $QUALITY)"
  echo "  -s, --separator STYLE  Separator style: 'dash', 'underscore', or 'caps'"
  echo "  -a, --append           Append original filename."
  echo "  -r, --reset            Reset log and rename files back."
  echo "  --debug                Enable debug."
  exit 0
}

# Argument parsing
original_arg_count=$#
POSITIONAL_ARGS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
  -h | --help | -man | --man) show_help ;;
  -r | --reset)
    RESET_LOG_REQUESTED="true"
    shift
    ;;
  -d | --dimension)
    MAX_SIZE="$2"
    shift 2
    ;;
  -q | --quality)
    if [[ "$2" =~ ^[0-9]+$ && "$2" -ge 1 && "$2" -le 100 ]]; then
      QUALITY="$2"
    else
      echo "❌ Error: Invalid quality '$2'. Use a number between 1 and 100."
      exit 1
    fi
    shift 2
    ;;
  -n | --name)
    CUSTOM_BASE="$2"
    shift 2
    ;;
  -s | --separator)
    case "$2" in
    dash | underscore | caps) SEPARATOR_STYLE="$2" ;;
    *)
      echo "❌ Error: Invalid separator '$2'. Use dash, underscore, or caps."
      exit 1
      ;;
    esac
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

if [[ ${#POSITIONAL_ARGS[@]} -eq 1 ]]; then
  INPUT_PATH="${POSITIONAL_ARGS[0]}"
elif [[ ${#POSITIONAL_ARGS[@]} -gt 1 ]]; then
  echo "❌ Too many arguments."
  exit 1
fi

if [[ "$RESET_LOG_REQUESTED" == "true" ]]; then
  reset_log "$INPUT_PATH"
fi

if [[ "$original_arg_count" -eq 0 ]]; then
  wizard_mode
else
  convert
fi
