use std::{
    env,
    path::{Path, PathBuf},
};

#[cfg(windows)]
use base64::{Engine as _, engine::general_purpose::STANDARD};
use thiserror::Error;

pub fn default_data_dir() -> PathBuf {
    resolve_default_data_dir(
        current_data_platform(),
        environment_path("AKRA_HOOKERS_DATA_DIR"),
        environment_path("LOCALAPPDATA"),
        environment_path("XDG_DATA_HOME"),
        user_home(),
    )
}

pub fn codex_home() -> PathBuf {
    if let Some(path) = env::var_os("CODEX_HOME") {
        return PathBuf::from(path);
    }

    user_home().join(".codex")
}

pub fn user_home() -> PathBuf {
    resolve_user_home(
        current_data_platform(),
        environment_path("USERPROFILE"),
        environment_path("HOME"),
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DataPlatform {
    Windows,
    Linux,
    MacOs,
    Ios,
    Other,
}

const fn current_data_platform() -> DataPlatform {
    if cfg!(target_os = "windows") {
        DataPlatform::Windows
    } else if cfg!(target_os = "linux") {
        DataPlatform::Linux
    } else if cfg!(target_os = "macos") {
        DataPlatform::MacOs
    } else if cfg!(target_os = "ios") {
        DataPlatform::Ios
    } else {
        DataPlatform::Other
    }
}

fn environment_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn resolve_default_data_dir(
    platform: DataPlatform,
    configured: Option<PathBuf>,
    local_app_data: Option<PathBuf>,
    xdg_data_home: Option<PathBuf>,
    home: PathBuf,
) -> PathBuf {
    if let Some(configured) = configured {
        return configured;
    }

    let base = match platform {
        DataPlatform::Windows => {
            local_app_data.unwrap_or_else(|| home.join("AppData").join("Local"))
        }
        DataPlatform::Linux => xdg_data_home.unwrap_or_else(|| home.join(".local").join("share")),
        DataPlatform::MacOs | DataPlatform::Ios => home.join("Library").join("Application Support"),
        DataPlatform::Other => xdg_data_home.unwrap_or_else(|| home.join(".local").join("share")),
    };
    base.join("akra-hookers")
}

fn resolve_user_home(
    platform: DataPlatform,
    user_profile: Option<PathBuf>,
    home: Option<PathBuf>,
) -> PathBuf {
    match platform {
        DataPlatform::Windows => user_profile.or(home),
        _ => home.or(user_profile),
    }
    .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(windows)]
pub fn hook_command(executable: &Path, data_dir: &Path) -> Result<String, HookCommandError> {
    hook_command_with_target(executable, data_dir, None, None)
}

#[cfg(windows)]
pub fn hook_command_for_target(
    executable: &Path,
    data_dir: &Path,
    capture_target: &str,
) -> Result<String, HookCommandError> {
    validate_capture_target(capture_target)?;
    hook_command_with_target(executable, data_dir, Some(capture_target), None)
}

#[cfg(windows)]
pub fn hook_command_for_target_and_home(
    executable: &Path,
    data_dir: &Path,
    capture_target: &str,
    codex_home: &str,
) -> Result<String, HookCommandError> {
    validate_capture_target(capture_target)?;
    hook_command_with_target(executable, data_dir, Some(capture_target), Some(codex_home))
}

#[cfg(windows)]
fn hook_command_with_target(
    executable: &Path,
    data_dir: &Path,
    capture_target: Option<&str>,
    codex_home: Option<&str>,
) -> Result<String, HookCommandError> {
    let powershell = windows_powershell()?;
    let encoded = encoded_capture_script(executable, data_dir, None, capture_target, codex_home)?;
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
    wsl_hook_command_with_target(executable, data_dir, distro, None, None)
}

#[cfg(windows)]
pub fn wsl_hook_command_for_target(
    executable: &Path,
    data_dir: &Path,
    distro: &str,
    capture_target: &str,
) -> Result<String, HookCommandError> {
    validate_capture_target(capture_target)?;
    wsl_hook_command_with_target(executable, data_dir, distro, Some(capture_target), None)
}

#[cfg(windows)]
pub fn wsl_hook_command_for_target_and_home(
    executable: &Path,
    data_dir: &Path,
    distro: &str,
    capture_target: &str,
    codex_home: &str,
) -> Result<String, HookCommandError> {
    validate_capture_target(capture_target)?;
    wsl_hook_command_with_target(
        executable,
        data_dir,
        distro,
        Some(capture_target),
        Some(codex_home),
    )
}

#[cfg(windows)]
fn wsl_hook_command_with_target(
    executable: &Path,
    data_dir: &Path,
    distro: &str,
    capture_target: Option<&str>,
    codex_home: Option<&str>,
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
    if let Some(codex_home) = codex_home {
        command.push_str(" --codex-home ");
        command.push_str(&posix_shell_text_argument(codex_home));
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
    shared_wsl_hook_command_with_target(executable, data_dir, None, None)
}

#[cfg(windows)]
pub fn shared_wsl_hook_command_for_target(
    executable: &Path,
    data_dir: &Path,
    capture_target: &str,
) -> Result<String, HookCommandError> {
    validate_capture_target(capture_target)?;
    shared_wsl_hook_command_with_target(executable, data_dir, Some(capture_target), None)
}

#[cfg(windows)]
pub fn shared_wsl_hook_command_for_target_and_home(
    executable: &Path,
    data_dir: &Path,
    capture_target: &str,
    codex_home: &str,
) -> Result<String, HookCommandError> {
    validate_capture_target(capture_target)?;
    shared_wsl_hook_command_with_target(
        executable,
        data_dir,
        Some(capture_target),
        Some(codex_home),
    )
}

#[cfg(windows)]
fn shared_wsl_hook_command_with_target(
    executable: &Path,
    data_dir: &Path,
    capture_target: Option<&str>,
    codex_home: Option<&str>,
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
    if let Some(codex_home) = codex_home {
        command.push_str(" --codex-home ");
        command.push_str(&posix_shell_text_argument(codex_home));
    }
    command.push_str(" --wsl-distro \"${WSL_DISTRO_NAME:?WSL_DISTRO_NAME is unavailable}\"");
    Ok(command)
}

#[cfg(not(windows))]
pub fn hook_command(executable: &Path, data_dir: &Path) -> Result<String, HookCommandError> {
    hook_command_with_target(executable, data_dir, None, None)
}

#[cfg(not(windows))]
pub fn hook_command_for_target(
    executable: &Path,
    data_dir: &Path,
    capture_target: &str,
) -> Result<String, HookCommandError> {
    validate_capture_target(capture_target)?;
    hook_command_with_target(executable, data_dir, Some(capture_target), None)
}

#[cfg(not(windows))]
pub fn hook_command_for_target_and_home(
    executable: &Path,
    data_dir: &Path,
    capture_target: &str,
    codex_home: &str,
) -> Result<String, HookCommandError> {
    validate_capture_target(capture_target)?;
    hook_command_with_target(executable, data_dir, Some(capture_target), Some(codex_home))
}

#[cfg(not(windows))]
fn hook_command_with_target(
    executable: &Path,
    data_dir: &Path,
    capture_target: Option<&str>,
    codex_home: Option<&str>,
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
    if let Some(codex_home) = codex_home {
        command.push_str(" --codex-home ");
        command.push_str(&posix_shell_text_argument(codex_home));
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
    codex_home: Option<&str>,
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
    if let Some(codex_home) = codex_home {
        script.push_str(" --codex-home ");
        script.push_str(&powershell_text_literal(codex_home));
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

#[cfg(test)]
mod data_directory_tests {
    use std::path::PathBuf;

    use super::{DataPlatform, resolve_default_data_dir, resolve_user_home};

    #[test]
    fn explicit_directory_overrides_every_platform_default() {
        assert_eq!(
            resolve_default_data_dir(
                DataPlatform::Linux,
                Some(PathBuf::from("/srv/akra-state")),
                Some(PathBuf::from("/mnt/c/Users/alex/AppData/Local")),
                Some(PathBuf::from("/home/alex/.data")),
                PathBuf::from("/home/alex"),
            ),
            PathBuf::from("/srv/akra-state")
        );
    }

    #[test]
    fn windows_uses_local_app_data() {
        let local_app_data = PathBuf::from(r"C:\Users\alex\AppData\Local");
        assert_eq!(
            resolve_default_data_dir(
                DataPlatform::Windows,
                None,
                Some(local_app_data.clone()),
                Some(PathBuf::from(r"C:\wrong-xdg")),
                PathBuf::from(r"C:\Users\alex"),
            ),
            local_app_data.join("akra-hookers")
        );
    }

    #[test]
    fn ubuntu_uses_xdg_and_ignores_inherited_windows_environment() {
        let xdg_data_home = PathBuf::from("/home/alex/.data");
        assert_eq!(
            resolve_default_data_dir(
                DataPlatform::Linux,
                None,
                Some(PathBuf::from("/mnt/c/Users/alex/AppData/Local")),
                Some(xdg_data_home.clone()),
                PathBuf::from("/home/alex"),
            ),
            xdg_data_home.join("akra-hookers")
        );
        assert_eq!(
            resolve_default_data_dir(
                DataPlatform::Linux,
                None,
                Some(PathBuf::from("/mnt/c/Users/alex/AppData/Local")),
                None,
                PathBuf::from("/home/alex"),
            ),
            PathBuf::from("/home/alex")
                .join(".local")
                .join("share")
                .join("akra-hookers")
        );
    }

    #[test]
    fn apple_platforms_use_application_support() {
        for platform in [DataPlatform::MacOs, DataPlatform::Ios] {
            assert_eq!(
                resolve_default_data_dir(
                    platform,
                    None,
                    None,
                    Some(PathBuf::from("/wrong-xdg")),
                    PathBuf::from("/sandbox/home"),
                ),
                PathBuf::from("/sandbox/home")
                    .join("Library")
                    .join("Application Support")
                    .join("akra-hookers")
            );
        }
    }

    #[test]
    fn unix_home_wins_over_an_inherited_windows_profile() {
        let windows_profile = Some(PathBuf::from(r"C:\Users\alex"));
        let unix_home = Some(PathBuf::from("/home/alex"));
        assert_eq!(
            resolve_user_home(
                DataPlatform::Linux,
                windows_profile.clone(),
                unix_home.clone()
            ),
            PathBuf::from("/home/alex")
        );
        assert_eq!(
            resolve_user_home(DataPlatform::Other, windows_profile, unix_home),
            PathBuf::from("/home/alex")
        );
    }
}

#[cfg(all(test, windows))]
mod tests {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use std::path::Path;

    use super::{
        HookCommandError, hook_command, hook_command_for_target, hook_command_for_target_and_home,
        shared_wsl_hook_command, wsl_cwd_to_windows, wsl_hook_command, wsl_hook_command_for_target,
        wsl_hook_command_for_target_and_home,
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
    fn managed_hook_commands_carry_the_exact_codex_home() {
        let command = hook_command_for_target_and_home(
            Path::new(r"C:\tools\akra-hookers.exe"),
            Path::new(r"C:\state"),
            "windows-custom",
            r"D:\Codex User\.codex",
        )
        .expect("Windows command");
        let encoded = command.split_whitespace().last().expect("encoded command");
        let bytes = STANDARD.decode(encoded).expect("base64");
        let utf16 = bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        assert!(
            String::from_utf16(&utf16)
                .expect("PowerShell")
                .contains("--codex-home 'D:\\Codex User\\.codex'")
        );

        let wsl_command = wsl_hook_command_for_target_and_home(
            Path::new(r"C:\tools\akra-hookers.exe"),
            Path::new(r"C:\state"),
            "Ubuntu",
            "wsl:Ubuntu",
            "/home/alex/.codex-custom",
        )
        .expect("WSL command");
        assert!(wsl_command.contains("--codex-home '/home/alex/.codex-custom'"));
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
