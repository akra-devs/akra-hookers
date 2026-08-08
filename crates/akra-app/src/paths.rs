use std::{
    env,
    path::{Path, PathBuf},
};

pub fn default_data_dir() -> PathBuf {
    if let Some(path) = env::var_os("AKRA_HOOKERS_DATA_DIR") {
        return PathBuf::from(path);
    }

    if let Some(path) = env::var_os("LOCALAPPDATA") {
        return PathBuf::from(path).join("akra-hookers");
    }

    if let Some(path) = env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(path).join("akra-hookers");
    }

    user_home()
        .join(".local")
        .join("share")
        .join("akra-hookers")
}

pub fn user_home() -> PathBuf {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn hook_command(executable: &Path, data_dir: &Path) -> String {
    format!(
        "\"{}\" capture --data-dir \"{}\"",
        executable.display(),
        data_dir.display()
    )
}
