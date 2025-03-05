# WOLFGANG - Web Optimized Lightweight Fast Graphics Analyser and Generator

Ein Helfertool, das Bilder mit Hilfe von imagemagick für das Web optimiert.

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
