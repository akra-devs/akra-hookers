use std::{
    env,
    path::{Path, PathBuf},
};

#[cfg(windows)]
use base64::{Engine as _, engine::general_purpose::STANDARD};
use thiserror::Error;

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

pub fn codex_home() -> PathBuf {
    if let Some(path) = env::var_os("CODEX_HOME") {
        return PathBuf::from(path);
    }

    user_home().join(".codex")
}

pub fn user_home() -> PathBuf {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(windows)]
pub fn hook_command(executable: &Path, data_dir: &Path) -> Result<String, HookCommandError> {
    let powershell = windows_powershell()?;
    let script = format!(
        "& {} capture --data-dir {}",
        powershell_literal(executable)?,
        powershell_literal(data_dir)?
    );
    let bytes = script
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    Ok(format!(
        "{} -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass \
         -EncodedCommand {}",
        windows_executable_argument(&powershell)?,
        STANDARD.encode(bytes)
    ))
}

#[cfg(not(windows))]
pub fn hook_command(executable: &Path, data_dir: &Path) -> Result<String, HookCommandError> {
    Ok(format!(
        "{} capture --data-dir {}",
        posix_shell_argument(executable)?,
        posix_shell_argument(data_dir)?
    ))
}

#[cfg(windows)]
fn powershell_literal(path: &Path) -> Result<String, HookCommandError> {
    Ok(format!("'{}'", path_text(path)?.replace('\'', "''")))
}

#[cfg(windows)]
fn windows_powershell() -> Result<PathBuf, HookCommandError> {
    let system_root = env::var_os("SystemRoot").ok_or(HookCommandError::MissingSystemRoot)?;
    let executable = PathBuf::from(system_root)
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    if !executable.is_absolute()
        || !executable
            .metadata()
            .is_ok_and(|metadata| metadata.is_file())
    {
        return Err(HookCommandError::UnsafePowerShell(executable));
    }
    Ok(executable)
}

#[cfg(windows)]
fn windows_executable_argument(path: &Path) -> Result<String, HookCommandError> {
    let value = path_text(path)?;
    if value.chars().any(|character| {
        character.is_whitespace()
            || matches!(
                character,
                '"' | '%' | '!' | '&' | '|' | '<' | '>' | '(' | ')' | '^'
            )
    }) {
        return Err(HookCommandError::UnsafePowerShell(path.to_path_buf()));
    }
    Ok(value.to_owned())
}

#[cfg(not(windows))]
fn posix_shell_argument(path: &Path) -> Result<String, HookCommandError> {
    Ok(format!("'{}'", path_text(path)?.replace('\'', "'\"'\"'")))
}

fn path_text(path: &Path) -> Result<&str, HookCommandError> {
    path.to_str()
        .ok_or_else(|| HookCommandError::NonUnicodePath {
            path: path.to_path_buf(),
        })
}

#[derive(Debug, Error)]
pub enum HookCommandError {
    #[error("hook command path is not valid Unicode: {path}")]
    NonUnicodePath { path: PathBuf },
    #[error("SystemRoot is unavailable; refusing to search for PowerShell")]
    MissingSystemRoot,
    #[error("trusted Windows PowerShell executable is unavailable: {0}")]
    UnsafePowerShell(PathBuf),
}

#[cfg(all(test, windows))]
mod tests {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use std::path::Path;

    use super::hook_command;

    #[test]
    fn hook_command_encodes_shell_metacharacters_as_literal_arguments() {
        let command = hook_command(
            Path::new(r"C:\tools & apps\akra-hookers.exe"),
            Path::new(r"C:\state&$()`'%TEMP%"),
        )
        .expect("command");
        let system_root = std::env::var("SystemRoot").expect("SystemRoot");
        assert!(command.starts_with(&format!(
            "{system_root}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe "
        )));
        let encoded = command.split_whitespace().last().expect("encoded command");
        let bytes = STANDARD.decode(encoded).expect("base64");
        let utf16 = bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        assert_eq!(
            String::from_utf16(&utf16).expect("PowerShell"),
            r"& 'C:\tools & apps\akra-hookers.exe' capture --data-dir 'C:\state&$()`''%TEMP%'"
        );
    }
}
