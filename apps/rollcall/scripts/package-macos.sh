#!/bin/bash
set -euo pipefail

project_root="$(cd "$(dirname "$0")/.." && pwd)"
app_path="$project_root/src-tauri/target/release/bundle/macos/Rollcall.app"
dmg_dir="$project_root/src-tauri/target/release/bundle/dmg"
architecture="$(uname -m)"
app_version="$(awk -F'"' '/"version"/ { print $4; exit }' "$project_root/src-tauri/tauri.conf.json")"
dmg_path="$dmg_dir/Rollcall_${app_version}_${architecture}.dmg"
signing_identity="${ROLLCALL_SIGNING_IDENTITY:--}"
notary_profile="${ROLLCALL_NOTARY_PROFILE:-}"

if [[ "$signing_identity" != "-" && -z "$notary_profile" ]]; then
  echo "ROLLCALL_NOTARY_PROFILE is required for a signed public release" >&2
  exit 1
fi

cd "$project_root"
if [[ "${ROLLCALL_BUNDLE_ONLY:-0}" == "1" ]]; then
  npx tauri bundle --bundles app --no-sign
else
  npx tauri build --bundles app --no-sign
fi

if [[ ! -d "$app_path" ]]; then
  echo "Rollcall.app was not created at the expected path" >&2
  exit 1
fi
mkdir -p "$dmg_dir"

if [[ "$signing_identity" == "-" ]]; then
  codesign --force --timestamp=none --sign - "$app_path"
else
  codesign --force --options runtime --timestamp --sign "$signing_identity" "$app_path"
fi
codesign --verify --deep --strict --verbose=2 "$app_path"

if [[ "${ROLLCALL_APP_ONLY:-0}" == "1" ]]; then
  echo "$app_path"
  exit 0
fi

staging_dir="$(mktemp -d "${TMPDIR:-/tmp}/rollcall-dmg.XXXXXX")"
trap 'rm -rf "$staging_dir"' EXIT
ditto "$app_path" "$staging_dir/Rollcall.app"
ln -s /Applications "$staging_dir/Applications"
rm -f "$dmg_path"
hdiutil create -volname Rollcall -srcfolder "$staging_dir" -ov -format UDZO "$dmg_path"
hdiutil verify "$dmg_path"

if [[ "$signing_identity" != "-" ]]; then
  codesign --force --timestamp --sign "$signing_identity" "$dmg_path"
  xcrun notarytool submit "$dmg_path" --keychain-profile "$notary_profile" --wait
  xcrun stapler staple "$dmg_path"
  xcrun stapler validate "$dmg_path"
fi

echo "$app_path"
echo "$dmg_path"
