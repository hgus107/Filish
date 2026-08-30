use chrono::{DateTime, Local, NaiveDate};
use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use tauri::menu::{MenuBuilder, SubmenuBuilder};
use unicode_normalization::UnicodeNormalization;
use walkdir::WalkDir;

#[cfg(target_os = "macos")]
fn is_command_a(key_code: u16, modifier_flags: usize) -> bool {
    const COMMAND_FLAG: usize = 1 << 20;
    key_code == 0 && modifier_flags & COMMAND_FLAG != 0
}

#[cfg(target_os = "macos")]
fn install_open_panel_select_all() {
    use block2::RcBlock;
    use objc2::runtime::NSObjectProtocol;
    use objc2::{sel, ClassType, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSEvent, NSEventMask, NSOpenPanel};
    use std::ptr::NonNull;

    let handler = RcBlock::new(|event: NonNull<NSEvent>| -> *mut NSEvent {
        let event_ref = unsafe { event.as_ref() };
        let command_a = is_command_a(event_ref.keyCode(), event_ref.modifierFlags().bits());
        if command_a {
            if let Some(marker) = MainThreadMarker::new() {
                let app = NSApplication::sharedApplication(marker);
                let open_panel_active = app
                    .keyWindow()
                    .is_some_and(|window| window.isKindOfClass(NSOpenPanel::class()));
                if open_panel_active
                    && unsafe { app.sendAction_to_from(sel!(selectAll:), None, None) }
                {
                    return std::ptr::null_mut();
                }
            }
        }
        event.as_ptr()
    });

    let monitor = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &handler)
    };
    if let Some(monitor) = monitor {
        std::mem::forget(monitor);
    }
}

const MAX_FILES: usize = 20_000;
const MAX_FOLDER_DEPTH: usize = 8;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileEntry {
    path: String,
    name: String,
    bytes: u64,
    modified_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CollectedFiles {
    files: Vec<FileEntry>,
    skipped_hidden: usize,
    truncated: bool,
    folder_depth_limited: bool,
    unreadable_folders: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenameRules {
    template: String,
    find: String,
    replace: String,
    case_style: String,
    counter_start: u64,
    counter_padding: usize,
    date_source: String,
    date_format: String,
    custom_date: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewItem {
    source: String,
    target: String,
    original_name: String,
    proposed_name: String,
    status: String,
    message: String,
    matched: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RenamePreview {
    items: Vec<PreviewItem>,
    ready: bool,
    changed: usize,
    conflicts: usize,
    invalid: usize,
    unchanged: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchResult {
    count: usize,
    log_path: String,
}

#[cfg(test)]
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn add_file(path: &Path, seen: &mut HashSet<PathBuf>, files: &mut Vec<FileEntry>) {
    if files.len() >= MAX_FILES || !path.is_file() {
        return;
    }

    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !seen.insert(resolved.clone()) {
        return;
    }

    let name = resolved
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let metadata = fs::metadata(&resolved).ok();
    let bytes = metadata.as_ref().map(|value| value.len()).unwrap_or(0);
    let modified_ms = metadata
        .and_then(|value| value.modified().ok())
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0);
    files.push(FileEntry {
        path: path_string(&resolved),
        name,
        bytes,
        modified_ms,
    });
}

fn collect_files_impl(paths: Vec<String>) -> CollectedFiles {
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    let mut skipped_hidden = 0;
    let mut truncated = false;
    let mut folder_depth_limited = false;
    let mut unreadable_folders = 0;

    for raw_path in paths {
        if files.len() >= MAX_FILES {
            truncated = true;
            break;
        }

        let root = PathBuf::from(raw_path);
        if root.is_file() {
            add_file(&root, &mut seen, &mut files);
            continue;
        }
        if !root.is_dir() {
            continue;
        }

        let mut walker = WalkDir::new(&root)
            .follow_links(false)
            .max_depth(MAX_FOLDER_DEPTH)
            .sort_by_file_name()
            .into_iter();

        while let Some(next) = walker.next() {
            if files.len() >= MAX_FILES {
                truncated = true;
                break;
            }

            let entry = match next {
                Ok(entry) => entry,
                Err(_) => {
                    unreadable_folders += 1;
                    continue;
                }
            };
            if entry.depth() == 0 {
                continue;
            }

            let hidden = entry
                .file_name()
                .to_str()
                .map(|name| name.starts_with('.'))
                .unwrap_or(false);
            if hidden {
                skipped_hidden += 1;
                if entry.file_type().is_dir() {
                    walker.skip_current_dir();
                }
                continue;
            }

            if entry.depth() == MAX_FOLDER_DEPTH && entry.file_type().is_dir() {
                folder_depth_limited = true;
            }
            if entry.file_type().is_file() {
                add_file(entry.path(), &mut seen, &mut files);
            }
        }
    }

    files.sort_by(|left, right| {
        left.path
            .to_lowercase()
            .cmp(&right.path.to_lowercase())
            .then_with(|| left.path.cmp(&right.path))
    });
    CollectedFiles {
        files,
        skipped_hidden,
        truncated,
        folder_depth_limited,
        unreadable_folders,
    }
}

#[tauri::command]
async fn collect_files(paths: Vec<String>) -> Result<CollectedFiles, String> {
    tauri::async_runtime::spawn_blocking(move || collect_files_impl(paths))
        .await
        .map_err(|error| error.to_string())
}

fn normalized_name(value: &str) -> String {
    value.nfc().collect::<String>().to_lowercase()
}

fn apply_case(value: &str, case_style: &str) -> Result<String, String> {
    match case_style {
        "keep" => Ok(value.to_string()),
        "lower" => Ok(value.to_lowercase()),
        "upper" => Ok(value.to_uppercase()),
        "title" => {
            let mut output = String::with_capacity(value.len());
            let mut start_word = true;
            for character in value.chars() {
                if character == ' ' || character == '-' || character == '_' {
                    output.push(character);
                    start_word = true;
                } else if start_word {
                    output.extend(character.to_uppercase());
                    start_word = false;
                } else {
                    output.extend(character.to_lowercase());
                }
            }
            Ok(output)
        }
        _ => Err("Choose a supported letter-case option.".to_string()),
    }
}

fn validate_rules(rules: &RenameRules) -> Result<(), String> {
    if rules.template.trim().is_empty() {
        return Err("Name pattern cannot be empty.".to_string());
    }
    if rules.counter_padding == 0 || rules.counter_padding > 3 {
        return Err("Count digits must be between 1 and 3.".to_string());
    }
    apply_case("test", &rules.case_style)?;

    let token = Regex::new(r"\{[^{}]+\}").expect("static token regex is valid");
    for found in token.find_iter(&rules.template) {
        if !matches!(found.as_str(), "{name}" | "{n}" | "{ext}" | "{date}") {
            return Err(format!("Unknown pattern token: {}", found.as_str()));
        }
    }

    if !matches!(rules.date_source.as_str(), "today" | "modified" | "custom") {
        return Err("Choose a supported date source.".to_string());
    }
    if rules.date_source == "custom"
        && NaiveDate::parse_from_str(&rules.custom_date, "%Y-%m-%d").is_err()
    {
        return Err("Choose a valid custom date.".to_string());
    }
    if !matches!(
        rules.date_format.as_str(),
        "yyyy-mm-dd" | "mm-dd-yyyy" | "dd-mm-yyyy"
    ) {
        return Err("Choose a supported date format.".to_string());
    }

    Ok(())
}

fn validate_filename(name: &str) -> Result<(), String> {
    if name.is_empty() || name == "." || name == ".." {
        return Err("A filename cannot be empty, '.' or '..'.".to_string());
    }
    if name.len() > 255 {
        return Err("The new filename is longer than 255 bytes.".to_string());
    }
    if name.chars().any(|character| character.is_control()) {
        return Err("The new filename contains a control character.".to_string());
    }
    if name.chars().any(|character| {
        matches!(
            character,
            '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*'
        )
    }) {
        return Err(
            "The new filename contains a character unsupported on Windows or macOS.".to_string(),
        );
    }
    if name.ends_with(' ') || name.ends_with('.') {
        return Err("A filename cannot end with a space or period on Windows.".to_string());
    }

    let stem = name.split('.').next().unwrap_or_default().to_uppercase();
    let reserved = matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    );
    if reserved {
        return Err("That filename is reserved by Windows.".to_string());
    }
    Ok(())
}

fn planned_name(path: &Path, index: usize, rules: &RenameRules) -> Result<String, String> {
    let original_stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().into_owned())
        .unwrap_or_default();

    let replaced = if rules.find.is_empty() {
        original_stem
    } else {
        original_stem.replace(&rules.find, &rules.replace)
    };
    let transformed = apply_case(&replaced, &rules.case_style)?;
    let counter = rules.counter_start.saturating_add(index as u64);
    let counter = format!("{counter:0width$}", width = rules.counter_padding);
    let date_value = if rules.date_source == "custom" {
        NaiveDate::parse_from_str(&rules.custom_date, "%Y-%m-%d")
            .map_err(|_| "Choose a valid custom date.".to_string())?
    } else if rules.date_source == "modified" {
        fs::metadata(path)
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .map(DateTime::<Local>::from)
            .unwrap_or_else(Local::now)
            .date_naive()
    } else {
        Local::now().date_naive()
    };
    let date = match rules.date_format.as_str() {
        "mm-dd-yyyy" => date_value.format("%m-%d-%Y").to_string(),
        "dd-mm-yyyy" => date_value.format("%d-%m-%Y").to_string(),
        _ => date_value.format("%Y-%m-%d").to_string(),
    };
    let token = Regex::new(r"\{(name|n|ext|date)\}").expect("static pattern regex is valid");
    let mut output = token
        .replace_all(&rules.template, |captures: &Captures<'_>| {
            match &captures[1] {
                "name" => transformed.as_str(),
                "n" => counter.as_str(),
                "ext" => extension.as_str(),
                "date" => date.as_str(),
                _ => "",
            }
        })
        .into_owned();

    if !rules.template.contains("{ext}") && !extension.is_empty() {
        output.push('.');
        output.push_str(&extension);
    }
    Ok(output)
}

fn preview_save_as_impl(paths: Vec<String>, rules: &RenameRules) -> Result<RenamePreview, String> {
    validate_rules(rules)?;
    let mut items = Vec::with_capacity(paths.len());

    for (index, source) in paths.iter().enumerate() {
        let source_path = PathBuf::from(source);
        let original_name = source_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let proposed_name = planned_name(&source_path, index, rules)?;
        let original_stem = source_path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let matched = if rules.find.is_empty() {
            true
        } else {
            original_stem.contains(&rules.find)
        };
        let target_path = source_path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(&proposed_name);
        let target = path_string(&target_path);

        let (status, message) = if !source_path.is_file() {
            (
                "missing".to_string(),
                "The source file no longer exists.".to_string(),
            )
        } else if let Err(error) = validate_filename(&proposed_name) {
            ("invalid".to_string(), error)
        } else if source_path == target_path {
            (
                "unchanged".to_string(),
                "This name is unchanged.".to_string(),
            )
        } else {
            ("ready".to_string(), "Ready to save.".to_string())
        };

        items.push(PreviewItem {
            source: source.clone(),
            target,
            original_name,
            proposed_name,
            status,
            message,
            matched,
        });
    }

    let mut target_counts: HashMap<String, usize> = HashMap::new();
    for item in &items {
        if item.status == "ready" {
            let key = normalized_name(&item.proposed_name);
            *target_counts.entry(key).or_default() += 1;
        }
    }

    for item in &mut items {
        if item.status != "ready" {
            continue;
        }
        let key = normalized_name(&item.proposed_name);
        if target_counts.get(&key).copied().unwrap_or(0) > 1 {
            item.status = "conflict".to_string();
            item.message = "More than one file would receive this name.".to_string();
            continue;
        }
    }

    let changed = items.iter().filter(|item| item.status == "ready").count();
    let conflicts = items
        .iter()
        .filter(|item| item.status == "conflict")
        .count();
    let invalid = items
        .iter()
        .filter(|item| item.status == "invalid" || item.status == "missing")
        .count();
    let unchanged = items
        .iter()
        .filter(|item| item.status == "unchanged")
        .count();
    Ok(RenamePreview {
        ready: changed > 0 && conflicts == 0 && invalid == 0,
        items,
        changed,
        conflicts,
        invalid,
        unchanged,
    })
}

#[tauri::command]
async fn preview_save_as(paths: Vec<String>, rules: RenameRules) -> Result<RenamePreview, String> {
    tauri::async_runtime::spawn_blocking(move || preview_save_as_impl(paths, &rules))
        .await
        .map_err(|error| error.to_string())?
}

fn save_as_impl(
    paths: Vec<String>,
    rules: RenameRules,
    destination: String,
) -> Result<BatchResult, String> {
    let destination = if destination == "~" {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(destination))
    } else if let Some(relative) = destination.strip_prefix("~/") {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join(relative))
            .unwrap_or_else(|| PathBuf::from(destination))
    } else {
        PathBuf::from(destination)
    };
    if !destination.is_absolute() {
        return Err("Type An Absolute Destination Folder Path.".to_string());
    }
    if !destination.is_dir() {
        return Err("Choose A Valid Destination Folder.".to_string());
    }
    let destination = destination
        .canonicalize()
        .map_err(|error| format!("Could Not Open The Destination Folder: {error}"))?;

    validate_rules(&rules)?;
    let mut planned: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut output_names = HashSet::new();
    let existing_output_names: HashSet<String> = fs::read_dir(&destination)
        .map_err(|error| format!("Could Not Read The Destination Folder: {error}"))?
        .flatten()
        .map(|entry| normalized_name(&entry.file_name().to_string_lossy()))
        .collect();

    for (index, source) in paths.iter().enumerate() {
        let source = PathBuf::from(source);
        if !source.is_file() {
            return Err(format!("Source File Is Missing: {}", path_string(&source)));
        }
        let proposed_name = planned_name(&source, index, &rules)?;
        validate_filename(&proposed_name)?;
        let original_name = source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if proposed_name == original_name {
            continue;
        }

        let output_key = normalized_name(&proposed_name);
        if !output_names.insert(output_key.clone()) {
            return Err(format!(
                "More Than One File Would Be Saved As '{proposed_name}'."
            ));
        }
        let target = destination.join(&proposed_name);
        if existing_output_names.contains(&output_key) || target.exists() {
            return Err(format!(
                "A File Named '{proposed_name}' Already Exists In That Folder."
            ));
        }
        planned.push((source, target));
    }

    if planned.is_empty() {
        return Err("No Filenames Would Change.".to_string());
    }

    let mut created: Vec<PathBuf> = Vec::new();
    for (source, target) in &planned {
        let copy_result = (|| -> Result<(), io::Error> {
            let mut input = fs::File::open(source)?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(target)?;
            created.push(target.clone());
            io::copy(&mut input, &mut output)?;
            output.set_permissions(fs::metadata(source)?.permissions())?;
            output.sync_all()
        })();
        if let Err(error) = copy_result {
            for path in created.iter().rev() {
                let _ = fs::remove_file(path);
            }
            return Err(format!(
                "Could Not Save '{}': {error}. No Partial Copies Were Kept.",
                path_string(target)
            ));
        }
    }

    Ok(BatchResult {
        count: created.len(),
        log_path: path_string(&destination),
    })
}

#[tauri::command]
async fn save_as(
    paths: Vec<String>,
    rules: RenameRules,
    destination: String,
) -> Result<BatchResult, String> {
    tauri::async_runtime::spawn_blocking(move || save_as_impl(paths, rules, destination))
        .await
        .map_err(|error| error.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
            #[cfg(target_os = "macos")]
            install_open_panel_select_all();
            Ok(())
        })
        .menu(|app| {
            let rollcall_menu = SubmenuBuilder::new(app, "Rollcall")
                .about_with_text("About Rollcall", None)
                .separator()
                .quit_with_text("Quit Rollcall")
                .build()?;
            let view_menu = SubmenuBuilder::new(app, "View")
                .fullscreen_with_text("Toggle Full Screen")
                .build()?;

            MenuBuilder::new(app)
                .items(&[&rollcall_menu, &view_menu])
                .build()
        })
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            collect_files,
            preview_save_as,
            save_as
        ])
        .run(tauri::generate_context!())
        .expect("error while running Rollcall");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_directory(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "rollcall-{label}-{}-{}",
            std::process::id(),
            now_millis()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn default_rules() -> RenameRules {
        RenameRules {
            template: "{name}-{n}".to_string(),
            find: String::new(),
            replace: String::new(),
            case_style: "keep".to_string(),
            counter_start: 1,
            counter_padding: 3,
            date_source: "today".to_string(),
            date_format: "yyyy-mm-dd".to_string(),
            custom_date: String::new(),
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn command_a_detection_is_exact() {
        assert!(is_command_a(0, 1 << 20));
        assert!(is_command_a(0, (1 << 20) | (1 << 17)));
        assert!(!is_command_a(1, 1 << 20));
        assert!(!is_command_a(0, 0));
    }

    #[test]
    fn preview_uses_counter_and_preserves_extension() {
        let base = test_directory("preview");
        let first = base.join("IMG_1000.JPG");
        let second = base.join("IMG_1001.JPG");
        fs::write(&first, b"one").unwrap();
        fs::write(&second, b"two").unwrap();

        let preview = preview_save_as_impl(
            vec![path_string(&first), path_string(&second)],
            &default_rules(),
        )
        .unwrap();
        assert!(preview.ready);
        assert_eq!(preview.items[0].proposed_name, "IMG_1000-001.JPG");
        assert_eq!(preview.items[1].proposed_name, "IMG_1001-002.JPG");
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn preview_adds_a_formatted_date() {
        let base = test_directory("date");
        let source = base.join("report.pdf");
        fs::write(&source, b"report").unwrap();
        let mut rules = default_rules();
        rules.template = "{date}-{name}".to_string();
        rules.date_source = "today".to_string();
        rules.date_format = "yyyy-mm-dd".to_string();
        let preview = preview_save_as_impl(vec![path_string(&source)], &rules).unwrap();
        assert!(Regex::new(r"^\d{4}-\d{2}-\d{2}-report\.pdf$")
            .unwrap()
            .is_match(&preview.items[0].proposed_name));
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn preview_adds_a_custom_date() {
        let base = test_directory("custom-date");
        let source = base.join("report.pdf");
        fs::write(&source, b"report").unwrap();
        let mut rules = default_rules();
        rules.template = "{name}-{date}".to_string();
        rules.date_source = "custom".to_string();
        rules.custom_date = "2026-12-31".to_string();
        rules.date_format = "dd-mm-yyyy".to_string();
        let preview = preview_save_as_impl(vec![path_string(&source)], &rules).unwrap();
        assert_eq!(preview.items[0].proposed_name, "report-31-12-2026.pdf");
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn invalid_custom_date_is_rejected() {
        let mut rules = default_rules();
        rules.date_source = "custom".to_string();
        rules.custom_date = "2026-02-30".to_string();
        assert!(validate_rules(&rules)
            .unwrap_err()
            .contains("valid custom date"));
    }

    #[test]
    fn save_as_creates_renamed_copies_and_preserves_originals() {
        let base = test_directory("save-as");
        let source_folder = base.join("source");
        let destination = base.join("destination");
        fs::create_dir_all(&source_folder).unwrap();
        fs::create_dir_all(&destination).unwrap();
        let source = source_folder.join("report.txt");
        fs::write(&source, b"original").unwrap();

        let mut rules = default_rules();
        rules.template = "Final-{name}".to_string();
        let result =
            save_as_impl(vec![path_string(&source)], rules, path_string(&destination)).unwrap();

        assert_eq!(result.count, 1);
        assert_eq!(fs::read(&source).unwrap(), b"original");
        assert_eq!(
            fs::read(destination.join("Final-report.txt")).unwrap(),
            b"original"
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn save_as_never_overwrites_an_existing_file() {
        let base = test_directory("save-as-collision");
        let destination = base.join("destination");
        fs::create_dir_all(&destination).unwrap();
        let source = base.join("report.txt");
        fs::write(&source, b"source").unwrap();
        fs::write(destination.join("Final-report.txt"), b"existing").unwrap();

        let mut rules = default_rules();
        rules.template = "Final-{name}".to_string();
        let error =
            save_as_impl(vec![path_string(&source)], rules, path_string(&destination)).unwrap_err();

        assert!(error.contains("Already Exists"));
        assert_eq!(
            fs::read(destination.join("Final-report.txt")).unwrap(),
            b"existing"
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn literal_replace_and_case_share_the_preview_path() {
        let base = test_directory("literal-replace");
        let source = base.join("scan_2026_invoice.pdf");
        fs::write(&source, b"fixture").unwrap();
        let rules = RenameRules {
            template: "archive-{name}".to_string(),
            find: "scan_2026_".to_string(),
            replace: String::new(),
            case_style: "upper".to_string(),
            counter_start: 1,
            counter_padding: 2,
            date_source: "today".to_string(),
            date_format: "yyyy-mm-dd".to_string(),
            custom_date: String::new(),
        };
        let preview = preview_save_as_impl(vec![path_string(&source)], &rules).unwrap();
        assert_eq!(preview.items[0].proposed_name, "archive-INVOICE.pdf");

        let mut special_characters = rules.clone();
        special_characters.find = "[".to_string();
        special_characters.replace = "plain".to_string();
        let preview = preview_save_as_impl(vec![path_string(&source)], &special_characters).unwrap();
        assert!(!preview.items[0].matched);
        assert_eq!(preview.items[0].status, "ready");
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn preview_detects_duplicate_output_names() {
        let base = test_directory("conflicts");
        let first = base.join("a.txt");
        let second = base.join("b.txt");
        fs::write(&first, b"a").unwrap();
        fs::write(&second, b"b").unwrap();

        let mut rules = default_rules();
        rules.template = "same".to_string();
        let duplicate =
            preview_save_as_impl(vec![path_string(&first), path_string(&second)], &rules).unwrap();
        assert_eq!(duplicate.conflicts, 2);
        assert!(!duplicate.ready);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn save_as_preview_detects_same_names_across_source_folders() {
        let base = test_directory("save-as-preview");
        let first_folder = base.join("one");
        let second_folder = base.join("two");
        fs::create_dir_all(&first_folder).unwrap();
        fs::create_dir_all(&second_folder).unwrap();
        let first = first_folder.join("report.txt");
        let second = second_folder.join("report.txt");
        fs::write(&first, b"one").unwrap();
        fs::write(&second, b"two").unwrap();

        let mut rules = default_rules();
        rules.template = "Final-{name}".to_string();
        let preview =
            preview_save_as_impl(vec![path_string(&first), path_string(&second)], &rules).unwrap();

        assert_eq!(preview.conflicts, 2);
        assert!(!preview.ready);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn invalid_cross_platform_names_are_rejected() {
        assert!(validate_filename("CON.txt").is_err());
        assert!(validate_filename("bad:name.jpg").is_err());
        assert!(validate_filename("trailing.").is_err());
        assert!(validate_filename("normal name.jpg").is_ok());
    }

    #[test]
    fn collection_skips_hidden_items_inside_a_folder() {
        let base = test_directory("hidden");
        fs::write(base.join("visible.txt"), b"visible").unwrap();
        fs::write(base.join(".hidden.txt"), b"hidden").unwrap();
        let hidden_directory = base.join(".cache");
        fs::create_dir_all(&hidden_directory).unwrap();
        fs::write(hidden_directory.join("inside.txt"), b"inside").unwrap();

        let collected = collect_files_impl(vec![path_string(&base)]);
        assert_eq!(collected.files.len(), 1);
        assert_eq!(collected.files[0].name, "visible.txt");
        assert_eq!(collected.skipped_hidden, 2);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn destination_must_exist_and_be_absolute() {
        let base = test_directory("destination-validation");
        let source = base.join("report.txt");
        fs::write(&source, b"report").unwrap();
        let mut rules = default_rules();
        rules.template = "Final-{name}".to_string();

        let relative = save_as_impl(
            vec![path_string(&source)],
            rules.clone(),
            "relative/output".to_string(),
        )
        .unwrap_err();
        assert!(relative.contains("Absolute Destination"));

        let missing = save_as_impl(
            vec![path_string(&source)],
            rules,
            path_string(&base.join("missing")),
        )
        .unwrap_err();
        assert!(missing.contains("Valid Destination"));
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn missing_sources_and_unchanged_names_are_rejected() {
        let base = test_directory("source-validation");
        let destination = base.join("destination");
        fs::create_dir_all(&destination).unwrap();

        let missing = save_as_impl(
            vec![path_string(&base.join("missing.txt"))],
            default_rules(),
            path_string(&destination),
        )
        .unwrap_err();
        assert!(missing.contains("Source File Is Missing"));

        let source = base.join("same.txt");
        fs::write(&source, b"same").unwrap();
        let mut unchanged_rules = default_rules();
        unchanged_rules.template = "{name}".to_string();
        let unchanged = save_as_impl(
            vec![path_string(&source)],
            unchanged_rules,
            path_string(&destination),
        )
        .unwrap_err();
        assert!(unchanged.contains("No Filenames Would Change"));
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn destination_collisions_are_case_and_unicode_insensitive() {
        let base = test_directory("normalized-collision");
        let destination = base.join("destination");
        fs::create_dir_all(&destination).unwrap();
        let source = base.join("report.txt");
        fs::write(&source, b"source").unwrap();
        fs::write(destination.join("FINAL-CAFÉ.TXT"), b"existing").unwrap();

        let mut rules = default_rules();
        rules.template = "Final-Cafe\u{301}".to_string();
        let error =
            save_as_impl(vec![path_string(&source)], rules, path_string(&destination)).unwrap_err();
        assert!(error.contains("Already Exists"));
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn invalid_rules_fail_before_any_copy_is_created() {
        let base = test_directory("invalid-rules");
        let destination = base.join("destination");
        fs::create_dir_all(&destination).unwrap();
        let source = base.join("report.txt");
        fs::write(&source, b"source").unwrap();

        let mut rules = default_rules();
        rules.counter_padding = 4;
        assert!(preview_save_as_impl(vec![path_string(&source)], &rules).is_err());
        rules.counter_padding = 3;
        rules.template = "{unknown}-{name}".to_string();
        assert!(preview_save_as_impl(vec![path_string(&source)], &rules).is_err());
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn filename_byte_boundary_handles_multibyte_characters() {
        let valid = format!("{}a.txt", "é".repeat(125));
        let invalid = format!("{}aa.txt", "é".repeat(125));
        assert_eq!(valid.len(), 255);
        assert_eq!(invalid.len(), 256);
        assert!(validate_filename(&valid).is_ok());
        assert!(validate_filename(&invalid).is_err());
    }

    #[test]
    fn collection_deduplicates_paths_and_ignores_missing_inputs() {
        let base = test_directory("collection-deduplication");
        let source = base.join("visible.txt");
        fs::write(&source, b"visible").unwrap();
        let collected = collect_files_impl(vec![
            path_string(&source),
            path_string(&source),
            path_string(&base.join("missing.txt")),
        ]);
        assert_eq!(collected.files.len(), 1);
        assert!(!collected.truncated);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn replace_without_a_match_keeps_the_new_name_unchanged() {
        let base = test_directory("no-match");
        let source = base.join("notes.txt");
        fs::write(&source, b"notes").unwrap();
        let mut rules = default_rules();
        rules.template = "{name}".to_string();
        rules.find = "invoice".to_string();
        rules.replace = "receipt".to_string();
        let preview = preview_save_as_impl(vec![path_string(&source)], &rules).unwrap();
        assert!(!preview.items[0].matched);
        assert_eq!(preview.items[0].status, "unchanged");
        assert_eq!(preview.changed, 0);
        assert!(!preview.ready);
        let _ = fs::remove_dir_all(base);
    }

    #[cfg(unix)]
    #[test]
    fn failed_batch_removes_every_partial_copy() {
        use std::os::unix::fs::PermissionsExt;

        let base = test_directory("rollback");
        let destination = base.join("destination");
        fs::create_dir_all(&destination).unwrap();
        let first = base.join("first.txt");
        let second = base.join("second.txt");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();
        fs::set_permissions(&second, fs::Permissions::from_mode(0o000)).unwrap();

        let mut rules = default_rules();
        rules.template = "Copy-{name}".to_string();
        let result = save_as_impl(
            vec![path_string(&first), path_string(&second)],
            rules,
            path_string(&destination),
        );
        fs::set_permissions(&second, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(result.is_err());
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
        let _ = fs::remove_dir_all(base);
    }
}
