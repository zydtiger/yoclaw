use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use tokio::fs;

use crate::cli::SkillSource;

use super::{loader, Skill, SkillInstallError, SkillLoadError, SkillStore};

struct PreparedSkillSource {
    root: PathBuf,
    _temp_path: Option<TempPathGuard>,
}

/// RAII guard to ensure temporary directories are cleaned up on drop.
/// This prevents orphaned temp files when skill installation fails partway through.
struct TempPathGuard {
    path: PathBuf,
}

impl Drop for TempPathGuard {
    fn drop(&mut self) {
        if self.path.is_dir() {
            let _ = std::fs::remove_dir_all(&self.path);
        } else {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Installs a skill from the given source, validating and copying it into the skills directory.
///
/// The installation process:
/// 1. Prepares the source (validates directory or extracts zip)
/// 2. Loads and validates the skill manifest
/// 3. Checks for duplicate skill names and destinations
/// 4. Copies to a staging directory, then atomically renames to final location
///
/// The staging+rename pattern ensures partial installs don't leave corrupt data.
pub async fn install_skill(source: &SkillSource) -> Result<Skill, SkillInstallError> {
    let skills_dir = Path::new(&*crate::globals::CONFIG_DIR).join("skills");
    fs::create_dir_all(&skills_dir)
        .await
        .map_err(|source| SkillInstallError::Io {
            context: format!("Failed to create skills directory {}", skills_dir.display()),
            source,
        })?;

    // Prepare source: for directories this validates SKILL.md exists,
    // for zips this extracts to a temp directory
    let prepared = prepare_skill_source(source).await?;

    // Load skill metadata from the manifest
    let skill = loader::load_skill_from_path(&prepared.root)
        .await
        .map_err(map_skill_load_error)?;

    // Check against already-installed skills to prevent duplicate names
    let installed_skills =
        SkillStore::load_skills()
            .await
            .map_err(|source| SkillInstallError::Io {
                context: format!(
                    "Failed to inspect installed skills in {}",
                    skills_dir.display()
                ),
                source,
            })?;

    // Reject if a skill with the same name already exists
    if installed_skills
        .skills
        .iter()
        .any(|installed_skill| installed_skill.name == skill.name)
    {
        return Err(SkillInstallError::DuplicateSkillName(skill.name));
    }

    // Convert the skill name to a filesystem-safe slug
    let destination_name = slugify_skill_name(&skill.name)
        .ok_or_else(|| SkillInstallError::InvalidSkillName(skill.name.clone()))?;
    let destination = skills_dir.join(destination_name);

    // Verify the destination directory doesn't already exist
    if fs::try_exists(&destination)
        .await
        .map_err(|source| SkillInstallError::Io {
            context: format!("Failed to check {}", destination.display()),
            source,
        })?
    {
        return Err(SkillInstallError::DuplicateDestination(destination));
    }

    // Copy to a staging directory first (atomic rename pattern)
    // This prevents partial installs from appearing in the final location
    let staging = skills_dir.join(format!(".install-{}", uuid::Uuid::now_v7()));
    let copy_source = prepared.root.clone();
    let copy_destination = staging.clone();

    // Use spawn_blocking for the recursive copy since it's CPU-bound
    let copy_result =
        tokio::task::spawn_blocking(move || copy_dir_recursive(&copy_source, &copy_destination))
            .await
            .map_err(SkillInstallError::TaskJoin)?;

    if let Err(source) = copy_result {
        let _ = fs::remove_dir_all(&staging).await;
        return Err(SkillInstallError::Io {
            context: format!(
                "Failed to copy skill into staging directory {}",
                staging.display()
            ),
            source,
        });
    }

    // Atomic move from staging to final destination
    if let Err(source) = fs::rename(&staging, &destination).await {
        let _ = fs::remove_dir_all(&staging).await;
        return Err(SkillInstallError::Io {
            context: format!(
                "Failed to move skill into destination {}",
                destination.display()
            ),
            source,
        });
    }
    Ok(Skill {
        name: skill.name,
        description: skill.description,
        version: skill.version,
        path: destination.to_string_lossy().to_string(),
    })
}

/// Prepares the skill source by validating directories or extracting zip archives.
/// Returns a `PreparedSkillSource` with the root path and a guard for cleanup.
async fn prepare_skill_source(
    source: &SkillSource,
) -> Result<PreparedSkillSource, SkillInstallError> {
    match source {
        SkillSource::Dir(path) => prepare_directory_source(Path::new(path)).await,
        SkillSource::Zip(source) => prepare_zip_source(source).await,
    }
}

/// Validates that the directory contains a valid SKILL.md file.
async fn prepare_directory_source(path: &Path) -> Result<PreparedSkillSource, SkillInstallError> {
    let path = expand_install_source_path(&path.to_string_lossy());
    let metadata = fs::metadata(&path)
        .await
        .map_err(|source| SkillInstallError::Io {
            context: format!("Failed to read skill directory {}", path.display()),
            source,
        })?;

    if !metadata.is_dir() {
        return Err(SkillInstallError::InvalidDirectory(path.to_path_buf()));
    }

    // Ensure the directory contains a valid SKILL.md
    ensure_skill_file(&path).await?;

    Ok(PreparedSkillSource {
        root: path.to_path_buf(),
        _temp_path: None,
    })
}

/// Extracts a zip file (local or remote) to a temporary directory.
/// Handles both local file paths and HTTP/HTTPS URLs.
async fn prepare_zip_source(source: &str) -> Result<PreparedSkillSource, SkillInstallError> {
    if !source.to_ascii_lowercase().ends_with(".zip") {
        return Err(SkillInstallError::InvalidZipSource(source.to_string()));
    }

    // Download or read the zip file based on source type
    let bytes = if is_remote_zip(source) {
        let zip_url = source.to_string();
        let response = reqwest::get(source)
            .await
            .map_err(|error| SkillInstallError::Http {
                context: format!("Failed to download zip from {zip_url}"),
                source: error,
            })?
            .error_for_status()
            .map_err(|error| SkillInstallError::Http {
                context: format!("Received error response while downloading zip from {zip_url}"),
                source: error,
            })?;

        response
            .bytes()
            .await
            .map_err(|error| SkillInstallError::Http {
                context: format!("Failed to read zip response body from {zip_url}"),
                source: error,
            })?
            .to_vec()
    } else {
        let zip_path = expand_install_source_path(source);
        fs::read(&zip_path)
            .await
            .map_err(|error| SkillInstallError::Io {
                context: format!("Failed to read zip file {}", zip_path.display()),
                source: error,
            })?
    };

    let temp_path =
        std::env::temp_dir().join(format!("yoclaw-skill-import-{}", uuid::Uuid::now_v7()));
    fs::create_dir_all(&temp_path)
        .await
        .map_err(|source| SkillInstallError::Io {
            context: format!(
                "Failed to create temporary directory {}",
                temp_path.display()
            ),
            source,
        })?;

    // Extract the zip to the temp directory using blocking I/O
    let extract_path = temp_path.clone();
    tokio::task::spawn_blocking(move || extract_zip_bytes(&bytes, &extract_path))
        .await
        .map_err(SkillInstallError::TaskJoin)?
        .map_err(|source| SkillInstallError::Zip {
            context: format!("Failed to extract zip archive into {}", temp_path.display()),
            source,
        })?;

    // Resolve the actual skill root (may be wrapped in a single directory)
    let root = resolve_extracted_skill_root(&temp_path).await?;
    ensure_skill_file(&root).await?;

    Ok(PreparedSkillSource {
        root,
        _temp_path: Some(TempPathGuard { path: temp_path }),
    })
}

/// Finds the skill root in an extracted zip by looking for SKILL.md.
/// Handles both: zip root containing SKILL.md directly, or a single wrapping directory.
async fn resolve_extracted_skill_root(path: &Path) -> Result<PathBuf, SkillInstallError> {
    // Check if SKILL.md is at the zip root level
    let root_skill = path.join("SKILL.md");
    if fs::try_exists(&root_skill)
        .await
        .map_err(|source| SkillInstallError::Io {
            context: format!("Failed to inspect {}", root_skill.display()),
            source,
        })?
    {
        return Ok(path.to_path_buf());
    }

    // Otherwise, expect exactly one directory that contains SKILL.md
    let mut entries = fs::read_dir(path)
        .await
        .map_err(|source| SkillInstallError::Io {
            context: format!(
                "Failed to inspect extracted zip directory {}",
                path.display()
            ),
            source,
        })?;

    let mut children = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|source| SkillInstallError::Io {
            context: format!(
                "Failed to inspect extracted zip directory {}",
                path.display()
            ),
            source,
        })?
    {
        children.push(entry.path());
    }

    if children.len() != 1 {
        return Err(SkillInstallError::InvalidArchiveLayout(
            "zip archive must contain a single skill root or a single wrapping directory"
                .to_string(),
        ));
    }

    let child = &children[0];
    let child_metadata = fs::metadata(child)
        .await
        .map_err(|source| SkillInstallError::Io {
            context: format!("Failed to inspect {}", child.display()),
            source,
        })?;

    if !child_metadata.is_dir() {
        return Err(SkillInstallError::InvalidArchiveLayout(
            "zip archive must contain a directory with SKILL.md at its root".to_string(),
        ));
    }

    // Verify the single child directory contains SKILL.md
    let child_skill = child.join("SKILL.md");
    if !fs::try_exists(&child_skill)
        .await
        .map_err(|source| SkillInstallError::Io {
            context: format!("Failed to inspect {}", child_skill.display()),
            source,
        })?
    {
        return Err(SkillInstallError::MissingSkillFile(child_skill));
    }

    Ok(child.to_path_buf())
}

/// Checks whether the given path contains a valid SKILL.md file.
async fn ensure_skill_file(path: &Path) -> Result<(), SkillInstallError> {
    let skill_md = path.join("SKILL.md");
    let exists = fs::try_exists(&skill_md)
        .await
        .map_err(|source| SkillInstallError::Io {
            context: format!("Failed to inspect {}", skill_md.display()),
            source,
        })?;

    if !exists {
        return Err(SkillInstallError::MissingSkillFile(skill_md));
    }

    Ok(())
}

/// Detects if the source is a remote ZIP URL (http:// or https://).
fn is_remote_zip(source: &str) -> bool {
    source.to_ascii_lowercase().ends_with(".zip")
        && (source.starts_with("http://") || source.starts_with("https://"))
}

/// Expands shell-style shortcuts in local install paths before filesystem access.
fn expand_install_source_path(source: &str) -> PathBuf {
    expand_env_vars(&expand_home_shortcut(source))
}

/// Expands `~` and `~/...` to the current user's home directory.
fn expand_home_shortcut(source: &str) -> PathBuf {
    match source {
        "~" => dirs::home_dir().unwrap_or_else(|| PathBuf::from(source)),
        _ => {
            if let Some(rest) = source
                .strip_prefix("~/")
                .or_else(|| source.strip_prefix("~\\"))
            {
                dirs::home_dir()
                    .map(|home| home.join(rest))
                    .unwrap_or_else(|| PathBuf::from(source))
            } else {
                return PathBuf::from(source);
            }
        }
    }
}

/// Expands every `$VAR` and `${VAR}` occurrence, leaving unknown variables unchanged.
fn expand_env_vars(path: &Path) -> PathBuf {
    let source = path.to_string_lossy();
    let mut expanded = String::with_capacity(source.len());
    let mut cursor = source.as_ref();

    while let Some(marker_index) = cursor.find('$') {
        expanded.push_str(&cursor[..marker_index]);
        let variable = &cursor[marker_index..];

        if let Some((name, rest, token_len)) = parse_env_var(variable) {
            if let Some(value) = std::env::var_os(name) {
                expanded.push_str(&value.to_string_lossy());
            } else {
                expanded.push_str(&variable[..token_len]);
            }
            cursor = rest;
        } else {
            expanded.push('$');
            cursor = &variable['$'.len_utf8()..];
        }
    }

    expanded.push_str(cursor);
    PathBuf::from(expanded)
}

/// Parses a single environment-variable token from the start of `source`.
fn parse_env_var(source: &str) -> Option<(&str, &str, usize)> {
    if let Some(rest) = source.strip_prefix("${") {
        let end = rest.find('}')?;
        let name = &rest[..end];
        if name.is_empty() {
            return None;
        }
        return Some((name, &rest[end + 1..], end + 3));
    }

    let rest = source.strip_prefix('$')?;
    let end = rest
        .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .unwrap_or(rest.len());

    if end == 0 {
        return None;
    }

    Some((&rest[..end], &rest[end..], end + 1))
}

/// Maps SkillLoadError to SkillInstallError variants.
fn map_skill_load_error(error: SkillLoadError) -> SkillInstallError {
    match error {
        SkillLoadError::MissingSkillFile(path) => SkillInstallError::MissingSkillFile(path),
        SkillLoadError::UnsupportedPath(path) => SkillInstallError::InvalidDirectory(path),
        SkillLoadError::FsError { context, source } => SkillInstallError::Io { context, source },
    }
}

/// Extracts a ZIP archive from bytes to a destination directory.
/// Uses standard library I/O operations wrapped in blocking context.
fn extract_zip_bytes(bytes: &[u8], destination: &Path) -> Result<(), zip::result::ZipError> {
    let reader = Cursor::new(bytes.to_vec());
    let mut archive = zip::ZipArchive::new(reader)?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        // Skip entries with invalid paths (e.g., containing `..`)
        let Some(relative_path) = entry.enclosed_name().map(|path| path.to_path_buf()) else {
            continue;
        };

        let output_path = destination.join(relative_path);

        // Create directories directly
        if entry.is_dir() {
            std::fs::create_dir_all(&output_path).map_err(zip::result::ZipError::Io)?;
            continue;
        }

        // Ensure parent directories exist for files
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).map_err(zip::result::ZipError::Io)?;
        }

        // Extract file content
        let mut output_file =
            std::fs::File::create(&output_path).map_err(zip::result::ZipError::Io)?;
        let mut buffer = Vec::new();
        entry
            .read_to_end(&mut buffer)
            .map_err(zip::result::ZipError::Io)?;
        output_file
            .write_all(&buffer)
            .map_err(zip::result::ZipError::Io)?;
    }

    Ok(())
}

/// Recursively copies a directory tree using blocking I/O.
/// Only supports files and directories; returns error for symlinks or other types.
fn copy_dir_recursive(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(destination)?;

    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
            continue;
        }

        if !file_type.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unsupported non-file entry at {}", source_path.display()),
            ));
        }

        if let Some(parent) = destination_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&source_path, &destination_path)?;
    }

    Ok(())
}

/// Converts a skill name into a filesystem-safe slug.
/// Lowercases alphanumeric chars, replaces others with dashes, no consecutive dashes.
/// Returns `None` if the resulting slug would be empty.
fn slugify_skill_name(name: &str) -> Option<String> {
    let mut slug = String::new();
    let mut last_was_dash = false;

    for ch in name.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            slug.push(lower);
            last_was_dash = false;
        } else if !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }

    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        None
    } else {
        Some(slug)
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::{LazyLock, OnceLock};

    use zip::write::FileOptions;

    use crate::cli::SkillSource;

    use super::{expand_install_source_path, install_skill, SkillInstallError};

    static TEST_LOCK: LazyLock<std::sync::Mutex<()>> = LazyLock::new(|| std::sync::Mutex::new(()));
    static TEST_CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();

    struct TestDir {
        path: std::path::PathBuf,
    }

    impl TestDir {
        fn new(prefix: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("yoclaw-{prefix}-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir_all(&path).expect("test temp directory should be created");
            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn installs_valid_directory_skill() {
        let _guard = TEST_LOCK.lock().unwrap();
        let config_dir = reset_test_config_dir().await;
        let source_dir = TestDir::new("source");
        write_skill_dir(
            &source_dir.path,
            "directory-skill",
            Some("Directory Skill"),
            &[("assets/note.txt", "hello")],
        );

        let installed = install_skill(&SkillSource::Dir(
            source_dir.path.to_string_lossy().to_string(),
        ))
        .await
        .expect("directory install should succeed");

        let installed_path = Path::new(&installed.path);
        assert_eq!(installed.name, "Directory Skill");
        assert!(installed_path.join("assets/note.txt").exists());
        assert!(installed_path.starts_with(&config_dir));
        assert!(installed_path.join("SKILL.md").exists());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn installs_valid_zip_skill() {
        let _guard = TEST_LOCK.lock().unwrap();
        let config_dir = reset_test_config_dir().await;
        let zip_dir = TestDir::new("zip");
        let zip_path = zip_dir.path.join("skill.zip");
        write_zip_archive(
            &zip_path,
            &[
                (
                    "zip-skill/SKILL.md",
                    "---\nname: Zip Skill\n---\n# Zip Skill\n",
                ),
                ("zip-skill/assets/tool.sh", "#!/bin/sh\necho zip\n"),
            ],
        );

        let installed = install_skill(&SkillSource::Zip(zip_path.to_string_lossy().to_string()))
            .await
            .expect("zip install should succeed");

        let installed_path = Path::new(&installed.path);
        assert_eq!(installed.name, "Zip Skill");
        assert!(installed_path.starts_with(&config_dir));
        assert!(installed_path.join("assets/tool.sh").exists());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn accepts_zip_with_single_wrapper_directory() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_config_dir().await;
        let zip_dir = TestDir::new("zip");
        let zip_path = zip_dir.path.join("wrapped.zip");
        write_zip_archive(
            &zip_path,
            &[(
                "wrapper/SKILL.md",
                "---\nname: Wrapped Skill\n---\n# Wrapped Skill\n",
            )],
        );

        let installed = install_skill(&SkillSource::Zip(zip_path.to_string_lossy().to_string()))
            .await
            .expect("wrapped zip install should succeed");

        assert_eq!(installed.name, "Wrapped Skill");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_skill_directory_without_skill_md() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_config_dir().await;
        let source_dir = TestDir::new("source");

        let error = install_skill(&SkillSource::Dir(
            source_dir.path.to_string_lossy().to_string(),
        ))
        .await
        .expect_err("install should fail without SKILL.md");

        assert!(matches!(error, SkillInstallError::MissingSkillFile(_)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_zip_with_multiple_candidate_roots() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_config_dir().await;
        let zip_dir = TestDir::new("zip");
        let zip_path = zip_dir.path.join("bad.zip");
        write_zip_archive(
            &zip_path,
            &[
                ("a/SKILL.md", "---\nname: A Skill\n---\n"),
                ("b/SKILL.md", "---\nname: B Skill\n---\n"),
            ],
        );

        let error = install_skill(&SkillSource::Zip(zip_path.to_string_lossy().to_string()))
            .await
            .expect_err("install should fail for ambiguous zip");

        assert!(matches!(error, SkillInstallError::InvalidArchiveLayout(_)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_duplicate_skill_name() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_config_dir().await;
        let first_source = TestDir::new("source");
        let second_source = TestDir::new("source");
        write_skill_dir(
            &first_source.path,
            "first",
            Some("Shared Name"),
            &[("README.txt", "first")],
        );
        write_skill_dir(
            &second_source.path,
            "second",
            Some("Shared Name"),
            &[("README.txt", "second")],
        );

        install_skill(&SkillSource::Dir(
            first_source.path.to_string_lossy().to_string(),
        ))
        .await
        .expect("first install should succeed");

        let error = install_skill(&SkillSource::Dir(
            second_source.path.to_string_lossy().to_string(),
        ))
        .await
        .expect_err("duplicate skill name should fail");

        assert!(
            matches!(error, SkillInstallError::DuplicateSkillName(name) if name == "Shared Name")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_existing_destination_directory() {
        let _guard = TEST_LOCK.lock().unwrap();
        let config_dir = reset_test_config_dir().await;
        let skills_dir = config_dir.join("skills");
        std::fs::create_dir_all(skills_dir.join("existing-skill"))
            .expect("destination directory should be created");

        let source_dir = TestDir::new("source");
        write_skill_dir(
            &source_dir.path,
            "source",
            Some("Existing Skill"),
            &[("README.txt", "content")],
        );

        let error = install_skill(&SkillSource::Dir(
            source_dir.path.to_string_lossy().to_string(),
        ))
        .await
        .expect_err("existing destination should fail");

        assert!(
            matches!(error, SkillInstallError::DuplicateDestination(path) if path.ends_with("existing-skill"))
        );
    }

    #[test]
    fn detects_remote_zip_urls_only_for_http_zip_sources() {
        assert!(super::is_remote_zip("https://example.com/skill.zip"));
        assert!(super::is_remote_zip("http://example.com/skill.ZIP"));
        assert!(!super::is_remote_zip("https://example.com/skill"));
        assert!(!super::is_remote_zip("/tmp/skill.zip"));
    }

    #[test]
    fn expands_home_shortcuts_for_install_paths() {
        let _guard = TEST_LOCK.lock().unwrap();
        let home_dir = TestDir::new("home");
        let previous_home = std::env::var_os("HOME");
        std::env::set_var("HOME", &home_dir.path);

        let expanded = expand_install_source_path("~/Downloads/tavily-search");

        restore_env_var("HOME", previous_home);
        assert_eq!(expanded, home_dir.path.join("Downloads/tavily-search"));
    }

    #[test]
    fn expands_env_vars_for_install_paths() {
        let _guard = TEST_LOCK.lock().unwrap();
        let source_dir = TestDir::new("env-root");
        let previous_root = std::env::var_os("YOCLAW_SKILL_TEST_ROOT");
        std::env::set_var("YOCLAW_SKILL_TEST_ROOT", &source_dir.path);

        let expanded = expand_install_source_path("$YOCLAW_SKILL_TEST_ROOT/skill-dir");
        let braced = expand_install_source_path("${YOCLAW_SKILL_TEST_ROOT}/skill-dir");
        let nested = expand_install_source_path("prefix/$YOCLAW_SKILL_TEST_ROOT/skill-dir");

        restore_env_var("YOCLAW_SKILL_TEST_ROOT", previous_root);
        assert_eq!(expanded, source_dir.path.join("skill-dir"));
        assert_eq!(braced, source_dir.path.join("skill-dir"));
        assert_eq!(
            nested,
            PathBuf::from("prefix")
                .join(&source_dir.path)
                .join("skill-dir")
        );
    }

    async fn reset_test_config_dir() -> PathBuf {
        let config_dir = test_config_dir().clone();
        let skills_dir = config_dir.join("skills");

        let _ = tokio::fs::remove_dir_all(&skills_dir).await;
        tokio::fs::create_dir_all(&config_dir)
            .await
            .expect("test config directory should be created");

        config_dir
    }

    fn test_config_dir() -> &'static PathBuf {
        TEST_CONFIG_DIR.get_or_init(|| {
            let path =
                std::env::temp_dir().join(format!("yoclaw-install-tests-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir_all(&path).expect("test config directory should be created");
            std::env::set_var("CONFIG_PATH", &path);
            path
        })
    }

    fn write_skill_dir(
        path: &Path,
        default_name: &str,
        explicit_name: Option<&str>,
        extra_files: &[(&str, &str)],
    ) {
        let skill_contents = match explicit_name {
            Some(name) => format!("---\nname: {name}\n---\n# {name}\n"),
            None => format!("# {default_name}\n"),
        };
        std::fs::write(path.join("SKILL.md"), skill_contents)
            .expect("skill file should be written");

        for (relative_path, contents) in extra_files {
            let target_path = path.join(relative_path);
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent).expect("file parent should be created");
            }
            std::fs::write(target_path, contents).expect("extra file should be written");
        }
    }

    fn write_zip_archive(path: &Path, files: &[(&str, &str)]) {
        let file = std::fs::File::create(path).expect("zip file should be created");
        let mut zip = zip::ZipWriter::new(file);
        let options = FileOptions::default();

        for (name, contents) in files {
            zip.start_file(*name, options)
                .expect("zip entry should be created");
            use std::io::Write;
            zip.write_all(contents.as_bytes())
                .expect("zip entry contents should be written");
        }

        zip.finish().expect("zip file should finish");
    }

    fn restore_env_var(name: &str, value: Option<std::ffi::OsString>) {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }
}
