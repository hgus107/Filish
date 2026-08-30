#!/bin/bash
set -euo pipefail

project_root="$(cd "$(dirname "$0")/.." && pwd)"
app_path="$project_root/src-tauri/target/release/bundle/macos/Quire.app"
binary_path="$app_path/Contents/MacOS/quire"
frameworks_dir="$app_path/Contents/Frameworks"
resources_dir="$app_path/Contents/Resources"
tools_dir="$resources_dir/tools"
bin_dir="$resources_dir/bin"
tessdata_dir="$resources_dir/tessdata"
libreoffice_dir="$resources_dir/libreoffice"
dmg_dir="$project_root/src-tauri/target/release/bundle/dmg"
architecture="$(uname -m)"
app_version="$(awk -F'"' '/"version"/ { print $4; exit }' "$project_root/src-tauri/tauri.conf.json")"
dmg_path="$dmg_dir/Quire_${app_version}_${architecture}.dmg"
signing_identity="${QUIRE_SIGNING_IDENTITY:--}"
notary_profile="${QUIRE_NOTARY_PROFILE:-}"

if [[ "$signing_identity" != "-" && -z "$notary_profile" ]]; then
  echo "QUIRE_NOTARY_PROFILE is required for a signed public release" >&2
  exit 1
fi

for tool in qpdf tesseract pandoc; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "$tool is required to package Quire" >&2
    exit 1
  fi
done

libreoffice_app="${QUIRE_LIBREOFFICE_APP:-}"
if [[ -z "$libreoffice_app" && -d /Applications/LibreOffice.app ]]; then
  libreoffice_app=/Applications/LibreOffice.app
fi
if [[ -z "$libreoffice_app" ]]; then
  runtime_libreoffice="$HOME/.cache/codex-runtimes/codex-primary-runtime/dependencies/native/libreoffice-headless/libreoffice/LibreOfficeDev.app"
  if [[ -d "$runtime_libreoffice" ]]; then
    libreoffice_app="$runtime_libreoffice"
  fi
fi
if [[ ! -d "$libreoffice_app" ]]; then
  echo "LibreOffice.app was not found; set QUIRE_LIBREOFFICE_APP" >&2
  exit 1
fi

tessdata_source="${QUIRE_TESSDATA_DIR:-}"
if [[ -z "$tessdata_source" ]]; then
  tessdata_source="$(brew --prefix tesseract)/share/tessdata"
fi
if [[ ! -f "$tessdata_source/eng.traineddata" ]]; then
  echo "English Tesseract data was not found; set QUIRE_TESSDATA_DIR" >&2
  exit 1
fi

cd "$project_root"
if [[ "${QUIRE_BUNDLE_ONLY:-0}" == "1" ]]; then
  npx tauri bundle --bundles app --no-sign
else
  npx tauri build --bundles app --no-sign
fi

if [[ ! -d "$app_path" ]]; then
  echo "Quire.app was not created at the expected path" >&2
  exit 1
fi

rm -rf "$frameworks_dir" "$tools_dir" "$bin_dir" "$tessdata_dir" "$libreoffice_dir"
mkdir -p "$frameworks_dir" "$tools_dir" "$bin_dir" "$tessdata_dir" "$libreoffice_dir" "$dmg_dir"

tool_sources=("$(command -v qpdf)" "$(command -v tesseract)" "$(command -v pandoc)")
for source in "${tool_sources[@]}"; do
  resolved="$(realpath "$source")"
  cp -L "$resolved" "$tools_dir/$(basename "$source")"
  chmod u+w "$tools_dir/$(basename "$source")"
done

ditto "$libreoffice_app" "$libreoffice_dir/$(basename "$libreoffice_app")"
ditto "$tessdata_source" "$tessdata_dir"

cat > "$bin_dir/qpdf" <<'WRAPPER'
#!/bin/bash
resources="$(cd "$(dirname "$0")/.." && pwd)"
exec "$resources/tools/qpdf" "$@"
WRAPPER
cat > "$bin_dir/pandoc" <<'WRAPPER'
#!/bin/bash
resources="$(cd "$(dirname "$0")/.." && pwd)"
exec "$resources/tools/pandoc" "$@"
WRAPPER
cat > "$bin_dir/tesseract" <<'WRAPPER'
#!/bin/bash
resources="$(cd "$(dirname "$0")/.." && pwd)"
export TESSDATA_PREFIX="$resources/tessdata"
exec "$resources/tools/tesseract" "$@"
WRAPPER
libreoffice_name="$(basename "$libreoffice_app")"
cat > "$bin_dir/soffice" <<WRAPPER
#!/bin/bash
set -euo pipefail
resources="\$(cd "\$(dirname "\$0")/.." && pwd)"
profile="\$(mktemp -d "\${TMPDIR:-/tmp}/quire-soffice.XXXXXXXX")"
trap 'rm -rf -- "\${profile:?}"' EXIT
"\$resources/libreoffice/$libreoffice_name/Contents/MacOS/soffice" "-env:UserInstallation=file://\$profile" "\$@"
WRAPPER
chmod +x "$bin_dir/qpdf" "$bin_dir/pandoc" "$bin_dir/tesseract" "$bin_dir/soffice"

resolve_dependency() {
  local dependency="$1"
  local current="$2"
  local name
  name="$(basename "$dependency")"
  case "$dependency" in
    /System/*|/usr/lib/*) return 1 ;;
    /*) [[ -e "$dependency" ]] && { realpath "$dependency"; return 0; } ;;
    @loader_path/*)
      local candidate="$(dirname "$current")/${dependency#@loader_path/}"
      [[ -e "$candidate" ]] && { realpath "$candidate"; return 0; }
      ;;
    @executable_path/*)
      local candidate="$(dirname "$current")/${dependency#@executable_path/}"
      [[ -e "$candidate" ]] && { realpath "$candidate"; return 0; }
      ;;
    @rpath/*)
      for candidate in \
        "$(dirname "$current")/$name" \
        "$(dirname "$current")/../lib/$name" \
        "/opt/homebrew/lib/$name" \
        /opt/homebrew/opt/*/lib/"$name"; do
        [[ -e "$candidate" ]] && { realpath "$candidate"; return 0; }
      done
      ;;
  esac
  return 1
}

queue=("$tools_dir/qpdf" "$tools_dir/tesseract" "$tools_dir/pandoc")
index=0
while [[ $index -lt ${#queue[@]} ]]; do
  current="${queue[$index]}"
  index=$((index + 1))
  while IFS= read -r dependency; do
    resolved="$(resolve_dependency "$dependency" "$current" || true)"
    [[ -n "$resolved" ]] || continue
    name="$(basename "$resolved")"
    destination="$frameworks_dir/$name"
    if [[ ! -e "$destination" ]]; then
      cp -L "$resolved" "$destination"
      chmod u+w "$destination"
      queue+=("$destination")
    elif ! cmp -s "$resolved" "$destination"; then
      echo "Two different libraries share the filename $name" >&2
      exit 1
    fi
  done < <(otool -L "$current" | tail -n +2 | awk '{print $1}')
done

targets=("$tools_dir/qpdf" "$tools_dir/tesseract" "$tools_dir/pandoc")
while IFS= read -r -d '' library; do
  targets+=("$library")
done < <(find "$frameworks_dir" -type f -name '*.dylib' -print0)

for target in "${targets[@]}"; do
  while IFS= read -r dependency; do
    case "$dependency" in
      /System/*|/usr/lib/*) continue ;;
      *)
        resolved="$(resolve_dependency "$dependency" "$target" || true)"
        [[ -n "$resolved" ]] || continue
        install_name_tool -change "$dependency" "@rpath/$(basename "$resolved")" "$target"
        ;;
    esac
  done < <(otool -L "$target" | tail -n +2 | awk '{print $1}')
done

for library in "${targets[@]:3}"; do
  install_name_tool -id "@rpath/$(basename "$library")" "$library"
done
for tool in "$tools_dir/qpdf" "$tools_dir/tesseract" "$tools_dir/pandoc"; do
  if ! otool -l "$tool" | grep -A2 LC_RPATH | grep -Fq '@executable_path/../../Frameworks'; then
    install_name_tool -add_rpath '@executable_path/../../Frameworks' "$tool"
  fi
done

# Rewritten Mach-O files must have a valid temporary signature before macOS
# will execute them for the bundled-tool smoke tests below.
for target in "${targets[@]}"; do
  codesign --force --timestamp=none --sign - "$target"
done

for tool in qpdf tesseract pandoc soffice; do
  "$bin_dir/$tool" --version >/dev/null
done

if [[ "$signing_identity" == "-" ]]; then
  codesign --force --deep --timestamp=none --sign - "$libreoffice_dir/$libreoffice_name"
  for target in "${targets[@]}"; do
    codesign --force --timestamp=none --sign - "$target"
  done
  codesign --force --deep --timestamp=none --sign - "$app_path"
else
  codesign --force --deep --options runtime --timestamp --sign "$signing_identity" "$libreoffice_dir/$libreoffice_name"
  for target in "${targets[@]}"; do
    codesign --force --options runtime --timestamp --sign "$signing_identity" "$target"
  done
  codesign --force --deep --options runtime --timestamp --sign "$signing_identity" "$app_path"
fi
codesign --verify --deep --strict --verbose=2 "$app_path"

if [[ "${QUIRE_APP_ONLY:-0}" == "1" ]]; then
  echo "$app_path"
  exit 0
fi

staging_dir="$(mktemp -d "${TMPDIR:-/tmp}/quire-dmg.XXXXXX")"
trap 'rm -rf "$staging_dir"' EXIT
ditto "$app_path" "$staging_dir/Quire.app"
ln -s /Applications "$staging_dir/Applications"
rm -f "$dmg_path"
hdiutil create -volname Quire -srcfolder "$staging_dir" -ov -format UDZO "$dmg_path"
hdiutil verify "$dmg_path"

if [[ "$signing_identity" != "-" ]]; then
  codesign --force --timestamp --sign "$signing_identity" "$dmg_path"
  xcrun notarytool submit "$dmg_path" --keychain-profile "$notary_profile" --wait
  xcrun stapler staple "$dmg_path"
  xcrun stapler validate "$dmg_path"
fi

echo "$app_path"
echo "$dmg_path"
