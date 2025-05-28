#!/bin/bash

# Einfache Funktion für Nachrichten (für die meisten Logs verwendet)
inform() {
  echo "wolfgang Installationsprogramm: $1"
}

# --- Homebrew prüfen und installieren ---
if ! command -v brew &>/dev/null; then
  inform "Homebrew nicht gefunden. Homebrew wird installiert..."
  # Homebrew-Installationsprogramm nicht-interaktiv ausführen
  if /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"; then
    inform "Homebrew wurde installiert."
  else
    echo "------------------------------------------------------------" >&2
    echo "FEHLER: Homebrew-Installation fehlgeschlagen." >&2
    echo "Bitte überprüfe die Meldungen oben. Möglicherweise musst Du dieses Installationsprogramm erneut ausführen." >&2
    echo "------------------------------------------------------------" >&2
    exit 1
  fi
else
  inform "Homebrew ist bereits installiert."
fi

# --- Homebrew für .zshrc konfigurieren ---
BREW_PREFIX=""
# Architekturspezifischen Pfad für Brew ermitteln
if [[ "$(uname -m)" == "arm64" ]]; then # Apple Silicon
  BREW_PREFIX="/opt/homebrew"
else # Intel
  BREW_PREFIX="/usr/local"
fi

SHELLENV_CMD="eval \"\$(${BREW_PREFIX}/bin/brew shellenv)\""
ZSHRC_FILE="$HOME/.zshrc" # Pfad zur .zshrc Datei

# Sicherstellen, dass .zshrc existiert
touch "$ZSHRC_FILE"

# Brew shellenv zu .zshrc hinzufügen, falls noch nicht vorhanden
if ! grep -qF "${BREW_PREFIX}/bin/brew shellenv" "$ZSHRC_FILE"; then
  inform "Homebrew wird zu Deiner Zsh-Umgebung hinzugefügt ($ZSHRC_FILE)..."
  echo "" >>"$ZSHRC_FILE" # Eine Leerzeile zur Trennung
  echo "# Homebrew Umgebung" >>"$ZSHRC_FILE"
  echo "$SHELLENV_CMD" >>"$ZSHRC_FILE"
else
  inform "Die Homebrew-Umgebung ist bereits in $ZSHRC_FILE konfiguriert."
fi

# Brew shellenv für die aktuelle Skriptausführung evaluieren
# Dies macht den 'brew'-Befehl verfügbar, falls er gerade erst installiert wurde.
eval "$(${BREW_PREFIX}/bin/brew shellenv)"
if ! command -v brew &>/dev/null; then
  echo "------------------------------------------------------------" >&2
  echo "FEHLER: Der Befehl 'brew' wurde auch nach dem Einrichten von shellenv nicht gefunden." >&2
  echo "Bitte überprüfe Deine Homebrew-Installation oder Terminal-Konfiguration." >&2
  echo "------------------------------------------------------------" >&2
  exit 1
fi

# --- Homebrew-Pakete installieren ---
inform "Homebrew wird aktualisiert und imagemagick sowie jq werden installiert..."
if brew update && brew install imagemagick jq; then
  inform "imagemagick und jq wurden erfolgreich installiert/aktualisiert."
else
  echo "------------------------------------------------------------" >&2
  echo "WARNUNG: Fehler bei der Installation oder Aktualisierung von imagemagick oder jq." >&2
  echo "Das 'wolfgang'-Skript funktioniert möglicherweise nicht korrekt ohne sie." >&2
  echo "Bitte überprüfe die Meldungen oben oder versuche, sie manuell zu installieren:" >&2
  echo "  brew install imagemagick jq" >&2
  echo "------------------------------------------------------------" >&2
  # Kein Abbruch, da wolfgang möglicherweise teilweise nutzbar oder installiert ist.
fi

# --- Wolfgang-Skript finden, ausführbar machen und verschieben ---
TARGET_DIR="/usr/local/bin"               # Zielverzeichnis
TARGET_SCRIPT_NAME="wolfgang"             # Zielname des Skripts
SOURCE_SCRIPT_PATTERN_NAME="wolfgang*.sh" # Suchmuster für das Quellskript

inform "Suche nach einem Skript, das auf '$SOURCE_SCRIPT_PATTERN_NAME' im aktuellen Verzeichnis passt..."

# Alle passenden Skripte finden und in einem Array speichern
# nullglob verhindert Fehler, wenn keine Übereinstimmung gefunden wird
shopt -s nullglob
wolfgang_scripts_found=(./wolfgang*.sh) # Erweitert zu passenden Dateien im aktuellen Verzeichnis
shopt -u nullglob                       # nullglob zurücksetzen

num_found_scripts=${#wolfgang_scripts_found[@]} # Anzahl gefundener Skripte
wolfgang_script_source=""                       # Initialisieren

if [[ $num_found_scripts -eq 0 ]]; then
  echo "------------------------------------------------------------" >&2
  echo "FEHLER: Kein Skript beginnend mit 'wolfgang' und endend mit '.sh' gefunden" >&2
  echo "im aktuellen Verzeichnis (wo Du dieses Installationsprogramm ausgeführt hast)." >&2
  echo "Bitte stelle sicher, dass Deine '$SOURCE_SCRIPT_PATTERN_NAME'-Datei hier vorhanden ist." >&2
  echo "------------------------------------------------------------" >&2
  exit 1
elif [[ $num_found_scripts -eq 1 ]]; then
  wolfgang_script_source="${wolfgang_scripts_found[0]}"
  inform "Ein Skript gefunden: $wolfgang_script_source"
else
  echo "------------------------------------------------------------" >&2
  echo "FEHLER: Mehrere Skripte beginnend mit 'wolfgang' wurden hier gefunden:" >&2
  for script_path in "${wolfgang_scripts_found[@]}"; do
    echo "  - $script_path" >&2
  done
  echo "Bitte stelle sicher, dass sich nur EIN solches Skript (z.B. ./wolfgang.sh)" >&2
  echo "in diesem Verzeichnis befindet und führe das Installationsprogramm erneut aus." >&2
  echo "------------------------------------------------------------" >&2
  exit 1
fi

inform "Verarbeite Skript: $wolfgang_script_source"

# Skript ausführbar machen
chmod +x "$wolfgang_script_source"
if [ $? -ne 0 ]; then
  echo "------------------------------------------------------------" >&2
  echo "FEHLER: Konnte $wolfgang_script_source nicht ausführbar machen." >&2
  echo "------------------------------------------------------------" >&2
  exit 1 # Kritischer Schritt
else
  inform "$wolfgang_script_source wurde ausführbar gemacht."
fi

# Sicherstellen, dass TARGET_DIR existiert; ggf. mit sudo erstellen
if [ ! -d "$TARGET_DIR" ]; then
  inform "$TARGET_DIR existiert nicht. Versuche, es zu erstellen (Du wirst möglicherweise nach Deinem Passwort gefragt)..."
  if sudo mkdir -p "$TARGET_DIR"; then
    inform "$TARGET_DIR wurde erfolgreich erstellt."
    # Optional: Besitzer/Berechtigungen setzen, falls nötig, z.B. sudo chown $(whoami) "$TARGET_DIR"
    # Homebrew verwaltet jedoch normalerweise die Berechtigungen von /usr/local korrekt.
  else
    echo "------------------------------------------------------------" >&2
    echo "FEHLER: Konnte $TARGET_DIR auch mit sudo nicht erstellen." >&2
    echo "Bitte erstelle es manuell (z.B. sudo mkdir -p $TARGET_DIR), stelle sicher, dass es beschreibbar ist, und führe das Skript erneut aus." >&2
    echo "------------------------------------------------------------" >&2
    exit 1
  fi
fi

# Bestehende 'wolfgang*'-Skripte aus /usr/local/bin entfernen
inform "Versuche, alle existierenden '$TARGET_SCRIPT_NAME*'-Skripte aus $TARGET_DIR zu entfernen..."
# Verwendung von find und einer Schleife für die Entfernung, mit sudo-Fallback.
found_for_removal=$(find "$TARGET_DIR" -maxdepth 1 -type f -name "${TARGET_SCRIPT_NAME}*")

if [[ -n "$found_for_removal" ]]; then
  while IFS= read -r old_script; do
    inform "Entferne $old_script..."
    if rm -f "$old_script"; then
      inform "$old_script erfolgreich entfernt."
    else
      inform "Fehler beim Entfernen von $old_script. Versuche es mit sudo (Du wirst möglicherweise nach Deinem Passwort gefragt)..."
      if sudo rm -f "$old_script"; then
        inform "$old_script erfolgreich mit sudo entfernt."
      else
        echo "------------------------------------------------------------" >&2
        echo "WARNUNG: Konnte $old_script auch mit sudo nicht entfernen." >&2
        echo "Du musst es möglicherweise manuell entfernen: sudo rm -f \"$old_script\"" >&2
        echo "------------------------------------------------------------" >&2
      fi
    fi
  done <<<"$found_for_removal"
else
  inform "Keine existierenden '$TARGET_SCRIPT_NAME*'-Skripte in $TARGET_DIR zum Entfernen gefunden."
fi

# Neues Skript kopieren, bei Fehlschlag mit sudo versuchen
inform "Kopiere $wolfgang_script_source nach $TARGET_DIR/$TARGET_SCRIPT_NAME..."
if cp "$wolfgang_script_source" "$TARGET_DIR/$TARGET_SCRIPT_NAME"; then
  inform "$wolfgang_script_source wurde nach $TARGET_DIR/$TARGET_SCRIPT_NAME kopiert."
else
  inform "Direktes Kopieren nach $TARGET_DIR fehlgeschlagen. Versuche es mit sudo (Du wirst möglicherweise für den Kopiervorgang nach Deinem Passwort gefragt)..."
  if sudo cp "$wolfgang_script_source" "$TARGET_DIR/$TARGET_SCRIPT_NAME"; then
    inform "$wolfgang_script_source wurde erfolgreich mit sudo nach $TARGET_DIR/$TARGET_SCRIPT_NAME kopiert."
  else
    echo "------------------------------------------------------------" >&2
    echo "FEHLER: Konnte $wolfgang_script_source auch mit sudo nicht nach $TARGET_DIR/$TARGET_SCRIPT_NAME kopieren." >&2
    echo "Bitte überprüfe die Berechtigungen für $TARGET_DIR oder versuche es manuell:" >&2
    echo "  sudo cp \"$wolfgang_script_source\" \"$TARGET_DIR/$TARGET_SCRIPT_NAME\"" >&2
    echo "------------------------------------------------------------" >&2
    exit 1 # Kritischer Schritt
  fi
fi

# --- PATHs zu .zshrc hinzufügen ---
inform "Stelle sicher, dass wichtige PATHs in $ZSHRC_FILE eingetragen sind..."
PATH_TO_ADD="$TARGET_DIR" # /usr/local/bin
PATH_EXPORT_LINE="export PATH=\"${PATH_TO_ADD}:\$PATH\""

# Prüfen, ob /usr/local/bin hinzugefügt werden muss (falls nicht der Homebrew-Pfad)
if [[ "$BREW_PREFIX" != "$PATH_TO_ADD" ]]; then # Nur /usr/local/bin hinzufügen, wenn es nicht bereits der Haupt-Brew-Binärpfad ist
  # Prüfen, ob die Zeile exakt oder der Pfad bereits in einer PATH-Zeile vorhanden ist.
  if ! grep -Fxq "$PATH_EXPORT_LINE" "$ZSHRC_FILE" && ! grep -q "export PATH=.*${PATH_TO_ADD}:" "$ZSHRC_FILE"; then
    echo "" >>"$ZSHRC_FILE" # Leerzeile für bessere Lesbarkeit
    echo "# Stelle sicher, dass $PATH_TO_ADD im PATH ist (für wolfgang Skript)" >>"$ZSHRC_FILE"
    echo "$PATH_EXPORT_LINE" >>"$ZSHRC_FILE"
    inform "$PATH_TO_ADD wurde zum PATH in $ZSHRC_FILE hinzugefügt."
  else
    inform "$PATH_TO_ADD wird bereits im PATH in $ZSHRC_FILE verwaltet."
  fi
else
  inform "$PATH_TO_ADD (Homebrew-Pfad auf diesem System) wird durch 'brew shellenv' in $ZSHRC_FILE verwaltet."
fi

# --- Abschließende Erfolgsmeldung ---
# Direkte echo-Befehle für bessere Formatierungskontrolle
echo ""
echo "============================================================"
echo "🎉 Installation abgeschlossen! Wolfgang ist (fast) bereit! 🎉"
echo "============================================================"
echo ""
echo "Das 'wolfgang'-Skript und seine Hilfsprogramme wurden installiert."
echo ""
echo "------------------------------------------------------------"
echo "🔴 WICHTIGER LETZTER SCHRITT, UM 'wolfgang' ZU NUTZEN 🔴"
echo "------------------------------------------------------------"
echo ""
echo "Damit der Befehl 'wolfgang' in Deinem Terminal funktioniert,"
echo "musst Du Deine aktuelle Sitzung aktualisieren."
echo ""
echo "Bitte führe EINEN der folgenden Schritte aus:"
echo ""
echo "  OPTION 1: Tippe diesen Befehl ein und drücke Enter:"
echo "            source $ZSHRC_FILE"
echo ""
echo "  OPTION 2: Schließe dieses Terminal-Fenster und öffne ein komplett Neues."
echo ""
echo "------------------------------------------------------------"
echo ""
echo "Nachdem Du einen dieser Schritte ausgeführt hast, kannst Du Wolfgang"
echo "starten, indem Du einfach Folgendes in ein neues Terminal-Fenster tippst:"
echo ""
echo "  wolfgang"
echo ""
echo "============================================================"
echo ""

exit 0
