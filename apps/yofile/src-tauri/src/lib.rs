use std::path::PathBuf;
use std::process::Command;

// Each tool is a full macOS app bundled inside YoFile.app/Contents/Resources/apps.
// The launcher simply opens the matching bundle; the tools keep their own code as-is.
fn bundled_app(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    // .../YoFile.app/Contents/MacOS/yofile  ->  .../YoFile.app/Contents/Resources/apps/<name>.app
    let contents = exe.parent()?.parent()?;
    let candidate = contents.join("Resources").join("apps").join(name);
    if candidate.exists() {
        Some(candidate)
    } else {
        None
    }
}

fn dev_app(name: &str) -> Option<PathBuf> {
    // Fallback for `tauri dev`: use the app already installed in /Applications.
    let candidate = PathBuf::from("/Applications").join(name);
    if candidate.exists() {
        Some(candidate)
    } else {
        None
    }
}

#[tauri::command]
fn launch_tool(tool: String) -> Result<(), String> {
    let bundle = match tool.as_str() {
        "convert" => "Kiln.app",
        "rename" => "Rollcall.app",
        "pdf" => "Quire.app",
        other => return Err(format!("Unknown tool: {other}")),
    };

    let target = bundled_app(bundle)
        .or_else(|| dev_app(bundle))
        .ok_or_else(|| format!("{bundle} is not available in this build."))?;

    Command::new("open")
        .arg("-n")
        .arg(&target)
        .spawn()
        .map_err(|error| format!("Could not open {bundle}: {error}"))?;
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![launch_tool])
        .run(tauri::generate_context!())
        .expect("error while running YoFile");
}
