# WOLFGANG - Web Optimized Lightweight Fast Graphics Analyser and Generator

Small little tool to prepare images for web use with [imagemagick](https://imagemagick.org/)

## Installation

Execute the installer script to install wolfgang and it's dependencies.

WOLFGANG - Web Optimized Lightweight Fast Graphics Analyser and Generator"

USAGE: wolfgang [OPTIONS] [INPUT_PATH]"

OPTIONS:"
-n, --name PREFIX Custom prefix for resulting files (Default: '$CUSTOM_BASE')"
-h, --help, -man, --man This help message"
-d, --dimension PIXEL Longest side in pixels (Default: $MAX_SIZE)"
-a, --append Append the original filename (without extension) to the new name."
-r, --reset Deletes the .wolfgang_run_log.jsonl in the INPUT_PATH (or current dir)."
--debug Enable detailed debug output."

OUTPUT FILENAME STRUCTURE:"
[LOCAL_KEYWORDS]-[PREFIX]-[OPTIONAL_OLD_FILENAME]-[INDEX].ext"

- LOCAL*KEYWORDS: Derived from the last modified '\*.txt' in the image's directory."
  Words on one line joined by '*', lines joined by '-'."
- PREFIX: The custom name set via -n or wizard (e.g., '$CUSTOM_BASE')."
- OPTIONAL_OLD_FILENAME: Original filename (sans extension) if -a is used."
- INDEX: A persistent 4-digit counter (e.g., 0001, 0002), unique per input directory log."

KEYWORD DETECTION FOR FILENAMES (LOCAL KEYWORDS):"
For each image, the last modified '_.txt' file in its own directory provides local keywords."
Keywords within these files should be one per line. Words on a single line will be joined by underscores."
Global keywords (from _.txt in script's CWD) are logged but NOT used in filenames."

OUTPUT FOLDER:"
Converted images are saved in a directory named 'YYYY-MM-DD_HH-M-SS-wolfgang' (created in current dir)."
The script will automatically skip any subdirectories ending with '-wolfgang' to avoid reprocessing."

RUN LOGGING (Requires 'jq' command-line JSON processor):"
If 'jq' is installed, each run's settings and processed files are logged to '.wolfgang_run_log.jsonl'"
in the root of the INPUT_PATH. The script uses this log to skip already converted files and to"
determine the next available index for filenames."
Wizard mode will offer to reuse settings from previous runs in the current directory if available."

ARGUMENTS:"
INPUT_PATH Directory that contains the images to convert"
(Default: Current directory)"
