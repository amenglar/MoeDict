use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub mdx_dirs: Vec<PathBuf>,
}

pub fn parse_args() -> Options {
    let mut options = Options::default();
    let mut iter = std::env::args_os().skip(1);
    while let Some(arg) = iter.next() {
        if arg == "--mdx-dir" {
            if let Some(value) = iter.next() {
                options.mdx_dirs.push(PathBuf::from(value));
            }
        }
    }
    if options.mdx_dirs.is_empty() {
        if let Some(value) = std::env::var_os("MOEDICT_MDX_DIR") {
            for part in std::env::split_paths(&value) {
                if !part.as_os_str().is_empty() {
                    options.mdx_dirs.push(part);
                }
            }
        }
    }
    options
}

fn override_dir(name: &str, fallback: Option<PathBuf>) -> PathBuf {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or(fallback)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn config_dir() -> PathBuf {
    override_dir(
        "MOEDICT_CONFIG_DIR",
        dirs::config_dir().map(|p| p.join("moe.mdict")),
    )
}

pub fn data_dir() -> PathBuf {
    override_dir(
        "MOEDICT_DATA_DIR",
        dirs::data_dir().map(|p| p.join("moe.mdict")),
    )
}

pub fn log_dir() -> PathBuf {
    override_dir(
        "MOEDICT_LOG_DIR",
        dirs::data_dir().map(|p| p.join("moe.mdict").join("logs")),
    )
}

pub fn index_dir() -> PathBuf {
    data_dir().join("index")
}

pub fn state_path() -> PathBuf {
    config_dir().join("native-state.json")
}

pub fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
