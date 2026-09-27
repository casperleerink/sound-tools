#!/bin/sh
# Installs Sound Tools for this user, in ~/.local. Run it from the folder of the tarball.
# Run it again to update. To remove Sound Tools, delete the four paths it prints.
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
program="$HOME/.local/lib/sound-tools/sound-tools"
link="$HOME/.local/bin/sound-tools"
desktop="$HOME/.local/share/applications/sound-tools.desktop"
icon="$HOME/.local/share/icons/hicolor/512x512/apps/sound-tools.png"

mkdir -p "$(dirname "$program")" "$(dirname "$link")" "$(dirname "$desktop")" "$(dirname "$icon")"
# A copy and a rename, so a running Sound Tools does not stop the update.
cp "$here/sound-tools" "$program.new"
mv -f "$program.new" "$program"
# A link, as the app's own "Install command line tool" makes, so that item still works.
ln -sf "$program" "$link"
sed "s|^Exec=.*|Exec=\"$program\"|" "$here/sound-tools.desktop" > "$desktop"
cp "$here/sound-tools.png" "$icon"

echo "Installed Sound Tools:"
echo "  $program"
echo "  $link"
echo "  $desktop"
echo "  $icon"
case ":$PATH:" in
  *":$HOME/.local/bin:"*) ;;
  *) echo "To run sound-tools in a terminal, add ~/.local/bin to your PATH." ;;
esac
