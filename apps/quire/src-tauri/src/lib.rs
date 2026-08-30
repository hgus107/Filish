use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::OsString;
#[cfg(target_os = "macos")]
use std::ffi::{CStr, CString};
use std::fs::{self, OpenOptions};
use std::io;
#[cfg(target_os = "macos")]
use std::os::raw::{c_char, c_double, c_int};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

const MAX_FILES: usize = 20_000;
const MAX_FOLDER_DEPTH: usize = 8;
const STALE_RUN_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const SUPPORTED: [&str; 11] = [
    "doc", "docx", "odt", "rtf", "md", "markdown", "html", "htm", "txt", "pdf", "text",
];
static RUN_COUNTER: AtomicU64 = AtomicU64::new(0);
static CANCELLED_RUNS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn quire_pdf_page_count(
        path: *const c_char,
        error: *mut c_char,
        error_capacity: usize,
    ) -> c_int;
    fn quire_pdf_extract_text(
        path: *const c_char,
        output_path: *const c_char,
        error: *mut c_char,
        error_capacity: usize,
    ) -> c_int;
    fn quire_pdf_render_pages(
        path: *const c_char,
        output_directory: *const c_char,
        dpi: c_double,
        error: *mut c_char,
        error_capacity: usize,
    ) -> c_int;
    fn quire_pdf_compress(
        path: *const c_char,
        output_path: *const c_char,
        dpi: c_double,
        quality: c_double,
        error: *mut c_char,
        error_capacity: usize,
    ) -> c_int;
}

thread_local! {
    static CURRENT_RUN_ID: RefCell<String> = const { RefCell::new(String::new()) };
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileEntry {
    path: String,
    name: String,
    bytes: u64,
    extension: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CollectedFiles {
    files: Vec<FileEntry>,
    ignored: usize,
    truncated: bool,
    folder_depth_limited: bool,
    unreadable_folders: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskRequest {
    run_id: String,
    task_function: String,
    output_type: String,
    paths: Vec<String>,
    detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConversionOutput {
    source: String,
    path: String,
    name: String,
    bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunTaskResult {
    outputs: Vec<ConversionOutput>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveResult {
    saved: Vec<String>,
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn extension(path: &Path) -> String {
    path.extension()
        .map(|value| value.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn is_supported(path: &Path) -> bool {
    SUPPORTED.contains(&extension(path).as_str())
}

fn add_file(
    path: &Path,
    seen: &mut HashSet<PathBuf>,
    files: &mut Vec<FileEntry>,
    ignored: &mut usize,
) {
    if files.len() >= MAX_FILES || !path.is_file() {
        return;
    }
    if !is_supported(path) {
        *ignored += 1;
        return;
    }

    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !seen.insert(resolved.clone()) {
        return;
    }
    let metadata = match fs::metadata(&resolved) {
        Ok(metadata) => metadata,
        Err(_) => {
            *ignored += 1;
            return;
        }
    };
    files.push(FileEntry {
        path: path_string(&resolved),
        name: resolved
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Document".to_string()),
        bytes: metadata.len(),
        extension: extension(&resolved),
    });
}

fn collect_files_impl(paths: Vec<String>) -> CollectedFiles {
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    let mut ignored = 0;
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
            add_file(&root, &mut seen, &mut files, &mut ignored);
            continue;
        }
        if !root.is_dir() {
            ignored += 1;
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
                if entry.file_type().is_dir() {
                    walker.skip_current_dir();
                } else {
                    ignored += 1;
                }
                continue;
            }
            if entry.depth() == MAX_FOLDER_DEPTH && entry.file_type().is_dir() {
                folder_depth_limited = true;
            }
            if entry.file_type().is_file() {
                add_file(entry.path(), &mut seen, &mut files, &mut ignored);
            }
        }
    }

    CollectedFiles {
        files,
        ignored,
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

fn scratch_root() -> PathBuf {
    std::env::temp_dir().join("quire")
}

fn cleanup_stale_run_dirs_in(root: &Path, now: SystemTime) -> io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let cutoff = now
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .saturating_sub(STALE_RUN_AGE.as_millis());
    let mut failures = Vec::new();

    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(timestamp) = name
            .strip_prefix("run-")
            .and_then(|suffix| suffix.split('-').next())
            .and_then(|value| value.parse::<u128>().ok())
        else {
            continue;
        };
        if timestamp > cutoff {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            failures.push(entry.path());
            continue;
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        if fs::remove_dir_all(entry.path()).is_err() {
            failures.push(entry.path());
        }
    }

    Ok(failures)
}

fn cleanup_stale_run_dirs() -> io::Result<Vec<PathBuf>> {
    cleanup_stale_run_dirs_in(&scratch_root(), SystemTime::now())
}

fn create_run_dir() -> Result<PathBuf, String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let counter = RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = scratch_root().join(format!("run-{timestamp}-{}-{counter}", std::process::id()));
    fs::create_dir_all(&path)
        .map_err(|error| format!("Temporary Folder Could Not Be Created: {error}"))?;
    Ok(path)
}

fn packaged_tool_candidates(name: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            candidates.push(parent.join(name));
            candidates.push(parent.join("bin").join(name));
            if let Some(contents) = parent.parent() {
                candidates.push(contents.join("Resources").join("bin").join(name));
            }
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for folder in std::env::split_paths(&path) {
            candidates.push(folder.join(name));
        }
    }
    candidates
}

fn find_tool(names: &[&str]) -> Option<PathBuf> {
    for name in names {
        for candidate in packaged_tool_candidates(name) {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn require_tool(names: &[&str], display_name: &str) -> Result<PathBuf, String> {
    find_tool(names)
        .ok_or_else(|| format!("{display_name} Is Not Installed or Bundled With Quire."))
}

fn cancelled_runs() -> &'static Mutex<HashSet<String>> {
    CANCELLED_RUNS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn current_run_cancelled() -> bool {
    CURRENT_RUN_ID.with(|value| {
        let run_id = value.borrow();
        !run_id.is_empty()
            && cancelled_runs()
                .lock()
                .map(|runs| runs.contains(run_id.as_str()))
                .unwrap_or(false)
    })
}

fn ensure_not_cancelled() -> Result<(), String> {
    if current_run_cancelled() {
        Err("Conversion Cancelled.".to_string())
    } else {
        Ok(())
    }
}

fn run_command(program: &Path, args: &[OsString], context: &str) -> Result<(), String> {
    ensure_not_cancelled()?;
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("{context}: {error}"))?;
    loop {
        if current_run_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Conversion Cancelled.".to_string());
        }
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(error) => return Err(format!("{context}: {error}")),
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("{context}: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if detail.is_empty() {
        Err(format!("{context}."))
    } else {
        Err(format!("{context}: {detail}"))
    }
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Document".to_string())
}

fn unique_path(folder: &Path, name: &str, extension: &str) -> PathBuf {
    let first = folder.join(format!("{name}.{extension}"));
    if !first.exists() {
        return first;
    }
    for number in 2..=10_000 {
        let candidate = folder.join(format!("{name} ({number}).{extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    folder.join(format!("{name}-copy.{extension}"))
}

fn conversion_output(source: &Path, output: &Path) -> Result<ConversionOutput, String> {
    let bytes = fs::metadata(output)
        .map(|metadata| metadata.len())
        .map_err(|error| format!("Output File Could Not Be Read: {error}"))?;
    Ok(ConversionOutput {
        source: path_string(source),
        path: path_string(output),
        name: output
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Output.pdf".to_string()),
        bytes,
    })
}

#[cfg(target_os = "macos")]
fn native_path(path: &Path) -> Result<CString, String> {
    CString::new(path_string(path))
        .map_err(|_| "The PDF Path Contains an Unsupported Character.".to_string())
}

#[cfg(target_os = "macos")]
fn native_error(buffer: &[u8], fallback: &str) -> String {
    CStr::from_bytes_until_nul(buffer)
        .ok()
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback)
        .to_string()
}

#[cfg(target_os = "macos")]
fn native_extract_text(source: &Path, output: &Path) -> Result<(), String> {
    let source = native_path(source)?;
    let output = native_path(output)?;
    let mut error = vec![0u8; 1024];
    let result = unsafe {
        quire_pdf_extract_text(
            source.as_ptr(),
            output.as_ptr(),
            error.as_mut_ptr().cast(),
            error.len(),
        )
    };
    if result == 1 {
        Ok(())
    } else {
        Err(native_error(&error, "PDF Text Could Not Be Extracted."))
    }
}

#[cfg(not(target_os = "macos"))]
fn native_extract_text(_source: &Path, _output: &Path) -> Result<(), String> {
    Err("Native PDF Text Extraction Is Available on macOS.".to_string())
}

#[cfg(target_os = "macos")]
fn native_render_pages(source: &Path, output_directory: &Path, dpi: f64) -> Result<u32, String> {
    let source = native_path(source)?;
    let output_directory = native_path(output_directory)?;
    let mut error = vec![0u8; 1024];
    let result = unsafe {
        quire_pdf_render_pages(
            source.as_ptr(),
            output_directory.as_ptr(),
            dpi,
            error.as_mut_ptr().cast(),
            error.len(),
        )
    };
    if result > 0 {
        Ok(result as u32)
    } else {
        Err(native_error(&error, "Scanned PDF Could Not Be Rendered."))
    }
}

#[cfg(not(target_os = "macos"))]
fn native_render_pages(_source: &Path, _output_directory: &Path, _dpi: f64) -> Result<u32, String> {
    Err("Native PDF Rendering Is Available on macOS.".to_string())
}

#[cfg(target_os = "macos")]
fn native_compress_pdf(source: &Path, output: &Path, dpi: f64, quality: f64) -> Result<(), String> {
    let source = native_path(source)?;
    let output = native_path(output)?;
    let mut error = vec![0u8; 1024];
    let result = unsafe {
        quire_pdf_compress(
            source.as_ptr(),
            output.as_ptr(),
            dpi,
            quality,
            error.as_mut_ptr().cast(),
            error.len(),
        )
    };
    if result == 1 {
        Ok(())
    } else {
        Err(native_error(&error, "PDF Could Not Be Compressed."))
    }
}

#[cfg(not(target_os = "macos"))]
fn native_compress_pdf(
    _source: &Path,
    _output: &Path,
    _dpi: f64,
    _quality: f64,
) -> Result<(), String> {
    Err("Native PDF Compression Is Available on macOS.".to_string())
}

fn convert_to_pdf(source: &Path, run_dir: &Path, index: usize) -> Result<PathBuf, String> {
    if extension(source) == "pdf" {
        pdf_page_count(source)?;
        let output = unique_path(run_dir, &stem(source), "pdf");
        fs::copy(source, &output).map_err(|error| format!("PDF Could Not Be Copied: {error}"))?;
        return Ok(output);
    }

    let office_source = if matches!(extension(source).as_str(), "md" | "markdown") {
        let pandoc = require_tool(&["pandoc", "pandoc.exe"], "Pandoc")?;
        let intermediate = unique_path(run_dir, &format!("{}-markdown", stem(source)), "odt");
        run_command(
            &pandoc,
            &[
                source.as_os_str().to_owned(),
                OsString::from("-o"),
                intermediate.as_os_str().to_owned(),
            ],
            "Markdown Could Not Be Prepared for PDF Conversion",
        )?;
        intermediate
    } else {
        source.to_path_buf()
    };

    let office = require_tool(&["soffice", "libreoffice", "soffice.exe"], "LibreOffice")?;
    let office_dir = run_dir.join(format!("office-{index}"));
    fs::create_dir_all(&office_dir)
        .map_err(|error| format!("Temporary Folder Could Not Be Created: {error}"))?;
    run_command(
        &office,
        &[
            OsString::from("--headless"),
            OsString::from("--convert-to"),
            OsString::from("pdf"),
            OsString::from("--outdir"),
            office_dir.as_os_str().to_owned(),
            office_source.as_os_str().to_owned(),
        ],
        "Document Could Not Be Converted to PDF",
    )?;
    let generated = office_dir.join(format!("{}.pdf", stem(&office_source)));
    if !generated.is_file() {
        return Err("LibreOffice Did Not Produce a PDF.".to_string());
    }
    let output = unique_path(run_dir, &stem(source), "pdf");
    fs::rename(&generated, &output)
        .or_else(|_| fs::copy(&generated, &output).map(|_| ()))
        .map_err(|error| format!("Converted PDF Could Not Be Staged: {error}"))?;
    Ok(output)
}

fn merge_pdfs(inputs: &[PathBuf], output: &Path) -> Result<(), String> {
    if inputs.is_empty() {
        return Err("No PDFs Were Available to Merge.".to_string());
    }
    if inputs.len() == 1 {
        fs::copy(&inputs[0], output)
            .map_err(|error| format!("PDF Could Not Be Copied: {error}"))?;
        return Ok(());
    }

    if let Some(qpdf) = find_tool(&["qpdf", "qpdf.exe"]) {
        let mut args = vec![OsString::from("--empty"), OsString::from("--pages")];
        args.extend(inputs.iter().map(|path| path.as_os_str().to_owned()));
        args.push(OsString::from("--"));
        args.push(output.as_os_str().to_owned());
        return run_command(&qpdf, &args, "PDFs Could Not Be Merged");
    }
    Err("qpdf Is Not Installed or Bundled With Quire.".to_string())
}

fn layout_columns(line: &str) -> Vec<String> {
    let mut columns = Vec::new();
    let mut current = String::new();
    let mut spaces = 0usize;
    for character in line.trim().chars() {
        if character.is_whitespace() {
            spaces += 1;
            continue;
        }
        if spaces >= 2 && !current.trim().is_empty() {
            columns.push(current.trim().to_string());
            current.clear();
        } else if spaces == 1 && !current.is_empty() {
            current.push(' ');
        }
        spaces = 0;
        current.push(character);
    }
    if !current.trim().is_empty() {
        columns.push(current.trim().to_string());
    }
    columns
}

fn markdown_table_row(columns: &[String]) -> String {
    format!(
        "| {} |\n",
        columns
            .iter()
            .map(|value| value.replace('|', "\\|"))
            .collect::<Vec<_>>()
            .join(" | ")
    )
}

fn plain_to_markdown(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut output = String::new();
    let mut previous_blank = true;
    let mut index = 0usize;
    while index < lines.len() {
        let raw_line = lines[index];
        let line = raw_line.trim_end();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !previous_blank {
                output.push('\n');
            }
            previous_blank = true;
            index += 1;
            continue;
        }
        if trimmed.contains('\u{c}') {
            output.push_str("\n---\n\n");
            previous_blank = true;
            index += 1;
            continue;
        }
        let columns = layout_columns(line);
        let next_columns = lines
            .get(index + 1)
            .map(|next| layout_columns(next))
            .unwrap_or_default();
        if columns.len() >= 2 && next_columns.len() == columns.len() {
            output.push_str(&markdown_table_row(&columns));
            output.push('|');
            for _ in &columns {
                output.push_str(" --- |");
            }
            output.push('\n');
            index += 1;
            while index < lines.len() {
                let row = layout_columns(lines[index]);
                if row.len() != columns.len() {
                    break;
                }
                output.push_str(&markdown_table_row(&row));
                index += 1;
            }
            output.push('\n');
            previous_blank = true;
            continue;
        }
        let next_blank = lines
            .get(index + 1)
            .map(|next| next.trim().is_empty())
            .unwrap_or(true);
        let looks_like_heading = trimmed.len() <= 72
            && next_blank
            && !trimmed.ends_with(['.', ',', ';', ':'])
            && trimmed.chars().any(char::is_alphabetic);
        let lower = trimmed.to_lowercase();
        if lower.starts_with("figure ") || lower.starts_with("fig. ") {
            output.push_str("**");
            output.push_str(trimmed);
            output.push_str("**\n");
        } else if looks_like_heading {
            output.push_str("## ");
            output.push_str(trimmed);
            output.push('\n');
        } else {
            output.push_str(line);
            output.push('\n');
        }
        previous_blank = false;
        index += 1;
    }
    output.trim().to_string() + "\n"
}

fn extract_pdf(source: &Path, output_type: &str, run_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let temporary_text = unique_path(run_dir, &format!("{}-extracted", stem(source)), "txt");
    native_extract_text(source, &temporary_text)?;
    let text = fs::read_to_string(&temporary_text)
        .map_err(|error| format!("Extracted Text Could Not Be Read: {error}"))?;
    let mut outputs = Vec::new();
    if matches!(output_type, "text" | "both") {
        let text_output = unique_path(run_dir, &stem(source), "txt");
        fs::write(&text_output, &text)
            .map_err(|error| format!("Text File Could Not Be Created: {error}"))?;
        outputs.push(text_output);
    }
    if matches!(output_type, "markdown" | "both") {
        let markdown_output = unique_path(run_dir, &stem(source), "md");
        fs::write(&markdown_output, plain_to_markdown(&text))
            .map_err(|error| format!("Markdown File Could Not Be Created: {error}"))?;
        outputs.push(markdown_output);
    }
    let _ = fs::remove_file(temporary_text);
    if outputs.is_empty() {
        return Err("Choose Markdown, Text, or Markdown and Text.".to_string());
    }
    Ok(outputs)
}

fn parse_ranges(value: &str) -> Result<Vec<(u32, u32, String)>, String> {
    let parse_page = |part: &str| -> Result<u32, String> {
        if part.is_empty()
            || !part.bytes().all(|byte| byte.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return Err("Page Ranges Are Invalid.".to_string());
        }
        part.parse::<u32>()
            .map_err(|_| "Page Ranges Are Invalid.".to_string())
    };
    let mut ranges = Vec::new();
    if value.trim().is_empty() {
        return Err("Enter at Least One Page Range.".to_string());
    }
    for raw_token in value.split(',') {
        let token = raw_token.trim();
        if token.is_empty() {
            return Err("Page Ranges Are Invalid.".to_string());
        }
        let parts: Vec<&str> = token.split('-').map(str::trim).collect();
        let (start, end) = match parts.as_slice() {
            [page] => {
                let page = parse_page(page)?;
                (page, page)
            }
            [start, end] => (parse_page(start)?, parse_page(end)?),
            _ => return Err("Page Ranges Are Invalid.".to_string()),
        };
        if start == 0 || end == 0 || start > end {
            return Err("Page Ranges Must Use Positive Pages in Ascending Order.".to_string());
        }
        ranges.push((start, end, token.to_string()));
    }
    let mut ordered = ranges
        .iter()
        .map(|(start, end, _)| (*start, *end))
        .collect::<Vec<_>>();
    ordered.sort_unstable();
    if ordered.windows(2).any(|pair| pair[1].0 <= pair[0].1) {
        return Err("Page Ranges Must Not Overlap.".to_string());
    }
    Ok(ranges)
}

fn validate_task_output(task: &str, output_type: &str) -> Result<(), String> {
    let valid = match task {
        "to-pdf" => matches!(output_type, "combined" | "separate"),
        "extract" => matches!(output_type, "markdown" | "text" | "both"),
        "merge" => output_type == "merged",
        "split" => matches!(output_type, "individual" | "ranges"),
        "rotate" => matches!(output_type, "clockwise" | "counterclockwise" | "half-turn"),
        "compress" => matches!(output_type, "screen" | "standard" | "print"),
        "ocr" => output_type == "searchable",
        _ => return Err("Choose a Function.".to_string()),
    };
    if valid {
        return Ok(());
    }
    Err(match task {
        "to-pdf" => "Choose One PDF or Separate PDFs.",
        "extract" => "Choose Markdown, Text, or Markdown and Text.",
        "merge" => "Choose One Combined PDF.",
        "split" => "Choose Individual Pages or Page Ranges.",
        "rotate" => "Choose a Rotation Type.",
        "compress" => "Choose Smallest, Standard, or Largest.",
        "ocr" => "Choose Searchable PDF.",
        _ => "Choose a Type.",
    }
    .to_string())
}

fn pdf_page_count(source: &Path) -> Result<u32, String> {
    #[cfg(target_os = "macos")]
    {
        let source = native_path(source)?;
        let mut error = vec![0u8; 1024];
        let count = unsafe {
            quire_pdf_page_count(source.as_ptr(), error.as_mut_ptr().cast(), error.len())
        };
        if count > 0 {
            Ok(count as u32)
        } else {
            Err(native_error(&error, "PDF Page Count Could Not Be Read."))
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = source;
        Err("Native PDF Page Counting Is Available on macOS.".to_string())
    }
}

fn validate_ranges_within_pdf(source: &Path, ranges: &[(u32, u32, String)]) -> Result<u32, String> {
    let page_count = pdf_page_count(source)?;
    let invalid = ranges
        .iter()
        .filter(|(_, end, _)| *end > page_count)
        .map(|(start, end, _)| {
            let missing_start = (*start).max(page_count + 1);
            if missing_start == *end {
                missing_start.to_string()
            } else {
                format!("{missing_start}-{end}")
            }
        })
        .collect::<Vec<_>>();
    if invalid.is_empty() {
        return Ok(page_count);
    }
    let name = source
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "This PDF".to_string());
    Err(format!(
        "{name} has {page_count} pages. Pages {} do not exist.",
        invalid.join(", ")
    ))
}

fn numbered_component(path: &Path) -> u32 {
    path.file_stem()
        .and_then(|value| value.to_str())
        .and_then(|value| value.rsplit('-').next())
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0)
}

fn sorted_files(folder: &Path, wanted_extension: &str) -> Result<Vec<PathBuf>, String> {
    let mut files = fs::read_dir(folder)
        .map_err(|error| format!("Temporary Folder Could Not Be Read: {error}"))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| extension(path) == wanted_extension)
        .collect::<Vec<_>>();
    files.sort_by_key(|path| numbered_component(path));
    Ok(files)
}

fn split_pdf(
    source: &Path,
    output_type: &str,
    detail: &str,
    run_dir: &Path,
    source_index: usize,
) -> Result<Vec<PathBuf>, String> {
    let page_count = pdf_page_count(source)?;
    if page_count == 1 {
        return Err("This PDF has only one page and cannot be split.".to_string());
    }
    let source_dir = run_dir.join(format!("split-{source_index}"));
    fs::create_dir_all(&source_dir)
        .map_err(|error| format!("Temporary Folder Could Not Be Created: {error}"))?;
    if output_type == "individual" {
        let qpdf = require_tool(&["qpdf", "qpdf.exe"], "qpdf")?;
        let mut results = Vec::new();
        for page in 1..=page_count {
            let output = source_dir.join(format!("{}-page-{page}.pdf", stem(source)));
            run_command(
                &qpdf,
                &[
                    OsString::from("--empty"),
                    OsString::from("--pages"),
                    source.as_os_str().to_owned(),
                    OsString::from(page.to_string()),
                    OsString::from("--"),
                    output.as_os_str().to_owned(),
                ],
                "PDF Could Not Be Split",
            )?;
            results.push(output);
        }
        return Ok(results);
    }
    if output_type != "ranges" {
        return Err("Choose Individual Pages or Page Ranges.".to_string());
    }

    let ranges = parse_ranges(detail)?;
    validate_ranges_within_pdf(source, &ranges)?;
    let qpdf = require_tool(&["qpdf", "qpdf.exe"], "qpdf")?;
    let mut results = Vec::new();
    for (start, end, label) in &ranges {
        let output = unique_path(run_dir, &format!("{}-pages-{label}", stem(source)), "pdf");
        let range = if start == end {
            start.to_string()
        } else {
            format!("{start}-{end}")
        };
        run_command(
            &qpdf,
            &[
                OsString::from("--empty"),
                OsString::from("--pages"),
                source.as_os_str().to_owned(),
                OsString::from(range),
                OsString::from("--"),
                output.as_os_str().to_owned(),
            ],
            "PDF Page Range Could Not Be Created",
        )?;
        results.push(output);
    }
    Ok(results)
}

fn rotate_pdf(
    source: &Path,
    output_type: &str,
    detail: &str,
    run_dir: &Path,
) -> Result<PathBuf, String> {
    let qpdf = require_tool(&["qpdf", "qpdf.exe"], "PDF Rotation Engine")?;
    let angle = match output_type {
        "clockwise" => "+90",
        "counterclockwise" => "-90",
        "half-turn" => "+180",
        _ => return Err("Choose a Rotation Type.".to_string()),
    };
    let pages = if detail.trim().is_empty() || detail.eq_ignore_ascii_case("all") {
        "1-z".to_string()
    } else {
        if detail.contains(',') {
            return Err("Enter One Page or One Page Range to Rotate.".to_string());
        }
        let ranges = parse_ranges(detail)?;
        validate_ranges_within_pdf(source, &ranges)?;
        detail.replace(' ', "")
    };
    let output = unique_path(run_dir, &format!("{}-rotated", stem(source)), "pdf");
    run_command(
        &qpdf,
        &[
            source.as_os_str().to_owned(),
            OsString::from(format!("--rotate={angle}:{pages}")),
            OsString::from("--"),
            output.as_os_str().to_owned(),
        ],
        "PDF Could Not Be Rotated",
    )?;
    Ok(output)
}

fn compress_pdf(source: &Path, output_type: &str, run_dir: &Path) -> Result<PathBuf, String> {
    let (dpi, quality) = match output_type {
        "screen" => (96.0, 0.55),
        "standard" => (150.0, 0.72),
        "print" => (240.0, 0.86),
        _ => return Err("Choose Screen, Standard, or Print.".to_string()),
    };
    let output = unique_path(run_dir, &format!("{}-compressed", stem(source)), "pdf");
    let native_output = unique_path(
        run_dir,
        &format!("{}-native-compressed", stem(source)),
        "pdf",
    );
    native_compress_pdf(source, &native_output, dpi, quality)?;
    let mut candidates = vec![source.to_path_buf(), native_output.clone()];
    if let Some(qpdf) = find_tool(&["qpdf", "qpdf.exe"]) {
        let optimized = unique_path(run_dir, &format!("{}-optimized", stem(source)), "pdf");
        if run_command(
            &qpdf,
            &[
                OsString::from("--object-streams=generate"),
                OsString::from("--compress-streams=y"),
                OsString::from("--recompress-flate"),
                OsString::from("--compression-level=9"),
                native_output.as_os_str().to_owned(),
                optimized.as_os_str().to_owned(),
            ],
            "PDF Could Not Be Optimized",
        )
        .is_ok()
        {
            candidates.push(optimized);
        }
    }
    let smallest = candidates
        .iter()
        .filter_map(|path| {
            fs::metadata(path)
                .ok()
                .map(|metadata| (metadata.len(), path))
        })
        .min_by_key(|(bytes, _)| *bytes)
        .map(|(_, path)| path)
        .ok_or_else(|| "Compressed PDF Could Not Be Read.".to_string())?;
    fs::copy(smallest, &output)
        .map_err(|error| format!("Compressed PDF Could Not Be Staged: {error}"))?;
    Ok(output)
}

fn ocr_pdf(source: &Path, run_dir: &Path, index: usize) -> Result<PathBuf, String> {
    let tesseract = require_tool(&["tesseract", "tesseract.exe"], "Tesseract OCR")?;
    let pages_dir = run_dir.join(format!("ocr-{index}"));
    fs::create_dir_all(&pages_dir)
        .map_err(|error| format!("Temporary Folder Could Not Be Created: {error}"))?;
    native_render_pages(source, &pages_dir, 300.0)?;
    let images = sorted_files(&pages_dir, "png")?;
    if images.is_empty() {
        return Err("No Scanned Pages Were Found.".to_string());
    }
    let mut page_pdfs = Vec::new();
    for (page_index, image) in images.iter().enumerate() {
        let output_base = pages_dir.join(format!("ocr-page-{}", page_index + 1));
        run_command(
            &tesseract,
            &[
                image.as_os_str().to_owned(),
                output_base.as_os_str().to_owned(),
                OsString::from("pdf"),
            ],
            "A Scanned Page Could Not Be Recognized",
        )?;
        page_pdfs.push(output_base.with_extension("pdf"));
    }
    let output = unique_path(run_dir, &format!("{}-searchable", stem(source)), "pdf");
    merge_pdfs(&page_pdfs, &output)?;
    Ok(output)
}

fn validate_pdf_sources(paths: &[PathBuf]) -> Result<(), String> {
    if paths.iter().any(|path| extension(path) != "pdf") {
        return Err("This Function Accepts PDF Files Only.".to_string());
    }
    for path in paths {
        pdf_page_count(path)?;
    }
    Ok(())
}

#[allow(dead_code)]
fn run_task_sync(request: TaskRequest) -> Result<RunTaskResult, String> {
    run_task_sync_with_progress(request, |_, _, _| {})
}

fn run_task_sync_with_progress<F>(
    request: TaskRequest,
    progress: F,
) -> Result<RunTaskResult, String>
where
    F: Fn(usize, usize, &str),
{
    CURRENT_RUN_ID.with(|value| *value.borrow_mut() = request.run_id.clone());
    let result = run_task_sync_inner(request, &progress);
    CURRENT_RUN_ID.with(|value| value.borrow_mut().clear());
    result
}

fn run_task_sync_inner<F>(request: TaskRequest, progress: &F) -> Result<RunTaskResult, String>
where
    F: Fn(usize, usize, &str),
{
    ensure_not_cancelled()?;
    if request.paths.is_empty() {
        return Err("Import at Least One File.".to_string());
    }
    if request.output_type.trim().is_empty() {
        return Err("Choose a Type.".to_string());
    }
    validate_task_output(&request.task_function, &request.output_type)?;
    let sources: Vec<PathBuf> = request.paths.iter().map(PathBuf::from).collect();
    if sources.iter().any(|path| !path.is_file()) {
        return Err("One or More Source Files Could Not Be Found.".to_string());
    }
    let run_dir = create_run_dir()?;
    let operation = (|| -> Result<RunTaskResult, String> {
        let mut outputs = Vec::new();
        let warnings = Vec::new();

        match request.task_function.as_str() {
            "to-pdf" => {
                if sources.iter().any(|source| extension(source) == "pdf") {
                    return Err("Add Non-PDF Files to Convert.".to_string());
                }
                let mut pdfs = Vec::new();
                for (index, source) in sources.iter().enumerate() {
                    ensure_not_cancelled()?;
                    pdfs.push(convert_to_pdf(source, &run_dir, index)?);
                    progress(index + 1, sources.len(), "Preparing PDFs");
                }
                if request.output_type == "separate" {
                    for (source, output) in sources.iter().zip(pdfs.iter()) {
                        outputs.push(conversion_output(source, output)?);
                    }
                } else if request.output_type == "combined" {
                    let output = unique_path(&run_dir, "Combined Documents", "pdf");
                    merge_pdfs(&pdfs, &output)?;
                    outputs.push(conversion_output(Path::new(""), &output)?);
                } else {
                    return Err("Choose One PDF or Separate PDFs.".to_string());
                }
            }
            "extract" => {
                validate_pdf_sources(&sources)?;
                for (index, source) in sources.iter().enumerate() {
                    ensure_not_cancelled()?;
                    for output in extract_pdf(source, &request.output_type, &run_dir)? {
                        outputs.push(conversion_output(source, &output)?);
                    }
                    progress(index + 1, sources.len(), "Extracting Text");
                }
            }
            "merge" => {
                ensure_not_cancelled()?;
                validate_pdf_sources(&sources)?;
                if sources.len() < 2 {
                    return Err("Select at Least Two PDFs to Merge.".to_string());
                }
                let output = unique_path(&run_dir, "Merged Documents", "pdf");
                merge_pdfs(&sources, &output)?;
                outputs.push(conversion_output(Path::new(""), &output)?);
                progress(1, 1, "Merging PDFs");
            }
            "split" => {
                ensure_not_cancelled()?;
                validate_pdf_sources(&sources)?;
                if sources.len() != 1 {
                    return Err(
                        "Only One PDF Can Be Split at a Time. Please Select One PDF.".to_string(),
                    );
                }
                for (index, source) in sources.iter().enumerate() {
                    for output in split_pdf(
                        source,
                        &request.output_type,
                        &request.detail,
                        &run_dir,
                        index,
                    )? {
                        outputs.push(conversion_output(source, &output)?);
                    }
                    progress(index + 1, sources.len(), "Splitting PDF");
                }
            }
            "rotate" => {
                validate_pdf_sources(&sources)?;
                for (index, source) in sources.iter().enumerate() {
                    ensure_not_cancelled()?;
                    let output =
                        rotate_pdf(source, &request.output_type, &request.detail, &run_dir)?;
                    outputs.push(conversion_output(source, &output)?);
                    progress(index + 1, sources.len(), "Rotating PDFs");
                }
            }
            "compress" => {
                validate_pdf_sources(&sources)?;
                for (index, source) in sources.iter().enumerate() {
                    ensure_not_cancelled()?;
                    let output = compress_pdf(source, &request.output_type, &run_dir)?;
                    outputs.push(conversion_output(source, &output)?);
                    progress(index + 1, sources.len(), "Compressing PDFs");
                }
            }
            "ocr" => {
                validate_pdf_sources(&sources)?;
                for (index, source) in sources.iter().enumerate() {
                    ensure_not_cancelled()?;
                    let output = ocr_pdf(source, &run_dir, index)?;
                    outputs.push(conversion_output(source, &output)?);
                    progress(index + 1, sources.len(), "Recognizing Scanned PDFs");
                }
            }
            _ => return Err("Choose a Function.".to_string()),
        }

        Ok(RunTaskResult { outputs, warnings })
    })();
    if operation.is_err() {
        let _ = fs::remove_dir_all(&run_dir);
    }
    operation
}

#[tauri::command]
async fn run_task(request: TaskRequest) -> Result<RunTaskResult, String> {
    let run_id = request.run_id.clone();
    let result = tauri::async_runtime::spawn_blocking(move || run_task_sync(request))
        .await
        .map_err(|error| error.to_string())?;
    if let Ok(mut runs) = cancelled_runs().lock() {
        runs.remove(&run_id);
    }
    result
}

#[tauri::command]
fn cancel_task(run_id: String) -> Result<(), String> {
    if run_id.trim().is_empty() {
        return Ok(());
    }
    cancelled_runs()
        .lock()
        .map_err(|_| "Task Could Not Be Cancelled.".to_string())?
        .insert(run_id);
    Ok(())
}

#[tauri::command]
async fn save_outputs(
    paths: Vec<String>,
    destination: String,
    overwrite: bool,
) -> Result<SaveResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let entered = if destination == "~" {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(&destination))
        } else if let Some(relative) = destination.strip_prefix("~/") {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(relative))
                .unwrap_or_else(|| PathBuf::from(&destination))
        } else {
            PathBuf::from(&destination)
        };
        if !entered.is_absolute() {
            return Err("Type an Absolute Save Path.".to_string());
        }
        if paths.is_empty() {
            return Err("There Are No Output Files to Save.".to_string());
        }
        let root_path = scratch_root();
        fs::create_dir_all(&root_path)
            .map_err(|error| format!("Temporary Folder Could Not Be Opened: {error}"))?;
        let root = root_path.canonicalize().unwrap_or(root_path);
        let mut sources = Vec::new();
        let mut names = HashSet::new();
        for raw_path in paths {
            let source = PathBuf::from(raw_path);
            let resolved = source
                .canonicalize()
                .map_err(|_| "An Output File Could Not Be Found.".to_string())?;
            if !resolved.starts_with(&root) {
                return Err(
                    "Quire Refused to Save a File Outside Its Temporary Folder.".to_string()
                );
            }
            let name = resolved
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
                .ok_or_else(|| "An Output File Has No Filename.".to_string())?;
            if !names.insert(name.to_lowercase()) {
                return Err(format!("More Than One Output File Is Named '{name}'."));
            }
            sources.push((resolved, name));
        }

        let mut planned = Vec::new();
        if sources.len() == 1 {
            let (source, original_name) = &sources[0];
            let target = if entered.is_dir() {
                entered
                    .canonicalize()
                    .map_err(|error| format!("The Destination Folder Could Not Be Opened: {error}"))?
                    .join(original_name)
            } else {
                let parent = entered
                    .parent()
                    .filter(|path| path.is_dir())
                    .ok_or_else(|| "Select an existing folder for the file.".to_string())?
                    .canonicalize()
                    .map_err(|error| format!("The Destination Folder Could Not Be Opened: {error}"))?;
                let file_name = entered
                    .file_name()
                    .filter(|name| !name.is_empty())
                    .ok_or_else(|| "Type a filename after the folder path.".to_string())?;
                parent.join(file_name)
            };
            planned.push((source.clone(), target));
        } else {
            if !entered.is_dir() {
                return Err("Select an existing destination folder for multiple files.".to_string());
            }
            let folder = entered
                .canonicalize()
                .map_err(|error| format!("The Destination Folder Could Not Be Opened: {error}"))?;
            planned.extend(
                sources
                    .iter()
                    .map(|(source, name)| (source.clone(), folder.join(name))),
            );
        }

        for (_, target) in &planned {
            if target.exists() && !overwrite {
                return Err(format!("FILE_EXISTS::{}", path_string(target)));
            }
        }

        let created = if overwrite {
            let mut staged: Vec<(PathBuf, PathBuf, Option<PathBuf>)> = Vec::new();
            for (source, target) in &planned {
                let temp = target.with_file_name(format!(
                    ".quire-save-{}.tmp",
                    RUN_COUNTER.fetch_add(1, Ordering::Relaxed)
                ));
                let copy_result = (|| -> Result<(), io::Error> {
                    let mut input = fs::File::open(source)?;
                    let mut output = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&temp)?;
                    io::copy(&mut input, &mut output)?;
                    output.set_permissions(fs::metadata(source)?.permissions())?;
                    output.sync_all()
                })();
                if let Err(error) = copy_result {
                    let _ = fs::remove_file(&temp);
                    for (staged_temp, _, _) in &staged {
                        let _ = fs::remove_file(staged_temp);
                    }
                    return Err(format!(
                        "Output Files Could Not Be Prepared for Overwrite: {error}. Existing Files Were Kept."
                    ));
                }
                staged.push((temp, target.clone(), None));
            }

            for index in 0..staged.len() {
                let target = staged[index].1.clone();
                if !target.exists() {
                    continue;
                }
                let backup = target.with_file_name(format!(
                    ".quire-backup-{}.tmp",
                    RUN_COUNTER.fetch_add(1, Ordering::Relaxed)
                ));
                if let Err(error) = fs::rename(&target, &backup) {
                    for (_, previous_target, previous_backup) in &staged[..index] {
                        if let Some(previous_backup) = previous_backup {
                            let _ = fs::rename(previous_backup, previous_target);
                        }
                    }
                    for (temp, _, _) in &staged {
                        let _ = fs::remove_file(temp);
                    }
                    return Err(format!(
                        "Existing Files Could Not Be Prepared for Overwrite: {error}. Existing Files Were Kept."
                    ));
                }
                staged[index].2 = Some(backup);
            }

            for (installed, (temp, target, _)) in staged.iter().enumerate() {
                if let Err(error) = fs::rename(temp, target) {
                    for (_, installed_target, _) in &staged[..installed] {
                        let _ = fs::remove_file(installed_target);
                    }
                    for (_, original_target, backup) in &staged {
                        if let Some(backup) = backup {
                            let _ = fs::rename(backup, original_target);
                        }
                    }
                    for (remaining_temp, _, _) in &staged[installed..] {
                        let _ = fs::remove_file(remaining_temp);
                    }
                    return Err(format!(
                        "Output Files Could Not Be Overwritten: {error}. Existing Files Were Restored."
                    ));
                }
            }
            for (_, _, backup) in &staged {
                if let Some(backup) = backup {
                    let _ = fs::remove_file(backup);
                }
            }
            planned.iter().map(|(_, target)| target.clone()).collect()
        } else {
            let mut created = Vec::new();
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
                    for created_path in created.iter().rev() {
                        let _ = fs::remove_file(created_path);
                    }
                    return Err(format!(
                        "Output Files Could Not Be Saved: {error}. No Partial Files Were Kept."
                    ));
                }
            }
            created
        };
        let saved = created.iter().map(|path| path_string(path)).collect();
        Ok(SaveResult { saved })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn discard_outputs(paths: Vec<String>) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root_path = scratch_root();
        fs::create_dir_all(&root_path)
            .map_err(|error| format!("Temporary Folder Could Not Be Opened: {error}"))?;
        let root = root_path.canonicalize().unwrap_or(root_path);
        let mut failures = Vec::new();
        let mut run_directories = HashSet::new();
        for raw_path in paths {
            let path = PathBuf::from(&raw_path);
            if !path.exists() {
                continue;
            }
            let resolved = match path.canonicalize() {
                Ok(path) => path,
                Err(_) => {
                    failures.push(raw_path);
                    continue;
                }
            };
            if !resolved.starts_with(&root) {
                failures.push(raw_path);
                continue;
            }
            let relative = match resolved.strip_prefix(&root) {
                Ok(relative) => relative,
                Err(_) => {
                    failures.push(raw_path);
                    continue;
                }
            };
            let Some(run_name) = relative.components().next() else {
                failures.push(raw_path);
                continue;
            };
            let run_dir = root.join(run_name.as_os_str());
            if run_dir.parent() != Some(root.as_path()) {
                failures.push(raw_path);
                continue;
            }
            run_directories.insert(run_dir);
        }
        for run_dir in run_directories {
            if fs::remove_dir_all(&run_dir).is_err() {
                failures.push(path_string(&run_dir));
            }
        }
        Ok(failures)
    })
    .await
    .map_err(|error| error.to_string())?
}

pub fn run() {
    match cleanup_stale_run_dirs() {
        Ok(failures) => {
            for path in failures {
                eprintln!(
                    "Quire could not remove stale temporary folder: {}",
                    path.display()
                );
            }
        }
        Err(error) => eprintln!("Quire could not inspect temporary folders: {error}"),
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            collect_files,
            run_task,
            cancel_task,
            save_outputs,
            discard_outputs
        ])
        .run(tauri::generate_context!())
        .expect("error while running Quire");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "quire-test-{name}-{}-{}",
            std::process::id(),
            RUN_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn startup_cleanup_removes_only_old_quire_run_directories() {
        let root = test_dir("stale-run-cleanup");
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10 * 24 * 60 * 60);
        let now_ms = now.duration_since(UNIX_EPOCH).unwrap().as_millis();
        let old_ms = now_ms - STALE_RUN_AGE.as_millis() - 1;
        let fresh_ms = now_ms - STALE_RUN_AGE.as_millis() + 1;
        let old = root.join(format!("run-{old_ms}-100-0"));
        let fresh = root.join(format!("run-{fresh_ms}-100-1"));
        let unrelated = root.join("run-invalid");
        let matching_file = root.join(format!("run-{old_ms}-100-2"));
        fs::create_dir_all(&old).unwrap();
        fs::create_dir_all(&fresh).unwrap();
        fs::create_dir_all(&unrelated).unwrap();
        fs::write(&matching_file, b"not a directory").unwrap();

        let failures = cleanup_stale_run_dirs_in(&root, now).unwrap();

        assert!(failures.is_empty());
        assert!(!old.exists());
        assert!(fresh.exists());
        assert!(unrelated.exists());
        assert!(matching_file.exists());
        let _ = fs::remove_dir_all(root);
    }

    fn test_pdf(folder: &Path, name: &str, pages: usize) -> PathBuf {
        let output = folder.join(format!("{name}.pdf"));
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/Quire Native Test.pdf");
        let qpdf = require_tool(&["qpdf"], "qpdf").unwrap();
        let mut args = vec![OsString::from("--empty"), OsString::from("--pages")];
        for _ in 0..pages {
            args.push(fixture.as_os_str().to_owned());
            args.push(OsString::from("1"));
        }
        args.push(OsString::from("--"));
        args.push(output.as_os_str().to_owned());
        run_command(&qpdf, &args, "Test PDF Could Not Be Created").unwrap();
        output
    }

    fn empty_test_pdf(folder: &Path, name: &str) -> PathBuf {
        let output = folder.join(format!("{name}.pdf"));
        let qpdf = require_tool(&["qpdf"], "qpdf").unwrap();
        run_command(
            &qpdf,
            &[
                OsString::from("--empty"),
                OsString::from("--"),
                output.as_os_str().to_owned(),
            ],
            "Empty Test PDF Could Not Be Created",
        )
        .unwrap();
        output
    }

    fn request(
        task_function: &str,
        output_type: &str,
        paths: &[PathBuf],
        detail: &str,
    ) -> TaskRequest {
        TaskRequest {
            run_id: String::new(),
            task_function: task_function.to_string(),
            output_type: output_type.to_string(),
            paths: paths.iter().map(|path| path_string(path)).collect(),
            detail: detail.to_string(),
        }
    }

    fn office_fixture(folder: &Path, extension: &str) -> PathBuf {
        let source_dir = folder.join(format!("source-{extension}"));
        let output_dir = folder.join(format!("fixture-{extension}"));
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&output_dir).unwrap();
        let source = source_dir.join("Office Fixture.txt");
        fs::write(&source, "Quire office conversion fixture.\n").unwrap();
        let office = require_tool(&["soffice", "libreoffice"], "LibreOffice").unwrap();
        run_command(
            &office,
            &[
                OsString::from("--headless"),
                OsString::from("--convert-to"),
                OsString::from(extension),
                OsString::from("--outdir"),
                output_dir.as_os_str().to_owned(),
                source.as_os_str().to_owned(),
            ],
            "Office Test Fixture Could Not Be Created",
        )
        .unwrap();
        output_dir.join(format!("Office Fixture.{extension}"))
    }

    #[test]
    fn collection_keeps_supported_documents_only() {
        let folder = test_dir("collection");
        fs::write(folder.join("report.docx"), b"doc").unwrap();
        fs::write(folder.join("notes.md"), b"notes").unwrap();
        fs::write(folder.join("photo.jpg"), b"image").unwrap();
        let result = collect_files_impl(vec![path_string(&folder)]);
        assert_eq!(result.files.len(), 2);
        assert_eq!(result.ignored, 1);
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn collection_handles_duplicates_hidden_depth_and_file_limit_boundaries() {
        let folder = test_dir("collection-boundaries");
        let visible = folder.join("VISIBLE.PDF");
        fs::write(&visible, b"fixture").unwrap();
        fs::write(folder.join(".hidden.pdf"), b"hidden").unwrap();

        let mut deepest = folder.clone();
        for level in 1..=MAX_FOLDER_DEPTH {
            deepest = deepest.join(format!("level-{level}"));
            fs::create_dir_all(&deepest).unwrap();
        }
        let collected = collect_files_impl(vec![
            path_string(&folder),
            path_string(&visible),
            path_string(&visible),
        ]);
        assert_eq!(collected.files.len(), 1);
        assert_eq!(collected.files[0].extension, "pdf");
        assert_eq!(collected.ignored, 1);
        assert!(collected.folder_depth_limited);

        let limit_folder = test_dir("collection-file-limit");
        for index in 0..=MAX_FILES {
            fs::write(limit_folder.join(format!("{index:05}.txt")), b"").unwrap();
        }
        let limited = collect_files_impl(vec![path_string(&limit_folder)]);
        assert_eq!(limited.files.len(), MAX_FILES);
        assert!(limited.truncated);

        let _ = fs::remove_dir_all(folder);
        let _ = fs::remove_dir_all(limit_folder);
    }

    #[test]
    fn page_ranges_accept_pages_and_ranges() {
        let result = parse_ranges("1-3, 5, 8-10").unwrap();
        assert_eq!(result[0].0, 1);
        assert_eq!(result[0].1, 3);
        assert_eq!(result[1].0, 5);
        assert_eq!(result[1].1, 5);
        assert_eq!(result[2].0, 8);
        assert_eq!(result[2].1, 10);
    }

    #[test]
    fn page_ranges_reject_zero_and_descending_ranges() {
        assert!(parse_ranges("0").is_err());
        assert!(parse_ranges("9-3").is_err());
        assert_eq!(
            parse_ranges("").unwrap_err(),
            "Enter at Least One Page Range."
        );
        assert_eq!(
            parse_ranges("1,,3").unwrap_err(),
            "Page Ranges Are Invalid."
        );
        assert_eq!(
            parse_ranges("1-3, 3-5").unwrap_err(),
            "Page Ranges Must Not Overlap."
        );
        assert_eq!(
            parse_ranges("5, 1-5").unwrap_err(),
            "Page Ranges Must Not Overlap."
        );
        assert!(parse_ranges("5, 1-3").is_ok());
        assert!(parse_ranges("4294967295").is_ok());
        assert_eq!(
            parse_ranges("4294967296").unwrap_err(),
            "Page Ranges Are Invalid."
        );
    }

    #[test]
    fn native_backend_rejects_every_invalid_request_shape_and_output() {
        let folder = test_dir("invalid-requests");
        let pdf = test_pdf(&folder, "Valid", 2);
        let second_pdf = test_pdf(&folder, "Second", 1);
        let text = folder.join("Not PDF.txt");
        fs::write(&text, "not a PDF").unwrap();

        assert_eq!(
            run_task_sync(request("compress", "standard", &[], "")).unwrap_err(),
            "Import at Least One File."
        );
        assert_eq!(
            run_task_sync(request("compress", "", std::slice::from_ref(&pdf), "")).unwrap_err(),
            "Choose a Type."
        );
        assert_eq!(
            run_task_sync(request(
                "to-pdf",
                "separate",
                std::slice::from_ref(&pdf),
                ""
            ))
            .unwrap_err(),
            "Add Non-PDF Files to Convert."
        );
        assert_eq!(
            run_task_sync(request(
                "unknown",
                "anything",
                std::slice::from_ref(&pdf),
                ""
            ))
            .unwrap_err(),
            "Choose a Function."
        );
        assert_eq!(
            run_task_sync(request(
                "compress",
                "standard",
                &[folder.join("Missing.pdf")],
                ""
            ))
            .unwrap_err(),
            "One or More Source Files Could Not Be Found."
        );

        for (task, expected) in [
            ("to-pdf", "Choose One PDF or Separate PDFs."),
            ("extract", "Choose Markdown, Text, or Markdown and Text."),
            ("merge", "Choose One Combined PDF."),
            ("split", "Choose Individual Pages or Page Ranges."),
            ("rotate", "Choose a Rotation Type."),
            ("compress", "Choose Smallest, Standard, or Largest."),
            ("ocr", "Choose Searchable PDF."),
        ] {
            assert_eq!(
                run_task_sync(request(task, "invalid", std::slice::from_ref(&pdf), ""))
                    .unwrap_err(),
                expected,
                "task: {task}"
            );
        }

        for (task, output) in [
            ("extract", "text"),
            ("merge", "merged"),
            ("split", "individual"),
            ("rotate", "clockwise"),
            ("compress", "standard"),
            ("ocr", "searchable"),
        ] {
            assert_eq!(
                run_task_sync(request(task, output, std::slice::from_ref(&text), "")).unwrap_err(),
                "This Function Accepts PDF Files Only.",
                "task: {task}"
            );
        }
        assert_eq!(
            run_task_sync(request("merge", "merged", std::slice::from_ref(&pdf), "")).unwrap_err(),
            "Select at Least Two PDFs to Merge."
        );
        assert_eq!(
            run_task_sync(request("split", "individual", &[pdf, second_pdf], "")).unwrap_err(),
            "Only One PDF Can Be Split at a Time. Please Select One PDF."
        );
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn native_backend_rejects_splitting_a_one_page_pdf() {
        let folder = test_dir("one-page-split");
        let pdf = test_pdf(&folder, "One Page", 1);

        for (output, detail) in [("individual", ""), ("ranges", "1")] {
            assert_eq!(
                run_task_sync(request("split", output, std::slice::from_ref(&pdf), detail))
                    .unwrap_err(),
                "This PDF has only one page and cannot be split."
            );
        }
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn native_backend_rejects_corrupt_and_zero_page_pdf_boundaries() {
        let folder = test_dir("invalid-pdf-content");
        let corrupt = folder.join("Corrupt.pdf");
        fs::write(&corrupt, b"This is not a PDF.").unwrap();

        for (task, output, detail) in [
            ("to-pdf", "separate", ""),
            ("extract", "text", ""),
            ("merge", "merged", ""),
            ("split", "individual", ""),
            ("rotate", "clockwise", "all"),
            ("compress", "standard", ""),
            ("ocr", "searchable", ""),
        ] {
            assert!(
                run_task_sync(request(
                    task,
                    output,
                    std::slice::from_ref(&corrupt),
                    detail
                ))
                .is_err(),
                "{task} accepted a corrupt PDF"
            );
        }

        let empty = empty_test_pdf(&folder, "Empty");
        assert!(
            run_task_sync(request(
                "split",
                "individual",
                std::slice::from_ref(&empty),
                ""
            ))
            .is_err(),
            "Split accepted a zero-page PDF without creating an output"
        );

        let valid = test_pdf(&folder, "Detail Validation", 3);
        for detail in ["0", "01", "-1", "4", "1,2", "3-1", "abc", "4294967296"] {
            assert!(
                run_task_sync(request(
                    "rotate",
                    "clockwise",
                    std::slice::from_ref(&valid),
                    detail
                ))
                .is_err(),
                "Rotate accepted invalid detail {detail:?}"
            );
        }
        for detail in [
            "",
            "0",
            "01",
            "-1",
            "3-1",
            "1,,2",
            "1-3, 3",
            "abc",
            "4294967296",
        ] {
            assert!(
                run_task_sync(request(
                    "split",
                    "ranges",
                    std::slice::from_ref(&valid),
                    detail
                ))
                .is_err(),
                "Split accepted invalid detail {detail:?}"
            );
        }

        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn markdown_extraction_keeps_figure_references() {
        let markdown = plain_to_markdown("Overview\n\nFigure 2: Revenue by Region\n\nBody text.");
        assert!(markdown.contains("## Overview"));
        assert!(markdown.contains("**Figure 2: Revenue by Region**"));
    }

    #[test]
    fn markdown_extraction_formats_layout_tables() {
        let markdown = plain_to_markdown(
            "Region      Revenue      Growth\nNorth       120          12%\nSouth       95           8%\n\nFigure 3: Regional results",
        );
        assert!(markdown.contains("| Region | Revenue | Growth |"));
        assert!(markdown.contains("| --- | --- | --- |"));
        assert!(markdown.contains("| North | 120 | 12% |"));
        assert!(markdown.contains("**Figure 3: Regional results**"));
    }

    #[test]
    fn out_of_bounds_ranges_name_only_missing_pages() {
        let ranges = parse_ranges("1-5, 6-9, 11-14").unwrap();
        let invalid = ranges
            .iter()
            .filter(|(_, end, _)| *end > 7)
            .map(|(start, end, _)| {
                let missing_start = (*start).max(8);
                if missing_start == *end {
                    missing_start.to_string()
                } else {
                    format!("{missing_start}-{end}")
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(invalid, vec!["8-9", "11-14"]);
    }

    #[test]
    fn native_pdf_engines_complete_every_pdf_function() {
        let folder = test_dir("native-engines");
        let first = test_pdf(&folder, "First", 3);
        let second = test_pdf(&folder, "Second", 2);

        let extracted =
            run_task_sync(request("extract", "both", std::slice::from_ref(&first), "")).unwrap();
        assert_eq!(extracted.outputs.len(), 2);

        let merged = run_task_sync(request(
            "merge",
            "merged",
            &[first.clone(), second.clone()],
            "",
        ))
        .unwrap();
        assert_eq!(merged.outputs.len(), 1);
        assert_eq!(
            pdf_page_count(Path::new(&merged.outputs[0].path)).unwrap(),
            5
        );

        let split = run_task_sync(request(
            "split",
            "ranges",
            std::slice::from_ref(&first),
            "1-2, 3",
        ))
        .unwrap();
        assert_eq!(split.outputs.len(), 2);
        assert_eq!(
            pdf_page_count(Path::new(&split.outputs[0].path)).unwrap(),
            2
        );
        assert_eq!(
            pdf_page_count(Path::new(&split.outputs[1].path)).unwrap(),
            1
        );

        let split_pages = run_task_sync(request(
            "split",
            "individual",
            std::slice::from_ref(&first),
            "",
        ))
        .unwrap();
        assert_eq!(split_pages.outputs.len(), 3);

        let invalid = run_task_sync(request(
            "split",
            "ranges",
            std::slice::from_ref(&first),
            "1-2, 4-6",
        ))
        .unwrap_err();
        assert!(invalid.contains("First.pdf has 3 pages. Pages 4-6 do not exist."));

        for rotation in ["clockwise", "counterclockwise", "half-turn"] {
            let rotated = run_task_sync(request(
                "rotate",
                rotation,
                std::slice::from_ref(&first),
                "1-2",
            ))
            .unwrap();
            assert_eq!(rotated.outputs.len(), 1);
            assert_eq!(
                pdf_page_count(Path::new(&rotated.outputs[0].path)).unwrap(),
                3
            );
        }

        for preset in ["screen", "standard", "print"] {
            let compressed = run_task_sync(request(
                "compress",
                preset,
                std::slice::from_ref(&first),
                "",
            ))
            .unwrap();
            assert_eq!(compressed.outputs.len(), 1);
            let compressed_path = Path::new(&compressed.outputs[0].path);
            assert_eq!(pdf_page_count(compressed_path).unwrap(), 3);
            let extracted_text = folder.join(format!("compressed-{preset}.txt"));
            native_extract_text(compressed_path, &extracted_text).unwrap();
            assert!(!fs::read_to_string(extracted_text)
                .unwrap()
                .trim()
                .is_empty());
        }

        let searchable = run_task_sync(request(
            "ocr",
            "searchable",
            std::slice::from_ref(&first),
            "",
        ))
        .unwrap();
        assert_eq!(searchable.outputs.len(), 1);
        assert_eq!(
            pdf_page_count(Path::new(&searchable.outputs[0].path)).unwrap(),
            3
        );

        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn native_rotate_handles_every_page_mode_and_rejects_missing_pages() {
        let folder = test_dir("rotate-page-modes");
        let source = test_pdf(&folder, "Rotate Modes", 3);

        for detail in ["all", "", "2", "1-2"] {
            let rotated = run_task_sync(request(
                "rotate",
                "clockwise",
                std::slice::from_ref(&source),
                detail,
            ))
            .unwrap();
            assert_eq!(rotated.outputs.len(), 1, "detail: {detail:?}");
            assert_eq!(
                pdf_page_count(Path::new(&rotated.outputs[0].path)).unwrap(),
                3,
                "detail: {detail:?}"
            );
        }

        let invalid = run_task_sync(request(
            "rotate",
            "clockwise",
            std::slice::from_ref(&source),
            "4",
        ))
        .unwrap_err();
        assert!(invalid.contains("Rotate Modes.pdf has 3 pages. Pages 4 do not exist."));

        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn native_document_conversion_save_and_cleanup_work() {
        let folder = test_dir("document-engines");
        let text = folder.join("Notes.txt");
        let html = folder.join("Page.html");
        let markdown = folder.join("Readme.md");
        let rtf = folder.join("Letter.rtf");
        fs::write(&text, "Quire text document.\n").unwrap();
        fs::write(
            &html,
            "<!doctype html><html><body><h1>Quire HTML</h1></body></html>",
        )
        .unwrap();
        fs::write(&markdown, "# Quire Markdown\n\nA conversion test.\n").unwrap();
        fs::write(&rtf, "{\\rtf1\\ansi Quire RTF document.}").unwrap();
        let doc = office_fixture(&folder, "doc");
        let docx = office_fixture(&folder, "docx");
        let odt = office_fixture(&folder, "odt");
        let sources = vec![text, html, markdown, rtf, doc, docx, odt];

        let separate = run_task_sync(request("to-pdf", "separate", &sources, "")).unwrap();
        assert_eq!(separate.outputs.len(), sources.len());
        assert!(separate
            .outputs
            .iter()
            .all(|output| Path::new(&output.path).is_file()));

        let combined = run_task_sync(request("to-pdf", "combined", &sources, "")).unwrap();
        assert_eq!(combined.outputs.len(), 1);
        assert!(
            pdf_page_count(Path::new(&combined.outputs[0].path)).unwrap() >= sources.len() as u32
        );

        let save_folder = folder.join("saved");
        fs::create_dir_all(&save_folder).unwrap();
        let saved = tauri::async_runtime::block_on(save_outputs(
            combined
                .outputs
                .iter()
                .map(|output| output.path.clone())
                .collect(),
            path_string(&save_folder),
            false,
        ))
        .unwrap();
        assert_eq!(saved.saved.len(), 1);
        assert!(Path::new(&saved.saved[0]).is_file());

        let staged = separate
            .outputs
            .iter()
            .map(|output| output.path.clone())
            .collect::<Vec<_>>();
        let failures = tauri::async_runtime::block_on(discard_outputs(staged.clone())).unwrap();
        assert!(failures.is_empty());
        assert!(staged.iter().all(|path| !Path::new(path).exists()));

        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn save_refuses_existing_names_without_overwriting() {
        let folder = test_dir("save-collision");
        let source = test_pdf(&folder, "Collision", 1);
        let converted = run_task_sync(request(
            "compress",
            "standard",
            std::slice::from_ref(&source),
            "",
        ))
        .unwrap();
        let destination = folder.join("saved");
        fs::create_dir_all(&destination).unwrap();
        let existing = destination.join(&converted.outputs[0].name);
        fs::write(&existing, b"original destination contents").unwrap();
        let result = tauri::async_runtime::block_on(save_outputs(
            vec![converted.outputs[0].path.clone()],
            path_string(&destination),
            false,
        ));
        assert!(result.unwrap_err().starts_with("FILE_EXISTS::"));
        assert_eq!(
            fs::read(&existing).unwrap(),
            b"original destination contents"
        );

        let renamed = destination.join("Renamed.pdf");
        let renamed_result = tauri::async_runtime::block_on(save_outputs(
            vec![converted.outputs[0].path.clone()],
            path_string(&renamed),
            false,
        ))
        .unwrap();
        assert_eq!(
            PathBuf::from(&renamed_result.saved[0])
                .canonicalize()
                .unwrap(),
            renamed.canonicalize().unwrap()
        );
        assert_eq!(pdf_page_count(&renamed).unwrap(), 1);

        let overwritten = tauri::async_runtime::block_on(save_outputs(
            vec![converted.outputs[0].path.clone()],
            path_string(&existing),
            true,
        ))
        .unwrap();
        assert_eq!(
            PathBuf::from(&overwritten.saved[0]).canonicalize().unwrap(),
            existing.canonicalize().unwrap()
        );
        assert_eq!(pdf_page_count(&existing).unwrap(), 1);
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn save_can_overwrite_multiple_existing_outputs_as_one_transaction() {
        let folder = test_dir("save-multiple-overwrite");
        let first = test_pdf(&folder, "First", 1);
        let second = test_pdf(&folder, "Second", 2);
        let converted =
            run_task_sync(request("compress", "standard", &[first, second], "")).unwrap();
        let destination = folder.join("saved");
        fs::create_dir_all(&destination).unwrap();
        let paths = converted
            .outputs
            .iter()
            .map(|output| output.path.clone())
            .collect::<Vec<_>>();
        tauri::async_runtime::block_on(save_outputs(
            paths.clone(),
            path_string(&destination),
            false,
        ))
        .unwrap();

        let collision = tauri::async_runtime::block_on(save_outputs(
            paths.clone(),
            path_string(&destination),
            false,
        ));
        assert!(collision.unwrap_err().starts_with("FILE_EXISTS::"));

        let overwritten =
            tauri::async_runtime::block_on(save_outputs(paths, path_string(&destination), true))
                .unwrap();
        assert_eq!(overwritten.saved.len(), 2);
        assert_eq!(pdf_page_count(Path::new(&overwritten.saved[0])).unwrap(), 1);
        assert_eq!(pdf_page_count(Path::new(&overwritten.saved[1])).unwrap(), 2);
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn save_rejects_relative_destinations() {
        let result = tauri::async_runtime::block_on(save_outputs(
            Vec::new(),
            "relative/folder".to_string(),
            false,
        ));
        assert_eq!(result.unwrap_err(), "Type an Absolute Save Path.");
    }

    #[test]
    fn save_and_discard_reject_missing_external_duplicate_and_wrong_destination_cases() {
        let folder = test_dir("save-negative-cases");
        assert_eq!(
            tauri::async_runtime::block_on(save_outputs(Vec::new(), path_string(&folder), false,))
                .unwrap_err(),
            "There Are No Output Files to Save."
        );
        assert_eq!(
            tauri::async_runtime::block_on(save_outputs(
                vec![path_string(&folder.join("Missing.pdf"))],
                path_string(&folder),
                false,
            ))
            .unwrap_err(),
            "An Output File Could Not Be Found."
        );

        let source = test_pdf(&folder, "Source", 1);
        assert_eq!(
            tauri::async_runtime::block_on(save_outputs(
                vec![path_string(&source)],
                path_string(&folder),
                false,
            ))
            .unwrap_err(),
            "Quire Refused to Save a File Outside Its Temporary Folder."
        );

        let first = run_task_sync(request(
            "compress",
            "standard",
            std::slice::from_ref(&source),
            "",
        ))
        .unwrap();
        let second = run_task_sync(request(
            "compress",
            "standard",
            std::slice::from_ref(&source),
            "",
        ))
        .unwrap();
        let other_source = test_pdf(&folder, "Other", 1);
        let third = run_task_sync(request(
            "compress",
            "standard",
            std::slice::from_ref(&other_source),
            "",
        ))
        .unwrap();
        let staged = first.outputs[0].path.clone();
        assert_eq!(
            tauri::async_runtime::block_on(save_outputs(
                vec![staged.clone()],
                path_string(&folder.join("missing-parent").join("Output.pdf")),
                false,
            ))
            .unwrap_err(),
            "Select an existing folder for the file."
        );

        let destination_file = folder.join("destination-file");
        fs::write(&destination_file, "not a folder").unwrap();
        assert_eq!(
            tauri::async_runtime::block_on(save_outputs(
                vec![
                    first.outputs[0].path.clone(),
                    second.outputs[0].path.clone()
                ],
                path_string(&folder),
                false,
            ))
            .unwrap_err(),
            "More Than One Output File Is Named 'Source-compressed.pdf'."
        );
        assert_eq!(
            tauri::async_runtime::block_on(save_outputs(
                vec![first.outputs[0].path.clone(), third.outputs[0].path.clone()],
                path_string(&destination_file),
                false,
            ))
            .unwrap_err(),
            "Select an existing destination folder for multiple files."
        );

        let outside_failures =
            tauri::async_runtime::block_on(discard_outputs(vec![path_string(&source)])).unwrap();
        assert_eq!(outside_failures, vec![path_string(&source)]);
        let _ = tauri::async_runtime::block_on(discard_outputs(vec![
            first.outputs[0].path.clone(),
            second.outputs[0].path.clone(),
            third.outputs[0].path.clone(),
        ]));
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn cancellation_stops_a_running_native_process() {
        let run_id = format!(
            "cancel-test-{}",
            RUN_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        CURRENT_RUN_ID.with(|value| *value.borrow_mut() = run_id.clone());
        let cancellation_id = run_id.clone();
        let canceller = thread::spawn(move || {
            thread::sleep(Duration::from_millis(150));
            cancelled_runs().lock().unwrap().insert(cancellation_id);
        });
        let started = std::time::Instant::now();
        let result = run_command(
            Path::new("/bin/sleep"),
            &[OsString::from("5")],
            "Sleep Test Failed",
        );
        canceller.join().unwrap();
        CURRENT_RUN_ID.with(|value| value.borrow_mut().clear());
        cancelled_runs().lock().unwrap().remove(&run_id);
        assert_eq!(result.unwrap_err(), "Conversion Cancelled.");
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
