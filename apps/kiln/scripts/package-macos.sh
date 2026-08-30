#!/bin/bash
set -euo pipefail

project_root="$(cd "$(dirname "$0")/.." && pwd)"
app_path="$project_root/src-tauri/target/release/bundle/macos/Kiln.app"
binary_path="$app_path/Contents/MacOS/kiln"
frameworks_dir="$app_path/Contents/Frameworks"
contents_lib_dir="$app_path/Contents/lib"
dmg_dir="$project_root/src-tauri/target/release/bundle/dmg"
architecture="$(uname -m)"
app_version="$(awk -F'"' '/"version"/ { print $4; exit }' "$project_root/src-tauri/tauri.conf.json")"
dmg_path="$dmg_dir/Kiln_${app_version}_${architecture}.dmg"
signing_identity="${KILN_SIGNING_IDENTITY:--}"
notary_profile="${KILN_NOTARY_PROFILE:-}"

if [[ "$signing_identity" != "-" && -z "$notary_profile" ]]; then
  echo "KILN_NOTARY_PROFILE is required for a signed release" >&2
  exit 1
fi

cd "$project_root"
if [[ "${KILN_BUNDLE_ONLY:-0}" == "1" ]]; then
  npx tauri bundle --bundles app --no-sign
else
  npx tauri build --bundles app --no-sign
fi

if [[ "$frameworks_dir" != "$app_path/Contents/Frameworks" ]]; then
  echo "Refusing to replace an unexpected Frameworks directory" >&2
  exit 1
fi
rm -rf "$frameworks_dir"
rm -rf "$contents_lib_dir"
mkdir -p "$frameworks_dir" "$contents_lib_dir" "$dmg_dir"

queue=("$binary_path")
vips_library="$(otool -L "$binary_path" | awk 'NR > 1 && $1 ~ /libvips\.[0-9]+\.dylib$/ { print $1; exit }')"
vips_module="$(find "$(dirname "$vips_library")" -maxdepth 2 -type f -name 'vips-heif.dylib' -print -quit)"
if [[ -z "$vips_module" ]]; then
  echo "The libvips HEIC/AVIF module could not be found" >&2
  exit 1
fi
module_dir="$contents_lib_dir/$(basename "$(dirname "$vips_module")")"
mkdir -p "$module_dir"
cp -L "$vips_module" "$module_dir/vips-heif.dylib"
chmod u+w "$module_dir/vips-heif.dylib"
queue+=("$vips_module")

index=0
while [[ $index -lt ${#queue[@]} ]]; do
  current="${queue[$index]}"
  index=$((index + 1))
  while IFS= read -r dependency; do
    case "$dependency" in
      /System/*|/usr/lib/*) continue ;;
      /*) ;;
      @rpath/*)
        name="$(basename "$dependency")"
        candidate="$(dirname "$current")/$name"
        if [[ ! -e "$candidate" ]]; then
          candidate="$(dirname "$current")/../lib/$name"
        fi
        [[ -e "$candidate" ]] || continue
        dependency="$candidate"
        ;;
      @loader_path/*)
        dependency="$(dirname "$current")/${dependency#@loader_path/}"
        [[ -e "$dependency" ]] || continue
        ;;
      *) continue ;;
    esac
    name="$(basename "$dependency")"
    destination="$frameworks_dir/$name"
    if [[ ! -e "$destination" ]]; then
      cp -L "$dependency" "$destination"
      chmod u+w "$destination"
      queue+=("$dependency")
    elif ! cmp -s "$dependency" "$destination"; then
      echo "Two different libraries share the filename $name" >&2
      exit 1
    fi
  done < <(otool -L "$current" | tail -n +2 | awk '{print $1}')
done

targets=("$binary_path")
while IFS= read -r -d '' library; do
  targets+=("$library")
done < <(find "$frameworks_dir" -type f -name '*.dylib' -print0)
targets+=("$module_dir/vips-heif.dylib")

for target in "${targets[@]}"; do
  while IFS= read -r dependency; do
    case "$dependency" in
      /System/*|/usr/lib/*) continue ;;
      /*) install_name_tool -change "$dependency" "@rpath/$(basename "$dependency")" "$target" ;;
    esac
  done < <(otool -L "$target" | tail -n +2 | awk '{print $1}')
done

for library in "${targets[@]:1}"; do
  install_name_tool -id "@rpath/$(basename "$library")" "$library"
done
if ! otool -l "$binary_path" | grep -A2 LC_RPATH | grep -Fq '@executable_path/../Frameworks'; then
  install_name_tool -add_rpath '@executable_path/../Frameworks' "$binary_path"
fi

if [[ "$signing_identity" == "-" ]]; then
  for library in "${targets[@]:1}"; do
    codesign --force --timestamp=none --sign - "$library"
  done
  codesign --force --deep --timestamp=none --sign - "$app_path"
else
  for library in "${targets[@]:1}"; do
    codesign --force --options runtime --timestamp --sign "$signing_identity" "$library"
  done
  codesign --force --deep --options runtime --timestamp --sign "$signing_identity" "$app_path"
fi
codesign --verify --deep --strict --verbose=2 "$app_path"

if [[ "${KILN_APP_ONLY:-0}" == "1" ]]; then
  echo "$app_path"
  exit 0
fi

staging_dir="$(mktemp -d "${TMPDIR:-/tmp}/kiln-dmg.XXXXXX")"
trap 'rm -rf "$staging_dir"' EXIT
ditto "$app_path" "$staging_dir/Kiln.app"
ln -s /Applications "$staging_dir/Applications"
rm -f "$dmg_path"
hdiutil create -volname Kiln -srcfolder "$staging_dir" -ov -format UDZO "$dmg_path"
hdiutil verify "$dmg_path"

if [[ "$signing_identity" != "-" ]]; then
  codesign --force --timestamp --sign "$signing_identity" "$dmg_path"
  xcrun notarytool submit "$dmg_path" --keychain-profile "$notary_profile" --wait
  xcrun stapler staple "$dmg_path"
  xcrun stapler validate "$dmg_path"
fi

echo "$app_path"
echo "$dmg_path"
