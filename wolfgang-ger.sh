#!/bin/bash

# Default values
MAX_SIZE=1400
ADD_OLD_FILENAME="n"
INPUT_PATH="."
CUSTOM_BASE="bild" # Changed default to German
DEBUG="false"
RESET_LOG_REQUESTED="false"

# Debug function
debug() {
  if [[ "$DEBUG" == "true" ]]; then
    echo "[DEBUG] $*" >&2 # Debug output remains in English for technical users
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
    target_input_path="."
  fi

  if ! abs_target_input_path=$(readlink -f "$target_input_path"); then
    echo "❌ Fehler: Eingabepfad '$target_input_path' für das Zurücksetzen ist ungültig oder existiert nicht." >&2
    exit 1
  fi

  local log_file_name=".wolfgang_run_log.jsonl"
  local log_to_reset="${abs_target_input_path}/${log_file_name}"

  debug "Attempting to reset log file at: $log_to_reset"

  if [[ -f "$log_to_reset" ]]; then
    if rm "$log_to_reset"; then
      echo "✅ Log-Datei '$log_to_reset' wurde erfolgreich gelöscht."
    else
      echo "❌ Fehler: Log-Datei '$log_to_reset' konnte nicht gelöscht werden. Berechtigungen prüfen." >&2
      exit 1
    fi
  else
    echo "ℹ️ Log-Datei '$log_to_reset' nicht gefunden. Nichts zum Zurücksetzen."
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
    echo "⚠️ Warnung: Befehl 'jq' nicht gefunden. Protokollierung und Überspringen bereits protokollierter Dateien wird deaktiviert." >&2
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

  echo "Starte Bildkonvertierung - Präfix: '$CUSTOM_BASE', längere Bildseite: $MAX_SIZE px..."
  if [[ -n "$GLOBAL_KEYWORDS_STRING" ]]; then
    echo "Globale Schlüsselwörter aktiv (aus $global_keyword_file_path): $GLOBAL_KEYWORDS_STRING"
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
        echo "ℹ️ Bereits verarbeitet (protokolliert): $img_abs_path"
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
      echo "⚠️ Dimensionen für $img_abs_path konnten nicht ermittelt werden. Werden übersprungen."
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
        echo "⚠️ Kritisch: Konnte nach $iterator Versuchen keinen eindeutigen Dateinamen für '$img_abs_path' finden. Überspringe dieses Bild." >&2
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
      echo "❌ Fehler beim Konvertieren von '$img_abs_path' zu JPG. Magick Exit-Code: $?" >&2
    fi

    debug "Converting '$img_abs_path' to '$output_webp_abs_path'"
    if magick "$img_abs_path" $convert_cmd "$output_webp_abs_path"; then
      debug "Successfully converted '$img_abs_path' to '$output_webp_abs_path'"
      success_webp=true
    else
      echo "❌ Fehler beim Konvertieren von '$img_abs_path' zu WebP. Magick Exit-Code: $?" >&2
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
    echo "✔ Konvertiert: $REL_PATH → $FOLDERTIMESTAMP_NAME/$user_msg_jpg_path & $FOLDERTIMESTAMP_NAME/$user_msg_webp_path"
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
  echo "✅ Alle Bilder erfolgreich konvertiert! Ausgabe im Ordner '$output_folder_path'."
  if [[ "$JQ_AVAILABLE" == "true" ]]; then
    echo "ℹ️ Details des Durchlaufs sind hier protokolliert: $log_file_path"
  fi
}

wizard_mode() {
  echo "Moin! Lass uns ein paar Bilder fürs Web vorbereiten."

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
      echo "Du hast Wolfgang schonmal in diesem Verzeichnis mit folgenden Einstellungen ausgeführt:"
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
        if [[ "$aof" == "y" ]]; then aof_display="j"; fi # j for Ja
        echo "$((i + 1))) Präfix: '$cb', Dimension: $ms px, Dateiname anhängen: $aof_display"
      done

      while true; do
        read -rp "Wähle eine dieser Voreinstellungen (1-$preset_count) oder überspringen mit (N)ein: " user_choice
        if [[ "$user_choice" =~ ^[Nn]$ ]]; then
          echo "Wir fahren mit manueller Eingabe der Einstellungen fort."
          settings_from_preset="false"
          break
        elif [[ "$user_choice" =~ ^[0-9]+$ && "$user_choice" -ge 1 && "$user_choice" -le "$preset_count" ]]; then
          local selected_index=$((user_choice - 1))
          CUSTOM_BASE="${displayed_options_cb[$selected_index]}"
          MAX_SIZE="${displayed_options_ms[$selected_index]}"
          ADD_OLD_FILENAME="${displayed_options_aof[$selected_index]}"
          local aof_confirm_display="n"
          if [[ "$ADD_OLD_FILENAME" == "y" ]]; then aof_confirm_display="j"; fi
          echo "Voreinstellung verwendet: Präfix: '$CUSTOM_BASE', Dimension: $MAX_SIZE px, Dateiname anhängen: $aof_confirm_display"
          settings_from_preset="true"
          break
        else
          echo "Ungültige Auswahl. Bitte geben Sie eine Zahl zwischen 1 und $preset_count ein, oder N."
        fi
      done
    fi
  else
    debug "Wizard: No log file found, log is empty, or jq not available. Skipping presets."
  fi

  echo "---------------------------------------"

  if [[ "$settings_from_preset" != "true" ]]; then
    read -rp "Gebe den Basisnamen für deine Bilder ein (z.B. 'arismedia_'): " CUSTOM_BASE
    if [[ -z "$CUSTOM_BASE" ]]; then
      echo "FEHLER: Basisname ist erforderlich." >&2
      exit 1
    fi
    echo "---------------------------------------"

    while true; do
      read -rp "Geb den Pixelwert für die längere Seite des Bildes an (z.B. 1400): " MAX_SIZE
      if [[ "$MAX_SIZE" =~ ^[0-9]+$ && "$MAX_SIZE" -gt 0 ]]; then break; else echo "NICHT GANZ: Bitte geb eine gültige positive Zahl ein!"; fi
    done
    echo "---------------------------------------"

    while true; do
      echo "Soll der alte Dateiname an den neuen angehängt werden? (j/n)"
      read -n 1 -r user_aof_choice
      echo
      case "$user_aof_choice" in
      [JjYy]) # Accept j/J for Ja and y/Y for Yes
        ADD_OLD_FILENAME="y"
        break
        ;;
      [Nn])
        ADD_OLD_FILENAME="n"
        break
        ;;
      *) echo "Bitte 'j' oder 'n', um fortzufahren." ;;
      esac
    done
  fi

  echo "---------------------------------------"
  echo "Schlüsselwörter werden automatisch erkannt:"
  echo "  - Globale Schlüsselwörter: Aus der zuletzt geänderten *.txt-Datei im aktuellen Verzeichnis (wo das Skript ausgeführt wird)."
  echo "  - Lokale Schlüsselwörter: Aus der zuletzt geänderten *.txt-Datei im jeweiligen Bildverzeichnis."
  echo "---------------------------------------"

  convert
  exit 0
}

show_help() {
  echo "WOLFGANG - Web Optimierte Leichtgewichtige Schnelle Grafiken Analysator und Generator"
  echo ""
  echo "VERWENDUNG: wolfgang [OPTIONEN] [EINGABEPFAD]"
  echo ""
  echo "OPTIONEN:"
  echo "  -n, --name BASISNAME       Benutzerdefiniertes Präfix für Ergebnisdateien (Standard: '$CUSTOM_BASE')"
  echo "  -h, --help, -man, --man    Diese Hilfemeldung"
  echo "  -d, --dimension PIXEL      Längste Seite in Pixeln (Standard: $MAX_SIZE)"
  echo "  -a, --append               Hängt den ursprünglichen Dateinamen (ohne Erweiterung) an den neuen Namen an."
  echo "  -r, --reset                Löscht die .wolfgang_run_log.jsonl im EINGABEPFAD (oder aktuellem Verzeichnis)."
  echo "  --debug                    Aktiviert detaillierte Debug-Ausgabe."
  echo ""
  echo "SCHLÜSSELWORTERKENNUNG:"
  echo "  Schlüsselwörter werden automatisch aus '*.txt'-Dateien erkannt:"
  echo "  1. Globale Schlüsselwörter: Die zuletzt geänderte '*.txt'-Datei im Verzeichnis,"
  echo "     in dem das Skript ausgeführt wird, liefert globale Schlüsselwörter für alle Bilder."
  echo "  2. Lokale Schlüsselwörter: Für jedes Bild liefert die zuletzt geänderte '*.txt'-Datei"
  echo "     in seinem eigenen Verzeichnis lokale Schlüsselwörter für dieses Bild und andere im selben Verzeichnis."
  echo "     (Wenn diese Datei mit der globalen Schlüsselwortdatei identisch ist, wird sie nicht erneut hinzugefügt)."
  echo "  Schlüsselwörter in diesen Dateien sollten zeilenweise angegeben werden. Wörter in einer Zeile werden mit Unterstrichen verbunden."
  echo ""
  echo "AUSGABEORDNER:"
  echo "  Konvertierte Bilder werden in einem Ordner namens 'JJJJ-MM-TT_HH-MM-SS-wolfgang' gespeichert (erstellt im aktuellen Verzeichnis)."
  echo "  Das Skript überspringt automatisch alle Unterverzeichnisse, die auf '-wolfgang' enden, um eine erneute Verarbeitung zu vermeiden."
  echo ""
  echo "PROTOKOLLIERUNG (Benötigt 'jq' Kommandozeilen-JSON-Prozessor):"
  echo "  Wenn 'jq' installiert ist, werden die Einstellungen jedes Durchlaufs und die verarbeiteten Dateien in '.wolfgang_run_log.jsonl'"
  echo "  im Stammverzeichnis des EINGABEPFADS protokolliert. Das Skript verwendet dieses Protokoll, um bereits konvertierte Originaldateien zu überspringen."
  echo "  Der Assistentenmodus bietet an, Einstellungen von früheren Durchläufen im aktuellen Verzeichnis wiederzuverwenden, falls verfügbar."
  echo ""
  echo "ARGUMENTE:"
  echo "  EINGABEPFAD                Verzeichnis, das die zu konvertierenden Bilder enthält"
  echo "                             (Standard: Aktuelles Verzeichnis)"
  exit 0
}

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
    if [[ -z "$2" || "$2" == -* ]]; then
      echo "❌ Fehler: Fehlender Wert für Option $1." >&2
      exit 1
    fi
    if ! [[ "$2" =~ ^[0-9]+$ && "$2" -gt 0 ]]; then
      echo "❌ Fehler: Dimensionswert für $1 muss eine positive Ganzzahl sein. Erhalten: '$2'" >&2
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

if [[ ${#POSITIONAL_ARGS[@]} -eq 1 ]]; then
  INPUT_PATH="${POSITIONAL_ARGS[0]}"
elif [[ ${#POSITIONAL_ARGS[@]} -gt 1 ]]; then
  echo "❌ Zu viele Argumente. Nur ein EINGABEPFAD ist erlaubt. Siehe --help."
  exit 1
else
  :
fi

if [[ "$RESET_LOG_REQUESTED" == "true" ]]; then
  debug "Reset log requested for INPUT_PATH: $INPUT_PATH"
  reset_log "$INPUT_PATH"
fi

debug "Initial Configuration (after potential wizard/args):"
debug "  MAX_SIZE: $MAX_SIZE"
debug "  ADD_OLD_FILENAME: $ADD_OLD_FILENAME"
debug "  INPUT_PATH: $INPUT_PATH"
debug "  CUSTOM_BASE: $CUSTOM_BASE"
debug "  DEBUG: $DEBUG"
debug "  CWD for global keywords & output folder: $(pwd)"

if [[ "$original_arg_count" -eq 0 ]]; then
  wizard_mode
else
  if [[ "${#POSITIONAL_ARGS[@]}" -eq 1 || (-n "$INPUT_PATH" && "$INPUT_PATH" != ".") ]]; then
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
