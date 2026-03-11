use std::sync::LazyLock;

pub static CONFIG_DIR: LazyLock<String> = LazyLock::new(|| {
    std::env::var("CONFIG_PATH").unwrap_or_else(|_| {
        dirs::home_dir()
            .map(|home| home.join(".yoclaw").to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string())
    })
});
