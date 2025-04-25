# WOLFGANG - Web Optimized Lightweight Fast Graphics Analyser and Generator

Small little tool to prepare images for web use with [imagemagick](https://imagemagick.org/)

## Prerequisites

- [imagemagick](https://imagemagick.org/):
  - for macos see: <https://formulae.brew.sh/formula/imagemagick#default>
  - for linux check your corresponding package managers
  - for windows, switch to either of the ones before

## Installation

To install a bash script system wide on your MacOS machine you can place it in `/usr/local/bin/`.
Make sure the file is executable and run `chmod +x wolfgang.sh`. You can check if the file is executable by running `stat wolfgang.sh` or `ls -la` in your terminal.

USAGE: `wolfgang [OPTIONS] [INPUT_PATH]`

OPTIONS:  
-n, --name Custom Präfix für Dateinamen  
-h, --help, -man, --man Hilfe-Nachricht anzeigen  
-d, --dimension PIXEL Maximale Seitenlänge in Pixel (Standard: 1400)  
-k, --keywords FILE Pfad zur Keyword Datei.  
(Falls Keyword Datei in deinem aktuellen Verzeichnis ist,
dann reicht einfach nur dateiname.md)
-a, --append Ursprünglichen Dateinamen anhängen  
--debug Detaillierte Debug-Ausgaben aktivieren

ARGUMENTE:  
INPUT_PATH Verzeichnis mit zu optimierenden Bildern
(Standard: Aktuelles Verzeichnis)
