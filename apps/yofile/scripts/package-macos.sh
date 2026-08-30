#!/bin/bash
set -euo pipefail

# YoFile is a launcher shell that bundles the three tool apps as-is and opens
# them from its window. Build each tool app first (their own `npm run package:mac`),
# then this script embeds them into YoFile.app, signs, builds a DMG, and notarizes.

project_root="$(cd "$(dirname "$0")/.." && pwd)"
apps_root="$(cd "$project_root/.." && pwd)"
app_path="$project_root/src-tauri/target/release/bundle/macos/YoFile.app"
resources_apps="$app_path/Contents/Resources/apps"
dmg_dir="$project_root/src-tauri/target/release/bundle/dmg"
architecture="$(uname -m)"
app_version="$(awk -F'"' '/"version"/ { print $4; exit }' "$project_root/src-tauri/tauri.conf.json")"
dmg_path="$dmg_dir/YoFile_${app_version}_${architecture}.dmg"
signing_identity="${YOFILE_SIGNING_IDENTITY:--}"
notary_profile="${YOFILE_NOTARY_PROFILE:-}"

# Prebuilt tool bundles. Override with YOFILE_KILN_APP / YOFILE_ROLLCALL_APP / YOFILE_QUIRE_APP.
kiln_app="${YOFILE_KILN_APP:-$apps_root/kiln/src-tauri/target/release/bundle/macos/Kiln.app}"
rollcall_app="${YOFILE_ROLLCALL_APP:-$apps_root/rollcall/src-tauri/target/release/bundle/macos/Rollcall.app}"
quire_app="${YOFILE_QUIRE_APP:-$apps_root/quire/src-tauri/target/release/bundle/macos/Quire.app}"

for pair in "Kiln:$kiln_app" "Rollcall:$rollcall_app" "Quire:$quire_app"; do
  path="${pair#*:}"
  if [[ ! -d "$path" ]]; then
    echo "${pair%%:*} app not found at $path (build it first, or set the override var)" >&2
    exit 1
  fi
done

if [[ "$signing_identity" != "-" && -z "$notary_profile" ]]; then
  echo "YOFILE_NOTARY_PROFILE is required for a signed public release" >&2
  exit 1
fi

cd "$project_root"
npx tauri build --bundles app --no-sign

rm -rf "$resources_apps"
mkdir -p "$resources_apps" "$dmg_dir"
ditto "$kiln_app" "$resources_apps/Kiln.app"
ditto "$rollcall_app" "$resources_apps/Rollcall.app"
ditto "$quire_app" "$resources_apps/Quire.app"

if [[ "$signing_identity" == "-" ]]; then
  codesign --force --deep --timestamp=none --sign - "$app_path"
else
  # Nested tool apps are already Developer ID signed; sign YoFile's own code inside-out.
  codesign --force --options runtime --timestamp --sign "$signing_identity" "$app_path/Contents/MacOS/yofile"
  codesign --force --options runtime --timestamp --sign "$signing_identity" "$app_path"
fi
codesign --verify --deep --strict --verbose=2 "$app_path"

staging_dir="$(mktemp -d "${TMPDIR:-/tmp}/yofile-dmg.XXXXXX")"
trap 'rm -rf "$staging_dir"' EXIT
ditto "$app_path" "$staging_dir/YoFile.app"
ln -s /Applications "$staging_dir/Applications"
rm -f "$dmg_path"
hdiutil create -volname YoFile -srcfolder "$staging_dir" -ov -format UDZO "$dmg_path"
hdiutil verify "$dmg_path"

if [[ "$signing_identity" != "-" ]]; then
  codesign --force --timestamp --sign "$signing_identity" "$dmg_path"
  xcrun notarytool submit "$dmg_path" --keychain-profile "$notary_profile" --wait
  xcrun stapler staple "$dmg_path"
  xcrun stapler validate "$dmg_path"
fi

echo "$app_path"
echo "$dmg_path"
