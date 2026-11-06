#!/bin/bash

# Simple function for messages (used for most logs)
inform() {
  echo "wolfgang installer: $1"
}

# --- Check and Install Homebrew ---
if ! command -v brew &>/dev/null; then
  inform "Homebrew not found. Installing Homebrew..."
  if /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"; then
    inform "Homebrew installed."
  else
    echo "------------------------------------------------------------" >&2
    echo "ERROR: Homebrew installation failed." >&2
    echo "Please check the messages above. You might need to re-run this installer." >&2
    echo "------------------------------------------------------------" >&2
    exit 1
  fi
else
  inform "Homebrew is already installed."
fi

# --- Configure Homebrew for .zshrc ---
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
  inform "Adding Homebrew to your Zsh environment ($ZSHRC_FILE)..."
  echo "" >>"$ZSHRC_FILE"
  echo "# Homebrew environment" >>"$ZSHRC_FILE"
  echo "$SHELLENV_CMD" >>"$ZSHRC_FILE"
else
  inform "Homebrew environment already configured in $ZSHRC_FILE."
fi

eval "$(${BREW_PREFIX}/bin/brew shellenv)"
if ! command -v brew &>/dev/null; then
  echo "------------------------------------------------------------" >&2
  echo "ERROR: brew command not found even after setting up shellenv." >&2
  echo "Please check your Homebrew installation or terminal configuration." >&2
  echo "------------------------------------------------------------" >&2
  exit 1
fi

# --- Install Homebrew Packages ---
inform "Updating Homebrew and installing imagemagick and jq..."
if brew update && brew install imagemagick jq; then
  inform "imagemagick and jq are installed and up-to-date."
else
  echo "------------------------------------------------------------" >&2
  echo "WARNING: Failed to install or update imagemagick or jq." >&2
  echo "The 'wolfgang' script might not function correctly without them." >&2
  echo "Please check the messages above or try installing them manually:" >&2
  echo "  brew install imagemagick jq" >&2
  echo "------------------------------------------------------------" >&2
  # Not exiting, as wolfgang might still be partially usable or installed.
fi

# --- Locate, Make Executable, and Move Wolfgang Script ---
TARGET_DIR="/usr/local/bin"
TARGET_SCRIPT_NAME="wolfgang"
SOURCE_SCRIPT_NAME="wolfgang.sh"

inform "Looking for '$SOURCE_SCRIPT_NAME' in the current directory..."

wolfgang_script_source="./$SOURCE_SCRIPT_NAME"

if [[ ! -f "$wolfgang_script_source" ]]; then
  echo "------------------------------------------------------------" >&2
  echo "ERROR: '$SOURCE_SCRIPT_NAME' was not found in the current directory" >&2
  echo "(where you ran this installer)." >&2
  echo "Please run this installer from the directory that contains '$SOURCE_SCRIPT_NAME'." >&2
  echo "------------------------------------------------------------" >&2
  exit 1
fi

inform "Processing script: $wolfgang_script_source"

chmod +x "$wolfgang_script_source"
if [ $? -ne 0 ]; then
  echo "------------------------------------------------------------" >&2
  echo "ERROR: Failed to make $wolfgang_script_source executable." >&2
  echo "------------------------------------------------------------" >&2
  exit 1
else
  inform "$wolfgang_script_source made executable."
fi

if [ ! -d "$TARGET_DIR" ]; then
  inform "$TARGET_DIR does not exist. Attempting to create it (you may be prompted for your password)..."
  if sudo mkdir -p "$TARGET_DIR"; then
    inform "$TARGET_DIR created successfully."
  else
    echo "------------------------------------------------------------" >&2
    echo "ERROR: Failed to create $TARGET_DIR even with sudo." >&2
    echo "Please create it manually (e.g., sudo mkdir -p $TARGET_DIR) and ensure it's writable, then re-run." >&2
    echo "------------------------------------------------------------" >&2
    exit 1
  fi
fi

inform "Attempting to remove any existing '$TARGET_SCRIPT_NAME*' scripts from $TARGET_DIR..."
found_for_removal=$(find "$TARGET_DIR" -maxdepth 1 -type f -name "${TARGET_SCRIPT_NAME}*")

if [[ -n "$found_for_removal" ]]; then
  while IFS= read -r old_script; do
    inform "Removing $old_script..."
    if rm -f "$old_script"; then
      inform "$old_script removed successfully."
    else
      inform "Failed to remove $old_script. Attempting with sudo (you may be prompted for your password)..."
      if sudo rm -f "$old_script"; then
        inform "$old_script removed successfully with sudo."
      else
        echo "------------------------------------------------------------" >&2
        echo "WARNING: Failed to remove $old_script even with sudo." >&2
        echo "You may need to remove it manually: sudo rm -f \"$old_script\"" >&2
        echo "------------------------------------------------------------" >&2
      fi
    fi
  done <<<"$found_for_removal"
else
  inform "No existing '$TARGET_SCRIPT_NAME*' scripts found in $TARGET_DIR to remove."
fi

inform "Copying $wolfgang_script_source to $TARGET_DIR/$TARGET_SCRIPT_NAME..."
if cp "$wolfgang_script_source" "$TARGET_DIR/$TARGET_SCRIPT_NAME"; then
  inform "$wolfgang_script_source copied to $TARGET_DIR/$TARGET_SCRIPT_NAME."
else
  inform "Failed to copy directly to $TARGET_DIR. Attempting with sudo (you may be prompted for your password)..."
  if sudo cp "$wolfgang_script_source" "$TARGET_DIR/$TARGET_SCRIPT_NAME"; then
    inform "$wolfgang_script_source copied to $TARGET_DIR/$TARGET_SCRIPT_NAME successfully with sudo."
  else
    echo "------------------------------------------------------------" >&2
    echo "ERROR: Failed to copy $wolfgang_script_source to $TARGET_DIR/$TARGET_SCRIPT_NAME even with sudo." >&2
    echo "Please check permissions for $TARGET_DIR or try manually:" >&2
    echo "  sudo cp \"$wolfgang_script_source\" \"$TARGET_DIR/$TARGET_SCRIPT_NAME\"" >&2
    echo "------------------------------------------------------------" >&2
    exit 1
  fi
fi

# --- Add PATHs to .zshrc ---
inform "Ensuring essential PATHs are in $ZSHRC_FILE..."
PATH_TO_ADD="$TARGET_DIR"
PATH_EXPORT_LINE="export PATH=\"${PATH_TO_ADD}:\$PATH\""

if [[ "$BREW_PREFIX" != "$PATH_TO_ADD" ]]; then
  if ! grep -Fxq "$PATH_EXPORT_LINE" "$ZSHRC_FILE" && ! grep -q "export PATH=.*${PATH_TO_ADD}:" "$ZSHRC_FILE"; then
    echo "" >>"$ZSHRC_FILE"
    echo "# Ensure $PATH_TO_ADD is in PATH for wolfgang script" >>"$ZSHRC_FILE"
    echo "$PATH_EXPORT_LINE" >>"$ZSHRC_FILE"
    inform "$PATH_TO_ADD added to PATH in $ZSHRC_FILE."
  else
    inform "$PATH_TO_ADD is already managed in PATH in $ZSHRC_FILE."
  fi
else
  inform "$PATH_TO_ADD (Homebrew path) is managed by 'brew shellenv' in $ZSHRC_FILE."
fi

# --- Final Success Message ---
# Using direct echo for better formatting control here
echo ""
echo "============================================================"
echo "🎉 Installation Complete! Wolfgang is (almost) ready! 🎉"
echo "============================================================"
echo ""
echo "The 'wolfgang' script and its helper tools have been installed."
echo ""
echo "------------------------------------------------------------"
echo "🔴 IMPORTANT FINAL STEP TO USE 'wolfgang' 🔴"
echo "------------------------------------------------------------"
echo ""
echo "To make the 'wolfgang' command work in your terminal,"
echo "you need to update your current session."
echo ""
echo "Please do ONE of the following:"
echo ""
echo "  OPTION 1: Type this command and press Enter:"
echo "            source $ZSHRC_FILE"
echo ""
echo "  OPTION 2: Close this terminal window and open a brand new one."
echo ""
echo "------------------------------------------------------------"
echo ""
echo "After you've done one of these steps, you can run Wolfgang"
echo "by simply typing this in any new terminal window:"
echo ""
echo "  wolfgang"
echo ""
echo "============================================================"
echo ""

exit 0
