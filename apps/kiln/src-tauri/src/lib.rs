mod convert;

use convert::{Resize, Settings, SourceInfo};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use tauri::{
    menu::{MenuBuilder, SubmenuBuilder},
    AppHandle, Emitter, Manager, State,
};

/// libvips is initialised once for the life of the process.
static VIPS: OnceLock<libvips::VipsApp> = OnceLock::new();

#[derive(Default)]
struct Batch {
    cancelled: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
enum Probe {
    Ok(SourceInfo),
    Failed { path: String, error: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
enum Progress {
    Done {
        path: String,
        output: String,
        bytes: u64,
    },
    Failed {
        path: String,
        error: String,
    },
    Skipped {
        path: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConversionJob {
    path: String,
    resize: Resize,
}

fn process_job(job: &ConversionJob, base_settings: &Settings, cancelled: &AtomicBool) -> Progress {
    if cancelled.load(Ordering::SeqCst) {
        return Progress::Skipped {
            path: job.path.clone(),
        };
    }

    let mut settings = base_settings.clone();
    settings.resize = job.resize;
    match convert::convert_one(&PathBuf::from(&job.path), &settings) {
        Ok(output) => {
            let bytes = std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0);
            Progress::Done {
                path: job.path.clone(),
                output: output.to_string_lossy().into_owned(),
                bytes,
            }
        }
        Err(error) => Progress::Failed {
            path: job.path.clone(),
            error,
        },
    }
}

const SUPPORTED: [&str; 10] = [
    "heic", "heif", "avif", "webp", "jpg", "jpeg", "jfif", "png", "tif", "tiff",
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CollectedImages {
    paths: Vec<String>,
    ignored: usize,
    truncated: bool,
    folder_depth_limited: bool,
    unreadable_folders: usize,
}

/// Turns whatever was dropped into a flat list of supported-extension paths.
/// A later probe identifies corrupt or unreadable images without stopping the batch.
#[tauri::command]
fn collect_images(paths: Vec<String>) -> CollectedImages {
    #[derive(Default)]
    struct CollectionState {
        found: Vec<String>,
        seen_directories: std::collections::HashSet<PathBuf>,
        seen_files: std::collections::HashSet<PathBuf>,
        ignored: usize,
        truncated: bool,
        folder_depth_limited: bool,
        unreadable_folders: usize,
    }

    fn walk(path: &std::path::Path, depth: usize, state: &mut CollectionState) {
        if state.found.len() >= 20_000 {
            state.truncated = true;
            return;
        }

        if path.is_dir() {
            // Deep enough for a real photo library, shallow enough that a
            // dropped home directory cannot run away with the app.
            if depth >= 8 {
                state.folder_depth_limited = true;
                return;
            }
            let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            if !state.seen_directories.insert(resolved) {
                return;
            }
            let Ok(entries) = std::fs::read_dir(path) else {
                state.unreadable_folders += 1;
                return;
            };
            let mut children: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
            children.sort();
            for child in children {
                walk(&child, depth + 1, state);
            }
            return;
        }

        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if SUPPORTED.contains(&extension.as_str()) {
            let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            if state.seen_files.insert(resolved.clone()) {
                if let Some(path) = resolved.to_str() {
                    state.found.push(path.to_string());
                }
            }
        } else {
            state.ignored += 1;
        }
    }

    let mut state = CollectionState::default();
    for path in paths {
        walk(&PathBuf::from(path), 0, &mut state);
    }
    CollectedImages {
        paths: state.found,
        ignored: state.ignored,
        truncated: state.truncated,
        folder_depth_limited: state.folder_depth_limited,
        unreadable_folders: state.unreadable_folders,
    }
}

/// A fresh temp directory to convert into before the user saves. Conversion
/// writes here first (step one); Save copies the results to their real home.
fn scratch_root() -> PathBuf {
    std::env::temp_dir().join(format!("kiln-{}", std::process::id()))
}

#[tauri::command]
fn scratch_dir() -> Result<String, String> {
    let dir = scratch_root().join(format!(
        "run-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    dir.to_str()
        .map(str::to_string)
        .ok_or_else(|| "temp path is not valid UTF-8".to_string())
}

/// Removes converted-but-unsaved scratch files. Paths outside Kiln's private
/// scratch root are rejected, so Clear can never delete originals or saved files.
#[tauri::command]
fn discard_files(paths: Vec<String>) -> Vec<String> {
    let root = scratch_root();
    let canonical_root = root.canonicalize().ok();
    let mut failures = Vec::new();

    for path in paths {
        let candidate = PathBuf::from(&path);
        if !candidate.exists() {
            continue;
        }

        let safe = match (&canonical_root, candidate.canonicalize()) {
            (Some(root), Ok(resolved)) => resolved.starts_with(root),
            _ => false,
        };
        if !safe || std::fs::remove_file(&candidate).is_err() {
            failures.push(path);
            continue;
        }

        if let Some(parent) = candidate.parent() {
            let _ = std::fs::remove_dir(parent);
        }
    }

    failures
}

#[derive(serde::Deserialize)]
struct SaveCopy {
    from: String,
    to: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
enum Saved {
    Ok { from: String, path: String },
    Failed { from: String, error: String },
}

/// Which of these target paths already exist — used to decide whether to prompt.
fn existing_targets_impl(paths: Vec<String>) -> Vec<String> {
    paths
        .into_iter()
        .filter(|p| PathBuf::from(p).exists())
        .collect()
}

#[tauri::command]
async fn existing_targets(paths: Vec<String>) -> Vec<String> {
    tauri::async_runtime::spawn_blocking(move || existing_targets_impl(paths))
        .await
        .unwrap_or_default()
}

fn reserve_save_target(path: &std::path::Path) -> Result<PathBuf, String> {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = path.parent().map(PathBuf::from).unwrap_or_default();

    for counter in 0..=10_000 {
        let candidate = if counter == 0 {
            path.to_path_buf()
        } else {
            let name = if ext.is_empty() {
                format!("{stem} ({counter})")
            } else {
                format!("{stem} ({counter}).{ext}")
            };
            dir.join(name)
        };
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(_) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("could not find a free filename".to_string())
}

fn reserve_sidecar(path: &std::path::Path, label: &str) -> Result<PathBuf, String> {
    let parent = path
        .parent()
        .ok_or_else(|| "destination has no parent folder".to_string())?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    for counter in 0..=1000 {
        let candidate = parent.join(format!(".kiln-{label}-{name}-{nonce}-{counter}"));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(_) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("could not create a temporary save file".to_string())
}

fn copy_with_replace(from: &std::path::Path, target: &std::path::Path) -> Result<(), String> {
    if target.exists() && !target.is_file() {
        return Err("destination exists but is not a file".to_string());
    }

    let temporary = reserve_sidecar(target, "new")?;
    if let Err(error) = std::fs::copy(from, &temporary) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error.to_string());
    }

    let backup = if target.exists() {
        let backup = match reserve_sidecar(target, "old") {
            Ok(backup) => backup,
            Err(error) => {
                let _ = std::fs::remove_file(&temporary);
                return Err(error);
            }
        };
        let _ = std::fs::remove_file(&backup);
        if let Err(error) = std::fs::rename(target, &backup) {
            let _ = std::fs::remove_file(&temporary);
            return Err(error.to_string());
        }
        Some(backup)
    } else {
        None
    };

    if let Err(error) = std::fs::rename(&temporary, target) {
        if let Some(backup) = &backup {
            let _ = std::fs::rename(backup, target);
        }
        let _ = std::fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    if let Some(backup) = backup {
        let _ = std::fs::remove_file(backup);
    }
    Ok(())
}

fn save_files_impl(copies: Vec<SaveCopy>, overwrite: bool) -> Vec<Saved> {
    let canonical_root = scratch_root().canonicalize().ok();
    copies
        .into_iter()
        .map(|item| {
            let from = PathBuf::from(&item.from);
            let safe_source = match (&canonical_root, from.canonicalize()) {
                (Some(root), Ok(resolved)) => resolved.starts_with(root) && resolved.is_file(),
                _ => false,
            };
            if !safe_source {
                return Saved::Failed {
                    from: item.from,
                    error: "temporary converted file is unavailable".to_string(),
                };
            }

            let to = PathBuf::from(&item.to);
            if let Some(parent) = to.parent() {
                if let Err(error) = std::fs::create_dir_all(parent) {
                    return Saved::Failed {
                        from: item.from,
                        error: error.to_string(),
                    };
                }
            }

            if overwrite {
                match copy_with_replace(&from, &to) {
                    Ok(()) => Saved::Ok {
                        from: item.from,
                        path: to.to_string_lossy().into_owned(),
                    },
                    Err(error) => Saved::Failed {
                        from: item.from,
                        error,
                    },
                }
            } else {
                let target = match reserve_save_target(&to) {
                    Ok(target) => target,
                    Err(error) => {
                        return Saved::Failed {
                            from: item.from,
                            error,
                        }
                    }
                };
                match std::fs::copy(&from, &target) {
                    Ok(_) => Saved::Ok {
                        from: item.from,
                        path: target.to_string_lossy().into_owned(),
                    },
                    Err(error) => {
                        let _ = std::fs::remove_file(&target);
                        Saved::Failed {
                            from: item.from,
                            error: error.to_string(),
                        }
                    }
                }
            }
        })
        .collect()
}

#[tauri::command]
async fn save_files(copies: Vec<SaveCopy>, overwrite: bool) -> Vec<Saved> {
    let sources: Vec<String> = copies.iter().map(|item| item.from.clone()).collect();
    match tauri::async_runtime::spawn_blocking(move || save_files_impl(copies, overwrite)).await {
        Ok(results) => results,
        Err(error) => sources
            .into_iter()
            .map(|from| Saved::Failed {
                from,
                error: error.to_string(),
            })
            .collect(),
    }
}

/// Reads dimensions and size for the preview table. Files that cannot be opened
/// come back as Failed rather than taking the whole call down.
#[tauri::command]
fn probe_files(paths: Vec<String>) -> Vec<Probe> {
    paths
        .into_par_iter()
        .map(|path| match convert::probe(&PathBuf::from(&path)) {
            Ok(info) => Probe::Ok(info),
            Err(error) => Probe::Failed { path, error },
        })
        .collect()
}

/// Starts a batch and returns immediately. Results arrive as `conversion-progress`
/// events, one per file, followed by a single `conversion-finished`.
#[tauri::command]
fn convert_batch(app: AppHandle, jobs: Vec<ConversionJob>, settings: Settings) {
    let cancelled = {
        let batch: State<Batch> = app.state();
        batch.cancelled.store(false, Ordering::SeqCst);
        batch.cancelled.clone()
    };

    std::thread::spawn(move || {
        jobs.par_iter().for_each(|job| {
            let progress = process_job(job, &settings, &cancelled);
            let _ = app.emit("conversion-progress", progress);
        });

        let _ = app.emit("conversion-finished", ());
    });
}

/// Files already in flight finish; the rest are skipped. Nothing written is undone.
#[tauri::command]
fn cancel_batch(batch: State<Batch>) {
    batch.cancelled.store(true, Ordering::SeqCst);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .menu(|app| {
            let kiln_menu = SubmenuBuilder::new(app, "Kiln")
                .about_with_text("About Kiln", None)
                .separator()
                .quit_with_text("Quit Kiln")
                .build()?;
            let view_menu = SubmenuBuilder::new(app, "View")
                .fullscreen_with_text("Toggle Full Screen")
                .build()?;

            MenuBuilder::new(app)
                .items(&[&kiln_menu, &view_menu])
                .build()
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(Batch::default())
        .setup(|_app| {
            if let Ok(executable) = std::env::current_exe() {
                if let Some(macos_dir) = executable.parent() {
                    if macos_dir.file_name().and_then(|name| name.to_str()) == Some("MacOS") {
                        if let Some(contents_dir) = macos_dir.parent() {
                            std::env::set_var("VIPSHOME", contents_dir);
                        }
                    }
                }
            }
            // Homebrew's GLib otherwise discovers plug-ins outside the app and
            // loads a second, differently signed libgio into the process.
            let gio_modules = scratch_root().join("gio-modules");
            std::fs::create_dir_all(&gio_modules)?;
            std::env::set_var("GIO_MODULE_DIR", gio_modules);
            let vips = libvips::VipsApp::default("kiln")?;
            // One thread per image rather than per operation: rayon already has
            // every core busy with a different file, and letting vips fan out on
            // top of that oversubscribes the machine and slows the batch down.
            vips.concurrency_set(1);
            let _ = VIPS.set(vips);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            collect_images,
            scratch_dir,
            discard_files,
            save_files,
            existing_targets,
            probe_files,
            convert_batch,
            cancel_batch
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_vips() {
        VIPS.get_or_init(|| libvips::VipsApp::default("kiln-lib-tests").unwrap());
    }

    fn conversion_settings(destination: &std::path::Path) -> Settings {
        Settings {
            format: convert::OutputFormat::Jpeg,
            quality: 80,
            resize: Resize::Original,
            keep_metadata: false,
            destination: destination.to_string_lossy().into_owned(),
            suffix: None,
        }
    }

    #[test]
    fn a_pre_cancelled_job_is_reported_cancelled_without_touching_its_source() {
        let job = ConversionJob {
            path: "/definitely/missing/source.jpg".to_string(),
            resize: Resize::Exact {
                width: 1024,
                height: 1024,
            },
        };
        let cancelled = AtomicBool::new(true);
        let progress = process_job(
            &job,
            &conversion_settings(&std::env::temp_dir()),
            &cancelled,
        );
        assert!(matches!(progress, Progress::Skipped { path } if path == job.path));
    }

    #[test]
    fn conversion_jobs_deserialize_original_plus_exact_per_row_resizes() {
        let original: ConversionJob = serde_json::from_value(serde_json::json!({
            "path": "/a.jpg",
            "resize": { "mode": "original" }
        }))
        .unwrap();
        let exact: ConversionJob = serde_json::from_value(serde_json::json!({
            "path": "/b.jpg",
            "resize": { "mode": "exact", "width": 2048, "height": 3072 }
        }))
        .unwrap();
        assert!(matches!(original.resize, Resize::Original));
        assert!(matches!(
            exact.resize,
            Resize::Exact {
                width: 2048,
                height: 3072
            }
        ));
    }

    #[test]
    fn one_failed_job_does_not_stop_a_valid_job() {
        test_vips();
        let base = std::env::temp_dir().join("kiln-mixed-conversion-jobs");
        let _ = std::fs::remove_dir_all(&base);
        let source_dir = base.join("sources");
        let scratch = base.join("scratch");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&scratch).unwrap();
        let valid = source_dir.join("valid.jpg");
        let image = libvips::ops::black(64, 64).unwrap();
        libvips::ops::jpegsave(&image, valid.to_str().unwrap()).unwrap();

        let jobs = [
            ConversionJob {
                path: valid.to_string_lossy().into_owned(),
                resize: Resize::Original,
            },
            ConversionJob {
                path: source_dir
                    .join("missing.jpg")
                    .to_string_lossy()
                    .into_owned(),
                resize: Resize::Original,
            },
        ];
        let cancelled = AtomicBool::new(false);
        let settings = conversion_settings(&scratch);
        let results: Vec<_> = jobs
            .par_iter()
            .map(|job| process_job(job, &settings, &cancelled))
            .collect();

        assert!(results
            .iter()
            .any(|result| matches!(result, Progress::Done { path, .. } if path == &jobs[0].path)));
        assert!(results.iter().any(
            |result| matches!(result, Progress::Failed { path, .. } if path == &jobs[1].path)
        ));
    }

    #[test]
    fn complete_native_pipeline_collects_probes_converts_saves_and_clears_real_images() {
        test_vips();
        let source_dir = std::env::temp_dir().join("kiln-native-e2e-sources");
        let export_dir = std::env::temp_dir().join("kiln-native-e2e-export");
        let scratch = scratch_root().join("run-native-e2e-pipeline-test");
        let _ = std::fs::remove_dir_all(&source_dir);
        let _ = std::fs::remove_dir_all(&export_dir);
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&export_dir).unwrap();
        std::fs::create_dir_all(&scratch).unwrap();

        let image = libvips::ops::black(96, 64).unwrap();
        let jpeg = source_dir.join("alpha.jpg");
        let png = source_dir.join("beta.png");
        let webp = source_dir.join("gamma.webp");
        libvips::ops::jpegsave(&image, jpeg.to_str().unwrap()).unwrap();
        libvips::ops::pngsave(&image, png.to_str().unwrap()).unwrap();
        libvips::ops::webpsave(&image, webp.to_str().unwrap()).unwrap();
        let original_bytes: Vec<_> = [&jpeg, &png, &webp]
            .iter()
            .map(|path| std::fs::read(path).unwrap())
            .collect();

        let collected = collect_images(vec![source_dir.to_string_lossy().into_owned()]);
        assert_eq!(collected.paths.len(), 3);
        assert_eq!(collected.ignored, 0);
        let probes = probe_files(collected.paths.clone());
        assert_eq!(probes.len(), 3);
        assert!(probes.iter().all(
            |probe| matches!(probe, Probe::Ok(info) if info.width == 96 && info.height == 64)
        ));

        let settings = Settings {
            format: convert::OutputFormat::Jpeg,
            quality: 40,
            resize: Resize::Original,
            keep_metadata: false,
            destination: scratch.to_string_lossy().into_owned(),
            suffix: Some("-20260826-013300".to_string()),
        };
        let jobs: Vec<_> = collected
            .paths
            .iter()
            .map(|path| ConversionJob {
                path: path.clone(),
                resize: Resize::Exact {
                    width: 1024,
                    height: 1024,
                },
            })
            .collect();
        let cancelled = AtomicBool::new(false);
        let converted: Vec<_> = jobs
            .iter()
            .map(|job| process_job(job, &settings, &cancelled))
            .collect();
        let scratch_outputs: Vec<String> = converted
            .iter()
            .map(|result| match result {
                Progress::Done { output, .. } => output.clone(),
                _ => panic!("every valid image should convert"),
            })
            .collect();
        assert_eq!(scratch_outputs.len(), 3);
        for output in &scratch_outputs {
            let info = convert::probe(std::path::Path::new(output)).unwrap();
            assert_eq!((info.width, info.height), (1024, 1024));
            assert!(output.ends_with("-20260826-013300.jpg"));
        }

        let copies: Vec<_> = scratch_outputs
            .iter()
            .map(|output| SaveCopy {
                from: output.clone(),
                to: export_dir
                    .join(PathBuf::from(output).file_name().unwrap())
                    .to_string_lossy()
                    .into_owned(),
            })
            .collect();
        let saved = save_files_impl(copies, false);
        assert_eq!(saved.len(), 3);
        assert!(saved
            .iter()
            .all(|result| matches!(result, Saved::Ok { .. })));
        assert_eq!(std::fs::read_dir(&export_dir).unwrap().count(), 3);

        let discard_failures = discard_files(scratch_outputs.clone());
        assert!(discard_failures.is_empty());
        assert!(scratch_outputs
            .iter()
            .all(|path| !PathBuf::from(path).exists()));
        assert_eq!(std::fs::read_dir(&export_dir).unwrap().count(), 3);
        for (index, source) in [&jpeg, &png, &webp].iter().enumerate() {
            assert_eq!(std::fs::read(source).unwrap(), original_bytes[index]);
        }

        let _ = std::fs::remove_dir_all(source_dir);
        let _ = std::fs::remove_dir_all(export_dir);
        let _ = std::fs::remove_dir_all(scratch);
    }

    #[test]
    fn save_files_copies_results_and_never_overwrites() {
        let base = scratch_root().join("run-save-keep-both-test");
        let _ = std::fs::remove_dir_all(&base);
        let scratch = base.join("scratch");
        let out = base.join("out");
        std::fs::create_dir_all(&scratch).unwrap();
        std::fs::create_dir_all(&out).unwrap();

        // Two scratch results and an existing file at the first target.
        std::fs::write(scratch.join("a.jpg"), b"one").unwrap();
        std::fs::write(scratch.join("b.jpg"), b"two").unwrap();
        std::fs::write(out.join("a.jpg"), b"existing").unwrap();

        let copies = vec![
            SaveCopy {
                from: scratch.join("a.jpg").to_string_lossy().into(),
                to: out.join("a.jpg").to_string_lossy().into(),
            },
            SaveCopy {
                from: scratch.join("b.jpg").to_string_lossy().into(),
                to: out.join("b.jpg").to_string_lossy().into(),
            },
        ];
        let results = save_files_impl(copies, false);

        // The existing a.jpg is untouched; the copy lands as "a (1).jpg".
        assert_eq!(std::fs::read(out.join("a.jpg")).unwrap(), b"existing");
        assert_eq!(std::fs::read(out.join("a (1).jpg")).unwrap(), b"one");
        assert_eq!(std::fs::read(out.join("b.jpg")).unwrap(), b"two");
        // Scratch is kept (copy, not move), so results can be saved again.
        assert!(scratch.join("a.jpg").exists());
        assert!(scratch.join("b.jpg").exists());
        assert!(results.iter().all(|r| matches!(r, Saved::Ok { .. })));

        let second_out = base.join("another-folder");
        std::fs::create_dir_all(&second_out).unwrap();
        let second_results = save_files_impl(
            vec![SaveCopy {
                from: scratch.join("a.jpg").to_string_lossy().into_owned(),
                to: second_out.join("a.jpg").to_string_lossy().into_owned(),
            }],
            false,
        );
        assert!(matches!(&second_results[0], Saved::Ok { .. }));
        assert_eq!(std::fs::read(second_out.join("a.jpg")).unwrap(), b"one");
    }

    #[test]
    fn overwrite_replaces_only_after_the_new_copy_is_complete() {
        let run = scratch_root().join("run-save-overwrite-test");
        let out = std::env::temp_dir().join("kiln-save-overwrite-output");
        let _ = std::fs::remove_dir_all(&run);
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&run).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        let scratch = run.join("converted.jpg");
        let target = out.join("original.jpg");
        std::fs::write(&scratch, b"converted-complete").unwrap();
        std::fs::write(&target, b"original").unwrap();

        let results = save_files_impl(
            vec![SaveCopy {
                from: scratch.to_string_lossy().into_owned(),
                to: target.to_string_lossy().into_owned(),
            }],
            true,
        );

        assert!(
            matches!(&results[0], Saved::Ok { path, .. } if path == target.to_string_lossy().as_ref())
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"converted-complete");
        assert_eq!(std::fs::read(&scratch).unwrap(), b"converted-complete");
        assert!(std::fs::read_dir(&out).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".kiln-")));
        let _ = std::fs::remove_dir_all(out);
    }

    #[test]
    fn a_missing_scratch_result_never_damages_an_existing_destination() {
        let run = scratch_root().join("run-save-missing-test");
        let out = std::env::temp_dir().join("kiln-save-missing-output");
        let _ = std::fs::remove_dir_all(&run);
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&run).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        let target = out.join("original.jpg");
        std::fs::write(&target, b"must-survive").unwrap();

        let results = save_files_impl(
            vec![SaveCopy {
                from: run.join("missing.jpg").to_string_lossy().into_owned(),
                to: target.to_string_lossy().into_owned(),
            }],
            true,
        );

        assert!(matches!(&results[0], Saved::Failed { .. }));
        assert_eq!(std::fs::read(&target).unwrap(), b"must-survive");
        let _ = std::fs::remove_dir_all(out);
    }

    #[test]
    fn save_rejects_sources_outside_kiln_scratch() {
        let outside = std::env::temp_dir().join("kiln-not-a-scratch-result.jpg");
        let target = std::env::temp_dir().join("kiln-not-a-scratch-target.jpg");
        std::fs::write(&outside, b"outside").unwrap();
        let _ = std::fs::remove_file(&target);

        let results = save_files_impl(
            vec![SaveCopy {
                from: outside.to_string_lossy().into_owned(),
                to: target.to_string_lossy().into_owned(),
            }],
            false,
        );

        assert!(matches!(&results[0], Saved::Failed { .. }));
        assert!(!target.exists());
        assert!(outside.exists());
        let _ = std::fs::remove_file(outside);
    }

    #[test]
    fn keep_both_advances_past_multiple_existing_numbers() {
        let run = scratch_root().join("run-save-numbering-test");
        let out = std::env::temp_dir().join("kiln-save-numbering-output");
        let _ = std::fs::remove_dir_all(&run);
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&run).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        let scratch = run.join("photo.jpg");
        std::fs::write(&scratch, b"new").unwrap();
        std::fs::write(out.join("photo.jpg"), b"zero").unwrap();
        std::fs::write(out.join("photo (1).jpg"), b"one").unwrap();

        let results = save_files_impl(
            vec![SaveCopy {
                from: scratch.to_string_lossy().into_owned(),
                to: out.join("photo.jpg").to_string_lossy().into_owned(),
            }],
            false,
        );

        assert!(matches!(&results[0], Saved::Ok { path, .. } if path.ends_with("photo (2).jpg")));
        assert_eq!(std::fs::read(out.join("photo.jpg")).unwrap(), b"zero");
        assert_eq!(std::fs::read(out.join("photo (1).jpg")).unwrap(), b"one");
        assert_eq!(std::fs::read(out.join("photo (2).jpg")).unwrap(), b"new");
        let _ = std::fs::remove_dir_all(out);
    }

    #[test]
    fn one_save_failure_does_not_stop_other_results() {
        let run = scratch_root().join("run-save-mixed-test");
        let out = std::env::temp_dir().join("kiln-save-mixed-output");
        let _ = std::fs::remove_dir_all(&run);
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&run).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        let valid = run.join("valid.jpg");
        std::fs::write(&valid, b"valid").unwrap();

        let results = save_files_impl(
            vec![
                SaveCopy {
                    from: valid.to_string_lossy().into_owned(),
                    to: out.join("valid.jpg").to_string_lossy().into_owned(),
                },
                SaveCopy {
                    from: run.join("missing.jpg").to_string_lossy().into_owned(),
                    to: out.join("missing.jpg").to_string_lossy().into_owned(),
                },
            ],
            false,
        );

        assert!(matches!(&results[0], Saved::Ok { .. }));
        assert!(matches!(&results[1], Saved::Failed { .. }));
        assert_eq!(std::fs::read(out.join("valid.jpg")).unwrap(), b"valid");
        assert!(!out.join("missing.jpg").exists());
        let _ = std::fs::remove_dir_all(out);
    }

    #[test]
    fn overwrite_refuses_to_replace_a_destination_directory() {
        let run = scratch_root().join("run-save-directory-target-test");
        let out = std::env::temp_dir().join("kiln-save-directory-target-output");
        let _ = std::fs::remove_dir_all(&run);
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&run).unwrap();
        std::fs::create_dir_all(out.join("photo.jpg")).unwrap();
        let scratch = run.join("photo.jpg");
        std::fs::write(&scratch, b"new").unwrap();

        let results = save_files_impl(
            vec![SaveCopy {
                from: scratch.to_string_lossy().into_owned(),
                to: out.join("photo.jpg").to_string_lossy().into_owned(),
            }],
            true,
        );

        assert!(matches!(&results[0], Saved::Failed { .. }));
        assert!(out.join("photo.jpg").is_dir());
        let _ = std::fs::remove_dir_all(out);
    }

    #[test]
    fn existing_target_detection_reports_only_exact_collisions() {
        let base = std::env::temp_dir().join("kiln-existing-targets-test");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let existing = base.join("existing.jpg");
        let missing = base.join("missing.jpg");
        std::fs::write(&existing, b"existing").unwrap();

        let collisions = existing_targets_impl(vec![
            existing.to_string_lossy().into_owned(),
            missing.to_string_lossy().into_owned(),
        ]);

        assert_eq!(collisions, vec![existing.to_string_lossy().into_owned()]);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn discard_files_removes_only_kiln_scratch_results() {
        let root = scratch_root();
        let run = root.join("run-discard-test");
        let _ = std::fs::remove_dir_all(&run);
        std::fs::create_dir_all(&run).unwrap();
        let temporary = run.join("converted.jpg");
        std::fs::write(&temporary, b"temporary").unwrap();

        let outside = std::env::temp_dir().join("kiln-original-must-survive.jpg");
        std::fs::write(&outside, b"original").unwrap();

        let failures = discard_files(vec![
            temporary.to_string_lossy().into_owned(),
            outside.to_string_lossy().into_owned(),
            run.join("already-missing.jpg")
                .to_string_lossy()
                .into_owned(),
        ]);

        assert!(!temporary.exists());
        assert!(outside.exists());
        assert_eq!(failures, vec![outside.to_string_lossy().into_owned()]);
        let _ = std::fs::remove_file(outside);
    }

    #[test]
    fn collect_images_accepts_supported_aliases_and_rejects_unsupported_files() {
        let base = std::env::temp_dir().join("kiln-collect-images-test");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let names = [
            "a.heic",
            "b.heif",
            "c.avif",
            "d.webp",
            "e.jpg",
            "f.jpeg",
            "g.jfif",
            "h.png",
            "i.tif",
            "j.tiff",
            "not-image.bmp",
            "notes.txt",
        ];
        for name in names {
            std::fs::write(base.join(name), b"fixture").unwrap();
        }

        let found = collect_images(vec![base.to_string_lossy().into_owned()]);
        let found_names: Vec<_> = found
            .paths
            .iter()
            .filter_map(|path| {
                PathBuf::from(path)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .collect();

        assert_eq!(found_names.len(), 10);
        assert!(found_names.contains(&"a.heic".to_string()));
        assert!(found_names.contains(&"j.tiff".to_string()));
        assert!(!found_names.contains(&"not-image.bmp".to_string()));
        assert!(!found_names.contains(&"notes.txt".to_string()));
        assert_eq!(found.ignored, 2);
        assert!(!found.truncated);
        assert!(!found.folder_depth_limited);
        assert_eq!(found.unreadable_folders, 0);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn collect_images_stops_at_twenty_thousand_files() {
        let base = std::env::temp_dir().join("kiln-collect-limit-test");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        for index in 0..20_005 {
            std::fs::write(base.join(format!("{index:05}.jpg")), b"").unwrap();
        }

        let found = collect_images(vec![base.to_string_lossy().into_owned()]);
        assert_eq!(found.paths.len(), 20_000);
        assert!(found.truncated);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn collect_images_deduplicates_repeated_paths() {
        let base = std::env::temp_dir().join("kiln-collect-duplicate-test");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let image = base.join("same.jpg");
        std::fs::write(&image, b"fixture").unwrap();

        let path = image.to_string_lossy().into_owned();
        let found = collect_images(vec![path.clone(), path]);
        assert_eq!(found.paths.len(), 1);
        assert_eq!(
            PathBuf::from(&found.paths[0]),
            image.canonicalize().unwrap()
        );
        assert_eq!(found.ignored, 0);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn probe_files_returns_ready_information_for_one_and_multiple_images() {
        test_vips();
        let base = std::env::temp_dir().join("kiln-probe-valid-test");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let first = base.join("first.png");
        let second = base.join("second.jpg");
        let image = libvips::ops::black(320, 240).unwrap();
        libvips::ops::pngsave(&image, first.to_str().unwrap()).unwrap();
        libvips::ops::jpegsave(&image, second.to_str().unwrap()).unwrap();

        let probes = probe_files(vec![
            first.to_string_lossy().into_owned(),
            second.to_string_lossy().into_owned(),
        ]);
        assert_eq!(probes.len(), 2);
        for probe in probes {
            match probe {
                Probe::Ok(info) => {
                    assert_eq!((info.width, info.height), (320, 240));
                    assert!(info.bytes > 0);
                }
                Probe::Failed { error, .. } => panic!("valid image failed: {error}"),
            }
        }
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn probe_files_marks_a_corrupt_supported_file_failed_without_stopping_valid_files() {
        test_vips();
        let base = std::env::temp_dir().join("kiln-probe-corrupt-test");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let valid = base.join("valid.png");
        let corrupt = base.join("corrupt.jpg");
        let image = libvips::ops::black(64, 48).unwrap();
        libvips::ops::pngsave(&image, valid.to_str().unwrap()).unwrap();
        std::fs::write(&corrupt, b"not an image").unwrap();

        let probes = probe_files(vec![
            corrupt.to_string_lossy().into_owned(),
            valid.to_string_lossy().into_owned(),
        ]);
        assert!(matches!(&probes[0], Probe::Failed { path, .. } if path.ends_with("corrupt.jpg")));
        assert!(matches!(&probes[1], Probe::Ok(info) if info.name == "valid.png"));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn collect_images_handles_empty_folders_and_uppercase_extensions() {
        let base = std::env::temp_dir().join("kiln-drop-empty-uppercase-test");
        let _ = std::fs::remove_dir_all(&base);
        let empty = base.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let empty_result = collect_images(vec![empty.to_string_lossy().into_owned()]);
        assert!(empty_result.paths.is_empty());
        assert_eq!(empty_result.ignored, 0);
        assert!(!empty_result.truncated);
        assert!(!empty_result.folder_depth_limited);
        assert_eq!(empty_result.unreadable_folders, 0);

        let uppercase = base.join("PHOTO.JPEG");
        std::fs::write(&uppercase, b"fixture").unwrap();
        let uppercase_result = collect_images(vec![uppercase.to_string_lossy().into_owned()]);
        assert_eq!(uppercase_result.paths.len(), 1);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn collect_images_enforces_the_eight_level_folder_boundary() {
        let base = std::env::temp_dir().join("kiln-drop-depth-test");
        let _ = std::fs::remove_dir_all(&base);
        let mut level = base.clone();
        for index in 1..=7 {
            level = level.join(format!("level-{index}"));
        }
        std::fs::create_dir_all(&level).unwrap();
        let within_boundary = level.join("within.png");
        std::fs::write(&within_boundary, b"fixture").unwrap();

        let beyond_directory = level.join("level-8");
        std::fs::create_dir_all(&beyond_directory).unwrap();
        std::fs::write(beyond_directory.join("beyond.png"), b"fixture").unwrap();

        let result = collect_images(vec![base.to_string_lossy().into_owned()]);
        assert_eq!(result.paths.len(), 1);
        assert!(result.paths[0].ends_with("within.png"));
        assert!(result.folder_depth_limited);
        let _ = std::fs::remove_dir_all(base);
    }

    #[cfg(unix)]
    #[test]
    fn collect_images_ignores_a_symlink_folder_loop() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join("kiln-drop-symlink-loop-test");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let image = base.join("image.jpg");
        std::fs::write(&image, b"fixture").unwrap();
        symlink(&base, base.join("loop")).unwrap();

        let result = collect_images(vec![base.to_string_lossy().into_owned()]);
        assert_eq!(result.paths.len(), 1);
        assert!(result.paths[0].ends_with("image.jpg"));
        let _ = std::fs::remove_dir_all(base);
    }

    #[cfg(unix)]
    #[test]
    fn collect_images_reports_an_unreadable_folder() {
        use std::os::unix::fs::PermissionsExt;

        let base = std::env::temp_dir().join("kiln-drop-unreadable-test");
        let _ = std::fs::remove_dir_all(&base);
        let blocked = base.join("blocked");
        std::fs::create_dir_all(&blocked).unwrap();
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o000)).unwrap();

        let result = collect_images(vec![blocked.to_string_lossy().into_owned()]);
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.paths.is_empty());
        assert_eq!(result.unreadable_folders, 1);
        let _ = std::fs::remove_dir_all(base);
    }
}
