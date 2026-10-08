#!/bin/sh
# Installs Sound Tools for this user, in ~/.local. Run it from the folder of the tarball.
# Run it again to update. To remove Sound Tools, delete the four paths it prints.
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
# The program and the Bun that runs the tools of a project, next to each other.
lib="$HOME/.local/lib/sound-tools"
program="$lib/sound-tools"
link="$HOME/.local/bin/sound-tools"
desktop="$HOME/.local/share/applications/sound-tools.desktop"
icon="$HOME/.local/share/icons/hicolor/512x512/apps/sound-tools.png"

mkdir -p "$lib" "$(dirname "$link")" "$(dirname "$desktop")" "$(dirname "$icon")"
# A copy and a rename, so a running Sound Tools does not stop the update.
for name in sound-tools bun; do
  cp "$here/$name" "$lib/$name.new"
  mv -f "$lib/$name.new" "$lib/$name"
done
# A link, as the app's own "Install command line tool" makes, so that item still works.
ln -sf "$program" "$link"
sed "s|^Exec=.*|Exec=\"$program\"|" "$here/sound-tools.desktop" > "$desktop"
cp "$here/sound-tools.png" "$icon"

echo "Installed Sound Tools:"
echo "  $lib"
echo "  $link"
echo "  $desktop"
echo "  $icon"
case ":$PATH:" in
  *":$HOME/.local/bin:"*) ;;
  *) echo "To run sound-tools in a terminal, add ~/.local/bin to your PATH." ;;
esac
