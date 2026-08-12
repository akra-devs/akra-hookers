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
    hook_command_with_target(executable, data_dir, None)
}

#[cfg(windows)]
pub fn hook_command_for_target(
    executable: &Path,
    data_dir: &Path,
    capture_target: &str,
) -> Result<String, HookCommandError> {
    validate_capture_target(capture_target)?;
    hook_command_with_target(executable, data_dir, Some(capture_target))
}

#[cfg(windows)]
fn hook_command_with_target(
    executable: &Path,
    data_dir: &Path,
    capture_target: Option<&str>,
) -> Result<String, HookCommandError> {
    let powershell = windows_powershell()?;
    let encoded = encoded_capture_script(executable, data_dir, None, capture_target)?;
    Ok(format!(
        "{} -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass \
         -EncodedCommand {}",
        windows_executable_argument(&powershell)?,
        encoded
    ))
}

#[cfg(windows)]
pub fn wsl_hook_command(
    executable: &Path,
    data_dir: &Path,
    distro: &str,
) -> Result<String, HookCommandError> {
    wsl_hook_command_with_target(executable, data_dir, distro, None)
}

#[cfg(windows)]
pub fn wsl_hook_command_for_target(
    executable: &Path,
    data_dir: &Path,
    distro: &str,
    capture_target: &str,
) -> Result<String, HookCommandError> {
    validate_capture_target(capture_target)?;
    wsl_hook_command_with_target(executable, data_dir, distro, Some(capture_target))
}

#[cfg(windows)]
fn wsl_hook_command_with_target(
    executable: &Path,
    data_dir: &Path,
    distro: &str,
    capture_target: Option<&str>,
) -> Result<String, HookCommandError> {
    validate_wsl_distro(distro)?;
    let mut command = format!(
        "{} capture --data-dir {}",
        posix_shell_text_argument(&windows_path_to_wsl(executable)?),
        posix_shell_text_argument(path_text(data_dir)?)
    );
    if let Some(capture_target) = capture_target {
        command.push_str(" --capture-target ");
        command.push_str(&posix_shell_text_argument(capture_target));
    }
    command.push_str(" --wsl-distro ");
    command.push_str(&posix_shell_text_argument(distro));
    Ok(command)
}

#[cfg(windows)]
pub fn shared_wsl_hook_command(
    executable: &Path,
    data_dir: &Path,
) -> Result<String, HookCommandError> {
    shared_wsl_hook_command_with_target(executable, data_dir, None)
}

#[cfg(windows)]
pub fn shared_wsl_hook_command_for_target(
    executable: &Path,
    data_dir: &Path,
    capture_target: &str,
) -> Result<String, HookCommandError> {
    validate_capture_target(capture_target)?;
    shared_wsl_hook_command_with_target(executable, data_dir, Some(capture_target))
}

#[cfg(windows)]
fn shared_wsl_hook_command_with_target(
    executable: &Path,
    data_dir: &Path,
    capture_target: Option<&str>,
) -> Result<String, HookCommandError> {
    let mut command = format!(
        "{} capture --data-dir {}",
        posix_shell_text_argument(&windows_path_to_wsl(executable)?),
        posix_shell_text_argument(path_text(data_dir)?)
    );
    if let Some(capture_target) = capture_target {
        command.push_str(" --capture-target ");
        command.push_str(&posix_shell_text_argument(capture_target));
    }
    command.push_str(" --wsl-distro \"${WSL_DISTRO_NAME:?WSL_DISTRO_NAME is unavailable}\"");
    Ok(command)
}

#[cfg(not(windows))]
pub fn hook_command(executable: &Path, data_dir: &Path) -> Result<String, HookCommandError> {
    hook_command_with_target(executable, data_dir, None)
}

#[cfg(not(windows))]
pub fn hook_command_for_target(
    executable: &Path,
    data_dir: &Path,
    capture_target: &str,
) -> Result<String, HookCommandError> {
    validate_capture_target(capture_target)?;
    hook_command_with_target(executable, data_dir, Some(capture_target))
}

#[cfg(not(windows))]
fn hook_command_with_target(
    executable: &Path,
    data_dir: &Path,
    capture_target: Option<&str>,
) -> Result<String, HookCommandError> {
    let mut command = format!(
        "{} capture --data-dir {}",
        posix_shell_argument(executable)?,
        posix_shell_argument(data_dir)?
    );
    if let Some(capture_target) = capture_target {
        command.push_str(" --capture-target ");
        command.push_str(&posix_shell_text_argument(capture_target));
    }
    Ok(command)
}

#[cfg(windows)]
fn powershell_literal(path: &Path) -> Result<String, HookCommandError> {
    Ok(powershell_text_literal(path_text(path)?))
}

#[cfg(windows)]
fn powershell_text_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(windows)]
fn encoded_capture_script(
    executable: &Path,
    data_dir: &Path,
    wsl_distro: Option<&str>,
    capture_target: Option<&str>,
) -> Result<String, HookCommandError> {
    let mut script = format!(
        "& {} capture --data-dir {}",
        powershell_literal(executable)?,
        powershell_literal(data_dir)?
    );
    if let Some(capture_target) = capture_target {
        script.push_str(" --capture-target ");
        script.push_str(&powershell_text_literal(capture_target));
    }
    if let Some(distro) = wsl_distro {
        script.push_str(" --wsl-distro ");
        script.push_str(&powershell_text_literal(distro));
    }
    let bytes = script
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    Ok(STANDARD.encode(bytes))
}

fn validate_capture_target(target: &str) -> Result<(), HookCommandError> {
    if target.is_empty()
        || target.len() > 128
        || target.chars().any(|character| {
            !character.is_ascii_alphanumeric() && !matches!(character, '.' | '_' | '-' | ':')
        })
    {
        return Err(HookCommandError::InvalidCaptureTarget(target.to_owned()));
    }
    Ok(())
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
    Ok(posix_shell_text_argument(path_text(path)?))
}

fn posix_shell_text_argument(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[cfg(windows)]
fn windows_path_to_wsl(path: &Path) -> Result<String, HookCommandError> {
    let value = path_text(path)?;
    let bytes = value.as_bytes();
    if bytes.len() < 3 || bytes[1] != b':' || !matches!(bytes[2], b'\\' | b'/') {
        return Err(HookCommandError::UnsupportedWslPath(path.to_path_buf()));
    }
    let drive = char::from(bytes[0]).to_ascii_lowercase();
    if !drive.is_ascii_alphabetic() {
        return Err(HookCommandError::UnsupportedWslPath(path.to_path_buf()));
    }
    let remainder = value[3..].replace('\\', "/");
    Ok(format!("/mnt/{drive}/{remainder}"))
}

#[cfg(windows)]
pub fn wsl_cwd_to_windows(distro: &str, cwd: &str) -> Result<PathBuf, HookCommandError> {
    validate_wsl_distro(distro)?;
    if let Some(mounted) = cwd.strip_prefix("/mnt/") {
        let mut parts = mounted.splitn(2, '/');
        let drive = parts.next().unwrap_or_default();
        if drive.len() == 1 && drive.as_bytes()[0].is_ascii_alphabetic() {
            let remainder = parts.next().unwrap_or_default().replace('/', "\\");
            return Ok(PathBuf::from(format!(
                "{}:\\{}",
                drive.to_ascii_uppercase(),
                remainder
            )));
        }
    }
    if cwd.starts_with('/') {
        let remainder = cwd.trim_start_matches('/').replace('/', "\\");
        return Ok(PathBuf::from(format!(
            r"\\wsl.localhost\{distro}\{remainder}"
        )));
    }
    Err(HookCommandError::InvalidWslCwd(cwd.to_owned()))
}

#[cfg(windows)]
fn validate_wsl_distro(distro: &str) -> Result<(), HookCommandError> {
    if distro.is_empty()
        || distro.chars().any(|character| {
            !character.is_ascii_alphanumeric() && !matches!(character, '.' | '_' | '-')
        })
    {
        return Err(HookCommandError::InvalidWslDistro(distro.to_owned()));
    }
    Ok(())
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
    #[error("Windows path cannot be represented inside WSL: {0}")]
    UnsupportedWslPath(PathBuf),
    #[error("invalid WSL distribution name: {0}")]
    InvalidWslDistro(String),
    #[error("WSL working directory is not an absolute Linux path: {0}")]
    InvalidWslCwd(String),
    #[error("invalid capture target identifier: {0}")]
    InvalidCaptureTarget(String),
}

#[cfg(all(test, windows))]
mod tests {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use std::path::Path;

    use super::{
        HookCommandError, hook_command, hook_command_for_target, shared_wsl_hook_command,
        wsl_cwd_to_windows, wsl_hook_command, wsl_hook_command_for_target,
    };

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

    #[test]
    fn managed_hook_command_carries_a_literal_capture_target() {
        let command = hook_command_for_target(
            Path::new(r"C:\tools\akra-hookers.exe"),
            Path::new(r"C:\state"),
            "windows-native",
        )
        .expect("command");
        let encoded = command.split_whitespace().last().expect("encoded command");
        let bytes = STANDARD.decode(encoded).expect("base64");
        let utf16 = bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        assert_eq!(
            String::from_utf16(&utf16).expect("PowerShell"),
            r"& 'C:\tools\akra-hookers.exe' capture --data-dir 'C:\state' --capture-target 'windows-native'"
        );
        assert!(matches!(
            hook_command_for_target(
                Path::new(r"C:\tools\akra-hookers.exe"),
                Path::new(r"C:\state"),
                "unsafe target",
            ),
            Err(HookCommandError::InvalidCaptureTarget(_))
        ));
    }

    #[test]
    fn wsl_hook_command_calls_the_windows_capture_binary_through_interop() {
        let command = wsl_hook_command(
            Path::new(r"C:\tools\akra-hookers.exe"),
            Path::new(r"C:\state"),
            "Ubuntu-24.04",
        )
        .expect("WSL command");
        assert_eq!(
            command,
            r"'/mnt/c/tools/akra-hookers.exe' capture --data-dir 'C:\state' --wsl-distro 'Ubuntu-24.04'"
        );
    }

    #[test]
    fn wsl_managed_hook_command_carries_its_installation_id() {
        let command = wsl_hook_command_for_target(
            Path::new(r"C:\tools\akra-hookers.exe"),
            Path::new(r"C:\state"),
            "Ubuntu-24.04",
            "wsl:Ubuntu-24.04",
        )
        .expect("WSL command");
        assert_eq!(
            command,
            r"'/mnt/c/tools/akra-hookers.exe' capture --data-dir 'C:\state' --capture-target 'wsl:Ubuntu-24.04' --wsl-distro 'Ubuntu-24.04'"
        );
    }

    #[test]
    fn shared_wsl_hook_command_uses_the_runtime_distro_without_shell_injection() {
        let command = shared_wsl_hook_command(
            Path::new(r"C:\tools & apps\akra-hookers.exe"),
            Path::new(r"C:\state&$()`'"),
        )
        .expect("shared WSL command");

        assert_eq!(
            command,
            r#"'/mnt/c/tools & apps/akra-hookers.exe' capture --data-dir 'C:\state&$()`'"'"'' --wsl-distro "${WSL_DISTRO_NAME:?WSL_DISTRO_NAME is unavailable}""#
        );
    }

    #[test]
    fn wsl_working_directories_map_to_windows_accessible_paths() {
        assert_eq!(
            wsl_cwd_to_windows("Ubuntu", "/mnt/c/dev/project").expect("mounted path"),
            Path::new(r"C:\dev\project")
        );
        assert_eq!(
            wsl_cwd_to_windows("Ubuntu", "/home/akra/project").expect("distro path"),
            Path::new(r"\\wsl.localhost\Ubuntu\home\akra\project")
        );
    }
}
