use std::{
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
};

use openmw_config::OpenMWConfiguration;
use vfstool_lib::VFS;

pub fn load_config_from_path(openmw_cfg: Option<&Path>) -> io::Result<OpenMWConfiguration> {
    let config = if let Some(path) = openmw_cfg {
        OpenMWConfiguration::new(Some(explicit_config_path(path)?))
    } else {
        OpenMWConfiguration::from_env()
    };

    config.map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to read OpenMW configuration: {error}"),
        )
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConfigPathSource {
    Cli,
}

impl ConfigPathSource {
    const fn description(self) -> &'static str {
        match self {
            Self::Cli => "requested OpenMW configuration",
        }
    }
}

pub(crate) fn load_config_with_prompt(
    cli_openmw_cfg: Option<&Path>,
    stdin: &mut dyn BufRead,
    stderr: &mut dyn Write,
) -> io::Result<OpenMWConfiguration> {
    if let Some(path) = cli_openmw_cfg {
        return load_explicit_or_prompt(path, ConfigPathSource::Cli, stdin, stderr);
    }

    match load_config_from_path(None) {
        Ok(config) => {
            write_autodetected_config_message(&config, stderr)?;
            Ok(config)
        }
        Err(error) => prompt_for_default_config_path(None, &error, stdin, stderr),
    }
}

fn load_explicit_or_prompt(
    path: &Path,
    source: ConfigPathSource,
    stdin: &mut dyn BufRead,
    stderr: &mut dyn Write,
) -> io::Result<OpenMWConfiguration> {
    match load_config_from_path(Some(path)) {
        Ok(config) => Ok(config),
        Err(error) => prompt_for_default_config_path(Some((source, path)), &error, stdin, stderr),
    }
}

fn prompt_for_default_config_path(
    invalid_path: Option<(ConfigPathSource, &Path)>,
    original_error: &io::Error,
    stdin: &mut dyn BufRead,
    stderr: &mut dyn Write,
) -> io::Result<OpenMWConfiguration> {
    let default_config_file = default_user_config_file()?;

    if let Some((source, path)) = invalid_path {
        writeln!(
            stderr,
            "Greenmote could not use the {}:\n  {}\n\nReason: {original_error}",
            source.description(),
            path.display()
        )?;
        writeln!(stderr, "\nTry the default OpenMW user config path instead?")?;
    } else {
        writeln!(
            stderr,
            "Greenmote could not find an OpenMW configuration file.\n\nIt checked OpenMW's application-local and system root config locations.\n\nReason: {original_error}\n\nTry the default OpenMW user config path?"
        )?;
    }

    writeln!(stderr, "  {}", default_config_file.display())?;
    write!(stderr, "\nUse this path? [y/N]: ")?;
    stderr.flush()?;

    let mut answer = String::new();
    stdin.read_line(&mut answer)?;
    if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        return Err(no_config_selected_error(&default_config_file));
    }

    load_config_from_path(Some(&default_config_file)).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "default OpenMW config path is not valid:\n  {}\n\nReason: {error}\n\nPass one explicitly:\n\n  greenmote --openmw-cfg /path/to/openmw.cfg convert\n\nor place Greenmote where OpenMW-style config discovery can find the desired profile.",
                default_config_file.display()
            ),
        )
    })
}

fn no_config_selected_error(default_config_file: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!(
            "no OpenMW configuration selected\n\nPass one explicitly:\n\n  greenmote --openmw-cfg {} convert\n\nor place Greenmote where OpenMW-style config discovery can find the desired profile.",
            default_config_file.display()
        ),
    )
}

pub(crate) fn default_user_config_file() -> io::Result<PathBuf> {
    openmw_config::try_default_config_path()
        .map(|path| path.join("openmw.cfg"))
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("failed to determine default OpenMW user config path: {error}"),
            )
        })
}

fn write_autodetected_config_message(
    config: &OpenMWConfiguration,
    stderr: &mut dyn Write,
) -> io::Result<()> {
    let root_config_file = config.root_config_file();
    if let Ok(local_path) = openmw_config::try_default_local_path()
        && root_config_file == local_path.join("openmw.cfg")
    {
        writeln!(
            stderr,
            "Using OpenMW configuration found next to Greenmote:\n  {}",
            root_config_file.display()
        )?;
        return Ok(());
    }

    if let Ok(global_config_path) = openmw_config::try_default_global_config_path()
        && root_config_file == global_config_path.join("openmw.cfg")
    {
        writeln!(
            stderr,
            "Using system OpenMW configuration:\n  {}",
            root_config_file.display()
        )?;
    }

    Ok(())
}

fn explicit_config_path(path: &Path) -> io::Result<PathBuf> {
    let absolute_path = if path.is_relative() {
        path.canonicalize().unwrap_or_else(|_| path.to_owned())
    } else {
        path.to_owned()
    };

    if absolute_path.is_file()
        || (absolute_path.is_dir() && absolute_path.join("openmw.cfg").is_file())
    {
        return Ok(absolute_path);
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "explicit --openmw-cfg path {} is neither a file nor a directory containing openmw.cfg",
            path.display()
        ),
    ))
}

#[must_use]
pub fn persisted_config_path(config: &OpenMWConfiguration) -> PathBuf {
    config.root_config_file().to_owned()
}

#[must_use]
pub fn greenmote_config_path(config_path: Option<&Path>, config: &OpenMWConfiguration) -> PathBuf {
    config_path.map_or_else(
        || {
            config
                .user_config_path()
                .join(crate::groundcover::DEFAULT_CONFIG_NAME)
        },
        Path::to_owned,
    )
}

pub fn resolve_greenmote_config_path(
    config_path: Option<&Path>,
    openmw_cfg: Option<&Path>,
) -> io::Result<PathBuf> {
    if let Some(path) = config_path {
        return Ok(path.to_owned());
    }

    let config = load_config_from_path(openmw_cfg)?;
    Ok(greenmote_config_path(None, &config))
}

pub fn content_files(config: &OpenMWConfiguration) -> io::Result<Vec<String>> {
    let content_files = config
        .content_files_iter()
        .map(|plugin| plugin.value_str().to_owned())
        .collect::<Vec<_>>();

    if content_files.is_empty() {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "openmw.cfg has no content files",
        ))
    } else {
        Ok(content_files)
    }
}

#[must_use]
pub fn default_output_directory(config: &OpenMWConfiguration) -> PathBuf {
    config
        .data_local()
        .map_or_else(openmw_config::default_data_local_path, |data_local| {
            data_local.parsed().to_owned()
        })
}

#[must_use]
pub fn build_vfs(config: &OpenMWConfiguration) -> VFS {
    let directories = config
        .data_directories_iter()
        .map(openmw_config::DirectorySetting::parsed)
        .collect::<Vec<_>>();
    let fallback_archives = config
        .fallback_archives_iter()
        .map(openmw_config::FileSetting::value)
        .map(String::as_str)
        .collect::<Vec<_>>();

    VFS::from_directories(directories, Some(fallback_archives))
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsString,
        fs::{create_dir, write},
        path::PathBuf,
        sync::{
            Mutex, OnceLock,
            atomic::{AtomicU64, Ordering},
        },
    };

    use super::*;

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "greenmote-openmw-test-{}-{}",
                std::process::id(),
                NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            create_dir(&path).unwrap();

            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn snapshot_env(keys: &[&str]) -> Vec<(String, Option<OsString>)> {
        keys.iter()
            .map(|key| ((*key).to_owned(), std::env::var_os(key)))
            .collect()
    }

    fn restore_env(snapshot: Vec<(String, Option<OsString>)>) {
        for (key, value) in snapshot {
            // SAFETY: guarded by a process-wide mutex in this module's tests.
            unsafe {
                if let Some(value) = value {
                    std::env::set_var(&key, value);
                } else {
                    std::env::remove_var(&key);
                }
            }
        }
    }

    #[test]
    fn implicit_loading_honors_openmw_config_dir_env() {
        let _guard = env_lock().lock().unwrap();
        let snapshot = snapshot_env(&["OPENMW_CONFIG", "OPENMW_CONFIG_DIR"]);
        let dir = TempDir::new();
        write(dir.path.join("openmw.cfg"), "").unwrap();

        // SAFETY: guarded by a process-wide mutex in this module's tests.
        unsafe {
            std::env::remove_var("OPENMW_CONFIG");
            std::env::set_var("OPENMW_CONFIG_DIR", &dir.path);
        }

        let config = load_config_from_path(None).unwrap();

        assert_eq!(config.root_config_file(), dir.path.join("openmw.cfg"));
        restore_env(snapshot);
    }

    #[test]
    fn persisted_path_uses_loaded_root_config_file() {
        let dir = TempDir::new();
        let cfg = dir.path.join("openmw.cfg");
        write(&cfg, "").unwrap();

        let config = load_config_from_path(Some(&dir.path)).unwrap();

        assert_eq!(persisted_config_path(&config), cfg);
    }

    #[test]
    fn explicit_greenmote_config_path_does_not_force_openmw_discovery() {
        let _guard = env_lock().lock().unwrap();
        let snapshot = snapshot_env(&["OPENMW_CONFIG", "OPENMW_CONFIG_DIR"]);
        let dir = TempDir::new();
        let config_path = dir.path.join("custom-greenmote.toml");

        // SAFETY: guarded by a process-wide mutex in this module's tests.
        unsafe {
            std::env::set_var("OPENMW_CONFIG", dir.path.join("missing-openmw.cfg"));
            std::env::remove_var("OPENMW_CONFIG_DIR");
        }

        let resolved = resolve_greenmote_config_path(Some(&config_path), None).unwrap();

        assert_eq!(resolved, config_path);
        restore_env(snapshot);
    }

    #[test]
    fn missing_autodetected_config_prompts_for_default_user_config() {
        let _guard = env_lock().lock().unwrap();
        let snapshot = snapshot_env(&[
            "OPENMW_CONFIG",
            "OPENMW_CONFIG_DIR",
            "OPENMW_GLOBAL_CONFIG_PATH",
            "XDG_CONFIG_HOME",
        ]);
        let dir = TempDir::new();
        let default_config_dir = dir.path.join("xdg").join("openmw");
        std::fs::create_dir_all(&default_config_dir).unwrap();
        let default_config = default_config_dir.join("openmw.cfg");
        write(&default_config, "").unwrap();

        // SAFETY: guarded by a process-wide mutex in this module's tests.
        unsafe {
            std::env::remove_var("OPENMW_CONFIG");
            std::env::remove_var("OPENMW_CONFIG_DIR");
            std::env::set_var("OPENMW_GLOBAL_CONFIG_PATH", dir.path.join("missing-global"));
            std::env::set_var("XDG_CONFIG_HOME", dir.path.join("xdg"));
        }

        let mut input = io::Cursor::new(b"y\n");
        let mut stderr = Vec::new();
        let config = load_config_with_prompt(None, &mut input, &mut stderr).unwrap();

        assert_eq!(config.root_config_file(), default_config);
        let message = String::from_utf8(stderr).unwrap();
        assert!(message.contains("could not find an OpenMW configuration"));
        assert!(message.contains("Use this path? [y/N]:"));
        restore_env(snapshot);
    }

    #[test]
    fn declining_default_user_config_reports_repair_instructions() {
        let _guard = env_lock().lock().unwrap();
        let snapshot = snapshot_env(&[
            "OPENMW_CONFIG",
            "OPENMW_CONFIG_DIR",
            "OPENMW_GLOBAL_CONFIG_PATH",
            "XDG_CONFIG_HOME",
        ]);
        let dir = TempDir::new();

        // SAFETY: guarded by a process-wide mutex in this module's tests.
        unsafe {
            std::env::remove_var("OPENMW_CONFIG");
            std::env::remove_var("OPENMW_CONFIG_DIR");
            std::env::set_var("OPENMW_GLOBAL_CONFIG_PATH", dir.path.join("missing-global"));
            std::env::set_var("XDG_CONFIG_HOME", dir.path.join("xdg"));
        }

        let mut input = io::Cursor::new(b"\n");
        let mut stderr = Vec::new();
        let error = load_config_with_prompt(None, &mut input, &mut stderr).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(
            error
                .to_string()
                .contains("no OpenMW configuration selected")
        );
        assert!(error.to_string().contains("--openmw-cfg"));
        restore_env(snapshot);
    }

    #[test]
    fn invalid_cli_path_has_distinct_prompt_message() {
        let _guard = env_lock().lock().unwrap();
        let snapshot = snapshot_env(&["XDG_CONFIG_HOME"]);
        let dir = TempDir::new();
        let default_config_dir = dir.path.join("xdg").join("openmw");
        std::fs::create_dir_all(&default_config_dir).unwrap();
        let default_config = default_config_dir.join("openmw.cfg");
        write(&default_config, "").unwrap();
        let bad_config = dir.path.join("missing.cfg");

        // SAFETY: guarded by a process-wide mutex in this module's tests.
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir.path.join("xdg"));
        }

        let mut input = io::Cursor::new(b"yes\n");
        let mut stderr = Vec::new();
        let config = load_config_with_prompt(Some(&bad_config), &mut input, &mut stderr).unwrap();

        assert_eq!(config.root_config_file(), default_config);
        let message = String::from_utf8(stderr).unwrap();
        assert!(message.contains("requested OpenMW configuration"));
        assert!(message.contains(&bad_config.display().to_string()));
        restore_env(snapshot);
    }
}
