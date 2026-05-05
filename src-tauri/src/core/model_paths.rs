use std::fs;
use std::io;
use std::path::PathBuf;

const APP_MODEL_ROOT: &str = "WisprWin/models";

pub fn ensure_model_root_dir() -> Result<PathBuf, String> {
    let model_dir = resolve_model_root_dir()?;
    fs::create_dir_all(&model_dir)
        .map_err(|e| map_fs_error("create model directory", &model_dir, &e))?;
    Ok(model_dir)
}

fn resolve_model_root_dir() -> Result<PathBuf, String> {
    let appdata = std::env::var_os("APPDATA").ok_or_else(|| {
        "Failed to resolve model directory: %AppData% is not set. Please verify your Windows user profile and retry."
            .to_string()
    })?;

    if appdata.is_empty() {
        return Err(
            "Failed to resolve model directory: %AppData% is empty. Please verify your Windows profile and retry."
                .to_string(),
        );
    }

    Ok(PathBuf::from(appdata).join(APP_MODEL_ROOT))
}

pub fn map_fs_error(action: &str, path: &std::path::Path, error: &io::Error) -> String {
    let path_display = path.display();
    match error.kind() {
        io::ErrorKind::PermissionDenied => format!(
            "Cannot {} at '{}': permission denied. Try running with a user account that can access %AppData% or adjust folder permissions.",
            action, path_display
        ),
        io::ErrorKind::NotFound => format!(
            "Cannot {} at '{}': path not found. Verify that %AppData% resolves to a valid directory.",
            action, path_display
        ),
        io::ErrorKind::InvalidInput => format!(
            "Cannot {} at '{}': invalid path. Verify your %AppData% environment configuration.",
            action, path_display
        ),
        _ => format!("Failed to {} at '{}': {}", action, path_display, error),
    }
}
