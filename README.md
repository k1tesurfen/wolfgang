# WOLFGANG - Web Optimized Lightweight Fast Graphics Analyser and Generator

Small little tool to prepare images for web use with [imagemagick](https://imagemagick.org/)

## Prerequisites

- [imagemagick](https://imagemagick.org/):
  for macos see: <https://formulae.brew.sh/formula/imagemagick#default>
  for linux check your corresponding package managers
  for windows, switch to either of the ones above

## Installation

USAGE: wolfgang [OPTIONS] [INPUT_PATH]

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
