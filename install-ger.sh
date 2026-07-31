#!/bin/bash

# Einfache Funktion für Nachrichten (für die meisten Logs verwendet)
inform() {
  echo "wolfgang Installationsprogramm: $1"
}

# --- Homebrew prüfen und installieren ---
if ! command -v brew &>/dev/null; then
  inform "Homebrew nicht gefunden. Homebrew wird installiert..."
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
if [[ "$(uname -m)" == "arm64" ]]; then # Apple Silicon
  BREW_PREFIX="/opt/homebrew"
else # Intel
  BREW_PREFIX="/usr/local"
fi

SHELLENV_CMD="eval \"\$(${BREW_PREFIX}/bin/brew shellenv)\""
ZSHRC_FILE="$HOME/.zshrc"

touch "$ZSHRC_FILE"

if ! grep -qF "${BREW_PREFIX}/bin/brew shellenv" "$ZSHRC_FILE"; then
  inform "Homebrew wird zu Deiner Zsh-Umgebung hinzugefügt ($ZSHRC_FILE)..."
  echo "" >>"$ZSHRC_FILE"
  echo "# Homebrew Umgebung" >>"$ZSHRC_FILE"
  echo "$SHELLENV_CMD" >>"$ZSHRC_FILE"
else
  inform "Die Homebrew-Umgebung ist bereits in $ZSHRC_FILE konfiguriert."
fi

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
  inform "imagemagick und jq sind installiert und aktuell."
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
TARGET_DIR="/usr/local/bin"
TARGET_SCRIPT_NAME="wolfgang"
SOURCE_SCRIPT_NAME="wolfgang-ger.sh"

inform "Suche nach '$SOURCE_SCRIPT_NAME' im aktuellen Verzeichnis..."

wolfgang_script_source="./$SOURCE_SCRIPT_NAME"

if [[ ! -f "$wolfgang_script_source" ]]; then
  echo "------------------------------------------------------------" >&2
  echo "FEHLER: '$SOURCE_SCRIPT_NAME' wurde nicht im aktuellen Verzeichnis gefunden" >&2
  echo "(dort, wo Du dieses Installationsprogramm ausgeführt hast)." >&2
  echo "Bitte führe dieses Installationsprogramm in dem Verzeichnis aus, das '$SOURCE_SCRIPT_NAME' enthält." >&2
  echo "------------------------------------------------------------" >&2
  exit 1
fi

inform "Verarbeite Skript: $wolfgang_script_source"

chmod +x "$wolfgang_script_source"
if [ $? -ne 0 ]; then
  echo "------------------------------------------------------------" >&2
  echo "FEHLER: Konnte $wolfgang_script_source nicht ausführbar machen." >&2
  echo "------------------------------------------------------------" >&2
  exit 1
else
  inform "$wolfgang_script_source wurde ausführbar gemacht."
fi

if [ ! -d "$TARGET_DIR" ]; then
  inform "$TARGET_DIR existiert nicht. Versuche, es zu erstellen (Du wirst möglicherweise nach Deinem Passwort gefragt)..."
  if sudo mkdir -p "$TARGET_DIR"; then
    inform "$TARGET_DIR wurde erfolgreich erstellt."
  else
    echo "------------------------------------------------------------" >&2
    echo "FEHLER: Konnte $TARGET_DIR auch mit sudo nicht erstellen." >&2
    echo "Bitte erstelle es manuell (z.B. sudo mkdir -p $TARGET_DIR), stelle sicher, dass es beschreibbar ist, und führe das Skript erneut aus." >&2
    echo "------------------------------------------------------------" >&2
    exit 1
  fi
fi

inform "Versuche, alle existierenden '$TARGET_SCRIPT_NAME*'-Skripte aus $TARGET_DIR zu entfernen..."
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

inform "Kopiere $wolfgang_script_source nach $TARGET_DIR/$TARGET_SCRIPT_NAME..."
if cp "$wolfgang_script_source" "$TARGET_DIR/$TARGET_SCRIPT_NAME"; then
  inform "$wolfgang_script_source wurde nach $TARGET_DIR/$TARGET_SCRIPT_NAME kopiert."
else
  inform "Direktes Kopieren nach $TARGET_DIR fehlgeschlagen. Versuche es mit sudo (Du wirst möglicherweise nach Deinem Passwort gefragt)..."
  if sudo cp "$wolfgang_script_source" "$TARGET_DIR/$TARGET_SCRIPT_NAME"; then
    inform "$wolfgang_script_source wurde erfolgreich mit sudo nach $TARGET_DIR/$TARGET_SCRIPT_NAME kopiert."
  else
    echo "------------------------------------------------------------" >&2
    echo "FEHLER: Konnte $wolfgang_script_source auch mit sudo nicht nach $TARGET_DIR/$TARGET_SCRIPT_NAME kopieren." >&2
    echo "Bitte überprüfe die Berechtigungen für $TARGET_DIR oder versuche es manuell:" >&2
    echo "  sudo cp \"$wolfgang_script_source\" \"$TARGET_DIR/$TARGET_SCRIPT_NAME\"" >&2
    echo "------------------------------------------------------------" >&2
    exit 1
  fi
fi

# --- PATHs zu .zshrc hinzufügen ---
inform "Stelle sicher, dass wichtige PATHs in $ZSHRC_FILE eingetragen sind..."
PATH_TO_ADD="$TARGET_DIR"
PATH_EXPORT_LINE="export PATH=\"${PATH_TO_ADD}:\$PATH\""

if [[ "$BREW_PREFIX" != "$PATH_TO_ADD" ]]; then
  if ! grep -Fxq "$PATH_EXPORT_LINE" "$ZSHRC_FILE" && ! grep -q "export PATH=.*${PATH_TO_ADD}:" "$ZSHRC_FILE"; then
    echo "" >>"$ZSHRC_FILE"
    echo "# Stelle sicher, dass $PATH_TO_ADD im PATH ist (für wolfgang Skript)" >>"$ZSHRC_FILE"
    echo "$PATH_EXPORT_LINE" >>"$ZSHRC_FILE"
    inform "$PATH_TO_ADD wurde zum PATH in $ZSHRC_FILE hinzugefügt."
  else
    inform "$PATH_TO_ADD wird bereits im PATH in $ZSHRC_FILE verwaltet."
  fi
else
  inform "$PATH_TO_ADD (Homebrew-Pfad) wird durch 'brew shellenv' in $ZSHRC_FILE verwaltet."
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
