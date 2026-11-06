# Wolfgang 1.x (Bash)

The original ImageMagick-based script, kept for reference. Wolfgang 2 (Rust, in `crates/`) replaces it.

- `wolfgang.sh` / `wolfgang-ger.sh`: the English and German script
- `install.sh` / `install-ger.sh`: the old installers (Homebrew + ImageMagick + jq)

Wolfgang 1.x renamed your *original* files (`DSC_4711.jpg` → `DSC_4711-0001.jpg`).
Wolfgang 2 never does that. To undo the old renames:

```bash
wolfgang legacy-reset ~/Pictures/trip        # shows what would change
wolfgang legacy-reset ~/Pictures/trip --yes  # renames back
```

Only files recorded in the old `.wolfgang_run_log.jsonl` are renamed back.
