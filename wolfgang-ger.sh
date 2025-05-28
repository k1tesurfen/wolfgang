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
    echo "❌ Fehler: Der Eingabepfad '$target_input_path' zum Zurücksetzen ist ungültig oder existiert nicht." >&2
    exit 1
  fi

  local log_file_name=".wolfgang_run_log.jsonl"
  local log_to_reset="${abs_target_input_path}/${log_file_name}"

  debug "Attempting to reset log file at: $log_to_reset"

  if [[ -f "$log_to_reset" ]]; then
    if rm "$log_to_reset"; then
      echo "✅ Log-Datei '$log_to_reset' wurde erfolgreich gelöscht."
    else
      echo "❌ Fehler: Konnte die Log-Datei '$log_to_reset' nicht löschen. Überprüf mal die Berechtigungen." >&2
      exit 1
    fi
  else
    echo "ℹ️ Log-Datei '$log_to_reset' nicht gefunden. Gibt nix zum Zurücksetzen."
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
    echo "⚠️ Achtung: Befehl 'jq' nicht gefunden. Protokollierung, Überspringen bereits protokollierter Dateien und persistente Indizierung sind deaktiviert." >&2
    JQ_AVAILABLE="false"
  fi

  local log_file_name=".wolfgang_run_log.jsonl"
  local log_file_path="${abs_input_path}/${log_file_name}"
  debug "Run log file path: $log_file_path"
  if [[ "$JQ_AVAILABLE" == "true" ]]; then
    touch "$log_file_path" # Ensure log file exists
  fi

  local NEXT_AVAILABLE_INDEX=1
  if [[ "$JQ_AVAILABLE" == "true" && -s "$log_file_path" ]]; then
    debug "Scanning log file for highest existing index..."
    local max_logged_index
    max_logged_index=$(jq -s '[inputs.converted_files[]? | (.jpg_path // .webp_path // "") | select(test("-[0-9]{4}\\.(jpg|jpeg|png|webp|gif|tif|tiff|heic)$")) | capture("(?<idx>[0-9]{4})\\.[^.]+$") | .idx | tonumber] | max // 0' "$log_file_path")
    debug "Max index found in log: $max_logged_index"
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
    debug "Attempting to load global keywords from (last modified .txt in CWD): $global_keyword_file_path"
    GLOBAL_KEYWORDS_STRING=$(generate_keyword_string_from_file "$global_keyword_file_path")
    debug "Global keywords loaded (for logging only): '$GLOBAL_KEYWORDS_STRING'"
  else
    global_keyword_file_path=""
    debug "No global keyword file found in $CWD or it was not a regular file."
  fi

  echo "Starte Bildkonvertierung - Präfix: '$CUSTOM_BASE', längere Bildseite: $MAX_SIZE px..."
  echo "Fortlaufender Index für Dateinamen beginnt bei: $(printf "%04d" "$NEXT_AVAILABLE_INDEX")"
  if [[ -n "$GLOBAL_KEYWORDS_STRING" ]]; then
    echo "Globale Schlüsselwörter erkannt (aus $global_keyword_file_path): $GLOBAL_KEYWORDS_STRING (Hinweis: Werden nicht im Dateinamen verwendet)"
  fi
  echo "ℹ️ Originaldateien werden UMGENANNT und bekommen den Index (z.B. original-0001.ext)."
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
    local img_abs_path # This will be the path to the image file, potentially renamed.
    img_abs_path=$(readlink -f "$img_path_from_find")
    local initial_img_abs_path="$img_abs_path" # Keep a copy before any renaming, for log skip check

    if [[ "$JQ_AVAILABLE" == "true" && -s "$log_file_path" ]]; then
      # Check log against potential *future* renamed path OR current path if already renamed.
      # This logic might need refinement if script is re-run and files are already indexed.
      # For now, checking the current name. If it was renamed in a previous run, the log would have that name.
      if jq -e --arg OPATH "$initial_img_abs_path" 'select(.converted_files[]?.original_path == $OPATH)' "$log_file_path" >/dev/null; then
        debug "Skipping '$initial_img_abs_path' as it is logged as previously converted (jq check)."
        echo "ℹ️ Bereits verarbeitet (laut Log): $initial_img_abs_path"
        continue
      fi
    fi

    if ! file --mime-type "$initial_img_abs_path" | grep -qE 'image/'; then
      debug "Skipping non-image file: $initial_img_abs_path"
      continue
    fi

    # --- Determine paths and names based on the *initial* file path for output structure ---
    local REL_PATH_FOR_STRUCTURE # Relative path for determining output directory structure
    REL_PATH_FOR_STRUCTURE="${initial_img_abs_path#$abs_input_path/}"
    if [[ "$initial_img_abs_path" == "$abs_input_path" ]]; then
      REL_PATH_FOR_STRUCTURE=$(basename "$initial_img_abs_path")
    fi
    debug "Source image (initial): $initial_img_abs_path, Relative path for output structure: $REL_PATH_FOR_STRUCTURE"

    local DIR_PATH_RAW
    DIR_PATH_RAW=$(dirname "$REL_PATH_FOR_STRUCTURE")
    local current_img_src_dir # Directory where the original image resides
    current_img_src_dir=$(dirname "$initial_img_abs_path")
    local DIR_PATH_FOR_OUTPUT # Subdirectory structure for converted files
    if [[ "$DIR_PATH_RAW" == "." ]]; then
      DIR_PATH_FOR_OUTPUT=""
    else
      DIR_PATH_FOR_OUTPUT="$DIR_PATH_RAW/"
    fi

    local INITIAL_FILE_NAME # Filename before any renaming in this run
    INITIAL_FILE_NAME=$(basename "$initial_img_abs_path")
    local ORIGINAL_FILE_BASE_FOR_APPEND="${INITIAL_FILE_NAME%.*}" # Base name for -a option, e.g., "myphoto"
    local original_extension="${INITIAL_FILE_NAME##*.}"           # Extension, e.g., "jpg"

    mkdir -p "$output_base_dir/resized_jpg/${DIR_PATH_FOR_OUTPUT}"
    mkdir -p "$output_base_dir/resized_webp/${DIR_PATH_FOR_OUTPUT}"

    # --- Local Keywords (based on original file's directory) ---
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
      debug "Attempting to load local keywords for image $INITIAL_FILE_NAME from: $local_keyword_file_path"
      if [[ -n "$global_keyword_file_path" && "$local_keyword_file_path" == "$global_keyword_file_path" ]]; then
        debug "Local keyword file is same as global. Using its content for local keywords."
        LOCAL_KEYWORDS_STRING=$(generate_keyword_string_from_file "$local_keyword_file_path")
      else
        LOCAL_KEYWORDS_STRING=$(generate_keyword_string_from_file "$local_keyword_file_path")
      fi
      debug "Local keywords loaded for $INITIAL_FILE_NAME: '$LOCAL_KEYWORDS_STRING'"
    else
      local_keyword_file_path=""
      debug "No local keyword file found in $current_img_src_dir for $INITIAL_FILE_NAME."
    fi
    LOCAL_KEYWORDS_STRING=$(echo "$LOCAL_KEYWORDS_STRING" | sed -e 's/--\+/-/g' -e 's/^-//' -e 's/-$//')
    debug "Final local keywords for $INITIAL_FILE_NAME: '$LOCAL_KEYWORDS_STRING'"

    # --- Rename the original file ---
    local current_index_for_file=$NEXT_AVAILABLE_INDEX
    local formatted_index
    formatted_index=$(printf "%04d" "$current_index_for_file")

    local new_original_file_basename_indexed="${ORIGINAL_FILE_BASE_FOR_APPEND}-${formatted_index}"
    local new_original_filename_indexed="${new_original_file_basename_indexed}.${original_extension}"
    local new_original_img_abs_path="${current_img_src_dir}/${new_original_filename_indexed}"

    debug "Attempting to rename original image from '$initial_img_abs_path' to '$new_original_img_abs_path'"
    if mv -n "$initial_img_abs_path" "$new_original_img_abs_path"; then
      debug "Successfully renamed original image to '$new_original_img_abs_path'"
      img_abs_path="$new_original_img_abs_path" # CRITICAL: Update img_abs_path to the new name for magick & logging
    else
      echo "❌ Fehler: Konnte Originaldatei '$initial_img_abs_path' nicht nach '$new_original_img_abs_path' umbenennen. mv Exit-Code: $?. Überspringe diese Datei." >&2
      # Do not increment NEXT_AVAILABLE_INDEX if rename fails and we skip.
      continue
    fi
    # --- End Renaming ---

    # Get dimensions from the (now renamed) original file
    width=$(identify -format "%w" "$img_abs_path")
    height=$(identify -format "%h" "$img_abs_path")
    if [[ -z "$width" || -z "$height" ]]; then
      echo "⚠️ Konnte Dimensionen für $img_abs_path nicht ermitteln. Überspringe." >&2
      # If identify fails, something is wrong. Original file (renamed) might be kept.
      # Do not increment NEXT_AVAILABLE_INDEX.
      continue
    fi

    # --- Generate filename for CONVERTED images ---
    local temp_filename_parts=()
    [[ -n "$LOCAL_KEYWORDS_STRING" ]] && temp_filename_parts+=("$LOCAL_KEYWORDS_STRING")
    temp_filename_parts+=("$CUSTOM_BASE")
    # Use the ORIGINAL_FILE_BASE_FOR_APPEND (e.g., "myphoto") if -a is specified
    [[ "$ADD_OLD_FILENAME" == "y" && -n "$ORIGINAL_FILE_BASE_FOR_APPEND" ]] && temp_filename_parts+=("$ORIGINAL_FILE_BASE_FOR_APPEND")
    temp_filename_parts+=("$formatted_index") # Index for the converted file

    local new_filename_base # For converted files
    new_filename_base=$(
      IFS='-'
      echo "${temp_filename_parts[*]}"
    )
    new_filename_base=$(echo "$new_filename_base" | sed -e 's/--\+/-/g' -e 's/^-//' -e 's/-$//' -e 's/[^a-zA-Z0-9_.-]/_/g')
    debug "Generated filename base for converted files (from $INITIAL_FILE_NAME): '$new_filename_base' (using index $formatted_index)"

    local check_path_jpg="$output_base_dir/resized_jpg/${DIR_PATH_FOR_OUTPUT}${new_filename_base}.jpg"
    if [[ -f "$check_path_jpg" ]]; then
      debug "Warning: Output file '$check_path_jpg' already exists. ImageMagick will likely overwrite it."
    fi

    local convert_cmd
    if [ "$width" -gt "$height" ]; then
      convert_cmd="-resize ${MAX_SIZE}x\> -quality 90"
    else
      convert_cmd="-resize x${MAX_SIZE}\> -quality 90"
    fi

    # Paths for CONVERTED files, using DIR_PATH_FOR_OUTPUT from original structure
    local output_jpg_rel_path="${DIR_PATH_FOR_OUTPUT}${new_filename_base}.jpg"
    local output_webp_rel_path="${DIR_PATH_FOR_OUTPUT}${new_filename_base}.webp"
    local output_jpg_abs_path="$output_base_dir/resized_jpg/${output_jpg_rel_path}"
    local output_webp_abs_path="$output_base_dir/resized_webp/${output_webp_rel_path}"
    local success_jpg=false
    local success_webp=false

    debug "Converting '$img_abs_path' (renamed original) to '$output_jpg_abs_path'"
    if magick "$img_abs_path" $convert_cmd "$output_jpg_abs_path"; then
      debug "Successfully converted '$img_abs_path' to '$output_jpg_abs_path'"
      success_jpg=true
    else
      echo "❌ Fehler beim Konvertieren von '$img_abs_path' nach JPG. magick Exit-Code: $?" >&2
    fi

    debug "Converting '$img_abs_path' (renamed original) to '$output_webp_abs_path'"
    if magick "$img_abs_path" $convert_cmd "$output_webp_abs_path"; then
      debug "Successfully converted '$img_abs_path' to '$output_webp_abs_path'"
      success_webp=true
    else
      echo "❌ Fehler beim Konvertieren von '$img_abs_path' nach WebP. magick Exit-Code: $?" >&2
    fi

    if [[ "$success_jpg" == true || "$success_webp" == true ]]; then
      NEXT_AVAILABLE_INDEX=$((NEXT_AVAILABLE_INDEX + 1))
      debug "Incremented NEXT_AVAILABLE_INDEX to $NEXT_AVAILABLE_INDEX"

      if [[ "$JQ_AVAILABLE" == "true" ]]; then
        # Calculate relative path of the RENAMED original file for logging
        local rel_path_of_renamed_original_for_log
        rel_path_of_renamed_original_for_log="${img_abs_path#$abs_input_path/}"
        if [[ "$img_abs_path" == "$abs_input_path" ]]; then # Unlikely for a file from find within input path
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
        debug "Updated processed_files_json_array"
      fi
    fi

    local user_msg_jpg_path="resized_jpg/${output_jpg_rel_path}"
    local user_msg_webp_path="resized_webp/${output_webp_rel_path}"
    # Display the INITIAL_FILE_NAME in the success message for user clarity as "source"
    echo "✔ Konvertiert: $REL_PATH_FOR_STRUCTURE (umbenannt zu $(basename "$img_abs_path")) → $FOLDERTIMESTAMP_NAME/$user_msg_jpg_path & $FOLDERTIMESTAMP_NAME/$user_msg_webp_path"

  done < <(find "$abs_input_path" \
    \( -type d -name "*-${output_id}" -prune \) -o \
    \( -type d -name ".git" -prune \) -o \
    \( -type d -name ".DS_Store" -prune \) -o \
    \( -type f \( -iname "*.jpg" -o -iname "*.jpeg" -o -iname "*.png" -o -iname "*.webp" -o -iname "*.heic" -o -iname "*.gif" -o -iname "*.tif" -o -iname "*.tiff" \) -print0 \))

  if [[ "$JQ_AVAILABLE" == "true" ]]; then
    debug "Final processed_files_json_array before logging run."
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
  echo "✅ Alle Bilder erfolgreich konvertiert! Ausgabe ist in '$output_folder_path'."
  if [[ "$JQ_AVAILABLE" == "true" ]]; then
    echo "ℹ️ Details zum Durchlauf protokolliert in: $log_file_path"
  fi
}

wizard_mode() {
  echo "Willkommen! Lass uns ein paar Bilder fürs Web vorbereiten."

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
      echo "Du hast Wolfgang schon mal in diesem Verzeichnis mit folgenden Einstellungen genutzt:"
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

        local aof_display="n"                            # German 'n' for 'nein'
        if [[ "$aof" == "y" ]]; then aof_display="j"; fi # German 'j' for 'ja'
        echo "$((i + 1))) Präfix: '$cb', Abmessung: $ms px, Dateinamen anhängen: $aof_display"
      done

      while true; do
        read -rp "Wähl eine dieser Voreinstellungen (1-$preset_count) oder überspring mit (N)ein: " user_choice
        if [[ "$user_choice" =~ ^[Nn]$ ]]; then
          echo "Weiter mit manueller Eingabe der Einstellungen."
          settings_from_preset="false"
          break
        elif [[ "$user_choice" =~ ^[0-9]+$ && "$user_choice" -ge 1 && "$user_choice" -le "$preset_count" ]]; then
          local selected_index=$((user_choice - 1))
          CUSTOM_BASE="${displayed_options_cb[$selected_index]}"
          MAX_SIZE="${displayed_options_ms[$selected_index]}"
          ADD_OLD_FILENAME="${displayed_options_aof[$selected_index]}"
          echo "Voreinstellung wird genutzt: Präfix: '$CUSTOM_BASE', Abmessung: $MAX_SIZE px, Dateinamen anhängen: $ADD_OLD_FILENAME"
          settings_from_preset="true"
          break
        else
          echo "Ungültige Auswahl. Bitte gib eine Zahl zwischen 1 und $preset_count ein, oder N."
        fi
      done
    fi
  else
    debug "Wizard: No log file found, log is empty, or jq not available. Skipping presets."
  fi

  echo "---------------------------------------"

  if [[ "$settings_from_preset" != "true" ]]; then
    read -rp "Gib den Basisnamen (Präfix) für deine Ausgabedateien ein (z. B. 'urlaub_'): " CUSTOM_BASE
    if [[ -z "$CUSTOM_BASE" ]]; then
      echo "FEHLER: Ein Basisname (Präfix) ist erforderlich." >&2
      exit 1
    fi
    echo "---------------------------------------"

    while true; do
      read -rp "Gib den Pixelwert für die längere Seite des Bildes ein (z.B. 1400): " MAX_SIZE
      if [[ "$MAX_SIZE" =~ ^[0-9]+$ && "$MAX_SIZE" -gt 0 ]]; then break; else echo "FEHLER: Bitte gib eine gültige positive Zahl ein!"; fi
    done
    echo "---------------------------------------"

    while true; do
      echo "Soll der alte Dateiname an den neuen angehängt werden? (j/n)"
      read -n 1 -r user_aof_choice
      echo
      case "$user_aof_choice" in
      [JjYy]) # Accept J, j, Y, y
        ADD_OLD_FILENAME="y"
        break
        ;;
      [Nn])
        ADD_OLD_FILENAME="n"
        break
        ;;
      *) echo "Bitte 'J' oder 'N' benutzen, um fortzufahren." ;;
      esac
    done
  fi

  echo "---------------------------------------"
  echo "Lokale Schlüsselwörter für Dateinamen: Aus der zuletzt geänderten *.txt-Datei im Verzeichnis des jeweiligen Bildes."
  echo "Globale Schlüsselwörter (falls *.txt im aktuellen Skriptverzeichnis vorhanden): Werden zur Info protokolliert, aber NICHT in Dateinamen verwendet."
  echo "Ausgabe-Dateinamenstruktur: [lokale-schlüsselwörter]-[präfix]-[optional-alter-dateiname]-[index].ext"
  echo "---------------------------------------"

  convert
  exit 0
}

show_help() {
  echo "WOLFGANG - Web Optimized Lightweight Fast Graphics Analyser and Generator"
  echo ""
  echo "VERWENDUNG: wolfgang [OPTIONEN] [EINGABEPFAD]"
  echo ""
  echo "OPTIONEN:"
  echo "  -n, --name PRÄFIX         Benutzerdefiniertes Präfix für Ergebnisdateien (Standard: '$CUSTOM_BASE')"
  echo "  -h, --help, -man, --man   Diese Hilfenachricht"
  echo "  -d, --dimension PIXEL     Längste Seite in Pixeln (Standard: $MAX_SIZE)"
  echo "  -a, --append              Hängt den ursprünglichen Dateinamen (ohne Erweiterung) an den neuen Namen an."
  echo "  -r, --reset               Löscht die .wolfgang_run_log.jsonl im EINGABEPFAD (oder aktuellen Verzeichnis)."
  echo "  --debug                   Detaillierte Debug-Ausgabe aktivieren."
  echo ""
  echo "AUSGABE-DATEINAMENSTRUKTUR:"
  echo "  [LOKALE_SCHLÜSSELWÖRTER]-[PRÄFIX]-[OPTIONAL_ALTER_DATEINAME]-[INDEX].ext"
  echo "  - LOKALE_SCHLÜSSELWÖRTER: Abgeleitet von der zuletzt geänderten '*.txt'-Datei im Verzeichnis des Bildes."
  echo "                          Wörter auf einer Zeile mit '_' verbunden, Zeilen mit '-' verbunden."
  echo "  - PRÄFIX: Der benutzerdefinierte Name, der über -n oder den Assistenten festgelegt wurde (z.B. '$CUSTOM_BASE')."
  echo "  - OPTIONAL_ALTER_DATEINAME: Ursprünglicher Dateiname (ohne Erweiterung), falls -a verwendet wird."
  echo "  - INDEX: Ein fortlaufender 4-stelliger Zähler (z.B. 0001, 0002), eindeutig pro Eingabeverzeichnis-Log."
  echo ""
  echo "SCHLÜSSELWORTERKENNUNG FÜR DATEINAMEN (LOKALE SCHLÜSSELWÖRTER):"
  echo "  Für jedes Bild liefert die zuletzt geänderte '*.txt'-Datei im eigenen Verzeichnis lokale Schlüsselwörter."
  echo "  Schlüsselwörter in diesen Dateien sollten einzeln pro Zeile stehen. Wörter auf einer Zeile werden mit Unterstrichen verbunden."
  echo "  Globale Schlüsselwörter (aus *.txt im aktuellen Arbeitsverzeichnis des Skripts) werden protokolliert, aber NICHT in Dateinamen verwendet."
  echo ""
  echo "AUSGABEORDNER:"
  echo "  Konvertierte Bilder werden in einem Ordner namens 'JJJJ-MM-TT_HH-M-SS-wolfgang' gespeichert (wird im aktuellen Verzeichnis erstellt)."
  echo "  Das Skript überspringt automatisch alle Unterverzeichnisse, die auf '-wolfgang' enden, um eine erneute Verarbeitung zu vermeiden."
  echo ""
  echo "PROTOKOLLIERUNG (Benötigt 'jq' Kommandozeilen-JSON-Prozessor):"
  echo "  Wenn 'jq' installiert ist, werden die Einstellungen jedes Durchlaufs und die verarbeiteten Dateien in '.wolfgang_run_log.jsonl'"
  echo "  im Stammverzeichnis des EINGABEPFADS protokolliert. Das Skript verwendet dieses Log, um bereits konvertierte Dateien zu überspringen und"
  echo "  den nächsten verfügbaren Index für Dateinamen zu bestimmen."
  echo "  Der Assistentenmodus bietet an, Einstellungen von früheren Durchläufen im aktuellen Verzeichnis wiederzuverwenden, falls vorhanden."
  echo ""
  echo "ARGUMENTE:"
  echo "  EINGABEPFAD               Verzeichnis, das die zu konvertierenden Bilder enthält"
  echo "                            (Standard: Aktuelles Verzeichnis)"
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
      echo "❌ Fehler: Fehlender Wert für Option $1." >&2
      exit 1
    fi
    if ! [[ "$2" =~ ^[0-9]+$ && "$2" -gt 0 ]]; then
      echo "❌ Fehler: Der Wert für Dimension ($1) muss eine positive Ganzzahl sein. Erhalten: '$2'" >&2
      exit 1
    fi
    MAX_SIZE="$2"
    shift 2
    ;;
  -n | --name)
    if [[ -z "$2" || "$2" == -* ]]; then
      echo "❌ Fehler: Fehlender Wert für Option $1." >&2
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
    echo "❌ Unbekannte Option: $1"
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
  echo "❌ Zu viele Argumente. Nur ein EINGABEPFAD ist erlaubt. Siehe --help."
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
      echo "❌ Fehler: Eingabepfad '$INPUT_PATH' ist kein gültiges Verzeichnis oder existiert nicht." >&2
      exit 1
    elif [[ ! -d "$temp_abs_input_path" ]]; then
      echo "❌ Fehler: Eingabepfad '$INPUT_PATH' ('$temp_abs_input_path') ist kein Verzeichnis." >&2
      exit 1
    fi
  fi
  convert
fi
