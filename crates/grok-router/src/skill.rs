//! Skill stub install, status, and removal.
//!
//! The long guide lives only in this module. Setup writes a short `SKILL.md`
//! whose body tells the agent to run `grok-router __internal skill`. Paths come
//! from the caller. This module does not read the home directory.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Directory name written under the skill root.
const STUB_DIR_NAME: &str = "grok-router";
/// File name of the installed stub.
const STUB_FILE_NAME: &str = "SKILL.md";
/// Line that marks a directory this command may remove.
const MARKER: &str = "grok-router-skill-stub";

/// Long guide printed by `skill show`. Not written into the stub.
const GUIDE: &str = "\
grok-router

Commands: list, search, benchmarks, add, set-default, remove, skill.

list, search, and benchmarks read.
benchmarks prints published price and scores from an openrouter-models catalog. Other catalogs refuse and download nothing.
add and set-default write.
remove deletes one saved model and refuses unless --yes is set.
skill setup writes a short stub.
skill remove deletes that stub directory only when it carries the grok-router marker and --yes is set.
skill status and skill show read.
--dry-run on a write command writes nothing.

The saved default applies to the next session.

`grok-router skill show` and `grok-router __internal skill` print this guide.
The installed stub does not contain this guide. It tells you to run `grok-router __internal skill`.

Examples:
  grok-router list
  grok-router search glm
  grok-router benchmarks
  grok-router benchmarks kimi
  grok-router add z-ai/glm-5.3
  grok-router set-default or-glm-5-3
  grok-router remove or-glm-5-3 --yes
  grok-router skill setup --skill-root /tmp/skills
  grok-router skill status --skill-root /tmp/skills
  grok-router skill show
  grok-router skill remove --skill-root /tmp/skills --yes
";

/// Short `SKILL.md` body. It points the agent at `__internal skill`.
const STUB: &str = "\
---
name: grok-router
description: Run `grok-router __internal skill` and follow that guide.
---

# grok-router

grok-router-skill-stub

Run `grok-router __internal skill`.
";

/// Whether a write should touch the filesystem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteMode {
    /// Create or replace the stub when it is missing or stale.
    Apply,
    /// Report the change and write nothing.
    DryRun,
}

impl WriteMode {
    /// Whether this mode must not create, replace, or delete.
    #[must_use]
    pub const fn is_dry_run(self) -> bool {
        matches!(self, Self::DryRun)
    }
}

/// Whether `skill remove` may delete.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confirmation {
    /// `--yes` was set.
    Yes,
    /// `--yes` was not set. Removal must not touch the filesystem.
    No,
}

/// What is at `<skill-root>/grok-router` before a dry-run, or after an apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    /// The stub is not installed. An empty stub directory counts as missing.
    Missing,
    /// `SKILL.md` is exactly the stub this binary writes.
    Current,
    /// The path exists and does not carry our marker. It must be left in place.
    Foreign,
    /// The marker is present, but `SKILL.md` is not the current stub.
    Stale,
}

impl Presence {
    /// Stable status word for text and JSON.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Current => "current",
            Self::Foreign => "foreign",
            Self::Stale => "stale",
        }
    }
}

/// What a setup or remove did, or would have done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// The stub was already in the reported state. Nothing was written.
    Unchanged,
    /// Setup wrote the one stub.
    Wrote,
    /// Setup would write the one stub. Nothing was written.
    WouldWrite,
    /// Remove deleted the stub directory.
    Removed,
    /// Remove would delete the stub directory. Nothing was written.
    WouldRemove,
    /// Remove left a path that does not carry the marker.
    Left,
}

impl Effect {
    /// Stable effect word for JSON.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unchanged => "unchanged",
            Self::Wrote => "wrote",
            Self::WouldWrite => "would_write",
            Self::Removed => "removed",
            Self::WouldRemove => "would_remove",
            Self::Left => "left",
        }
    }
}

/// Result of inspecting, installing, or removing the stub.
#[derive(Clone, Debug, PartialEq, Eq)]
#[must_use]
pub struct Report {
    /// `<skill-root>/grok-router`, whether or not it exists.
    pub path: PathBuf,
    /// Presence after an apply, or before a dry-run.
    pub disk: Presence,
    /// Dry-run bit. Status uses [`WriteMode::Apply`] because status never writes.
    pub mode: WriteMode,
    /// The mutation, the preview, or the refusal to touch a foreign path.
    pub effect: Effect,
}

/// Why setup or remove did not finish.
#[derive(Debug, thiserror::Error)]
pub enum SkillFailure {
    /// Removal was not confirmed. The filesystem was not touched.
    #[error(
        "`skill remove` refuses to delete without --yes. Nothing was changed. Re-run with --yes, or pass --yes --dry-run to preview."
    )]
    NeedsYes,
    /// The stub path is not ours, so nothing was written.
    #[error(
        "{} has no grok-router-skill-stub marker, so it was left in place and no stub was written. Move that directory aside or pass a different --skill-root.",
        path.display()
    )]
    Foreign {
        /// Path that was left in place.
        path: PathBuf,
    },
    /// A filesystem call failed.
    #[error("could not update {}: {source}. {next}", path.display())]
    Io {
        /// Path the call was about.
        path: PathBuf,
        /// The filesystem error.
        #[source]
        source: io::Error,
        /// What to do next.
        next: &'static str,
    },
}

impl SkillFailure {
    /// Machine-readable class: `refused` or `io`.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::NeedsYes | Self::Foreign { .. } => "refused",
            Self::Io { .. } => "io",
        }
    }

    /// Build an I/O failure that names `path` and what to do next.
    fn io(path: &Path, source: io::Error, next: &'static str) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            source,
            next,
        }
    }
}

/// The guide printed by `skill show` and `__internal skill`.
///
/// This text is not written into the stub.
#[must_use]
pub const fn guide() -> &'static str {
    GUIDE
}

/// Read whether the stub is missing, current, stale, or foreign.
///
/// An empty `<skill-root>/grok-router` directory counts as missing so setup can
/// write the one stub into it. This function does not create or delete.
///
/// # Errors
///
/// Returns a failure when the stub path cannot be read.
pub fn inspect(root: &Path) -> Result<Report, SkillFailure> {
    let path = stub_dir(root);
    let disk = presence(&path)?;
    Ok(Report {
        path,
        disk,
        mode: WriteMode::Apply,
        effect: Effect::Unchanged,
    })
}

/// Write the one stub, or leave it untouched when it is already current.
///
/// A second apply does not create another stub directory or another `SKILL.md`.
/// A foreign path is refused and left in place. A dry-run writes nothing.
///
/// # Errors
///
/// Returns a failure when the path exists and is not ours, or when the stub
/// cannot be created or written.
pub fn setup(root: &Path, mode: WriteMode) -> Result<Report, SkillFailure> {
    let path = stub_dir(root);
    let disk = presence(&path)?;
    match (disk, mode) {
        (Presence::Foreign, _) => Err(SkillFailure::Foreign { path }),
        (Presence::Current, _) => Ok(Report {
            path,
            disk,
            mode,
            effect: Effect::Unchanged,
        }),
        (Presence::Missing | Presence::Stale, WriteMode::DryRun) => Ok(Report {
            path,
            disk,
            mode,
            effect: Effect::WouldWrite,
        }),
        (Presence::Missing | Presence::Stale, WriteMode::Apply) => {
            write_stub(&path)?;
            Ok(Report {
                path,
                disk: Presence::Current,
                mode,
                effect: Effect::Wrote,
            })
        }
    }
}

/// Delete the stub directory when it carries our marker.
///
/// [`Confirmation::No`] returns before any filesystem call. A path without the
/// marker is left in place. A dry-run writes nothing.
///
/// # Errors
///
/// Returns a failure when confirmation is [`Confirmation::No`], or when
/// deleting the stub directory fails.
pub fn remove(
    root: &Path,
    confirmation: Confirmation,
    mode: WriteMode,
) -> Result<Report, SkillFailure> {
    if confirmation == Confirmation::No {
        return Err(SkillFailure::NeedsYes);
    }
    let path = stub_dir(root);
    let disk = presence(&path)?;
    match (disk, mode) {
        (Presence::Missing, _) => Ok(Report {
            path,
            disk,
            mode,
            effect: Effect::Unchanged,
        }),
        (Presence::Foreign, _) => Ok(Report {
            path,
            disk,
            mode,
            effect: Effect::Left,
        }),
        (Presence::Current | Presence::Stale, WriteMode::DryRun) => Ok(Report {
            path,
            disk,
            mode,
            effect: Effect::WouldRemove,
        }),
        (Presence::Current | Presence::Stale, WriteMode::Apply) => {
            fs::remove_dir_all(&path).map_err(|source| {
                SkillFailure::io(
                    &path,
                    source,
                    "Close anything using the stub directory and retry with --yes.",
                )
            })?;
            Ok(Report {
                path,
                disk: Presence::Missing,
                mode,
                effect: Effect::Removed,
            })
        }
    }
}

/// Path of the `grok-router` directory under `root`.
fn stub_dir(root: &Path) -> PathBuf {
    root.join(STUB_DIR_NAME)
}

/// Whether `dir` is missing, current, stale, or foreign.
///
/// # Errors
///
/// Returns [`SkillFailure::Io`] when the directory cannot be inspected.
fn presence(dir: &Path) -> Result<Presence, SkillFailure> {
    let meta = match fs::symlink_metadata(dir) {
        Ok(meta) => meta,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(Presence::Missing),
        Err(source) => {
            return Err(SkillFailure::io(
                dir,
                source,
                "Pass --skill-root as a directory you can read.",
            ));
        }
    };
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Ok(Presence::Foreign);
    }
    let file = dir.join(STUB_FILE_NAME);
    match fs::read(&file) {
        Ok(bytes) if bytes == STUB.as_bytes() => Ok(Presence::Current),
        Ok(bytes) => match std::str::from_utf8(&bytes) {
            Ok(text) if has_marker(text) => Ok(Presence::Stale),
            _ => Ok(Presence::Foreign),
        },
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            if directory_is_empty(dir)? {
                Ok(Presence::Missing)
            } else {
                Ok(Presence::Foreign)
            }
        }
        Err(source) => Err(SkillFailure::io(
            &file,
            source,
            "Retry with a readable stub directory, or pass a different --skill-root.",
        )),
    }
}

/// Whether `text` contains the stub marker on its own line.
fn has_marker(text: &str) -> bool {
    text.lines().any(|line| line == MARKER)
}

/// Whether `dir` has no entries.
///
/// # Errors
///
/// Returns [`SkillFailure::Io`] when the directory cannot be listed.
fn directory_is_empty(dir: &Path) -> Result<bool, SkillFailure> {
    let mut entries = fs::read_dir(dir).map_err(|source| {
        SkillFailure::io(
            dir,
            source,
            "Retry with a readable --skill-root, or pass a different directory.",
        )
    })?;
    match entries.next() {
        None => Ok(true),
        Some(Ok(_)) => Ok(false),
        Some(Err(source)) => Err(SkillFailure::io(
            dir,
            source,
            "Retry with a readable --skill-root, or pass a different directory.",
        )),
    }
}

/// Create the stub directory and write `SKILL.md`.
///
/// # Errors
///
/// Returns [`SkillFailure::Io`] when the stub directory or file cannot be written.
fn write_stub(dir: &Path) -> Result<(), SkillFailure> {
    fs::create_dir_all(dir).map_err(|source| {
        SkillFailure::io(
            dir,
            source,
            "Create a writable --skill-root and retry. Nothing further was written.",
        )
    })?;
    let file = dir.join(STUB_FILE_NAME);
    fs::write(&file, STUB).map_err(|source| {
        SkillFailure::io(
            &file,
            source,
            "Fix permissions on --skill-root and retry the setup.",
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{
        Confirmation, Effect, MARKER, Presence, STUB, STUB_DIR_NAME, STUB_FILE_NAME, SkillFailure,
        WriteMode, guide, inspect, remove, setup,
    };

    const NEXT_SESSION: &str = "The saved default applies to the next session.";
    const COMMANDS: &str = "Commands: list, search, benchmarks, add, set-default, remove, skill.";
    const INVOCATION: &str = "grok-router __internal skill";

    #[test]
    fn guide_names_commands_next_session_and_omits_secrets() {
        let text = guide();
        assert!(
            text.contains(COMMANDS),
            "the guide should name list, search, benchmarks, add, set-default, remove, and skill; guide: {text}"
        );
        assert!(
            text.contains(NEXT_SESSION),
            "the guide should say the saved default applies to the next session; guide: {text}"
        );
        let lower = text.to_lowercase();
        assert!(
            !lower.contains("api key"),
            "the guide should not print an API key; guide: {text}"
        );
        assert!(
            !lower.contains("config file"),
            "the guide should not print a config file; guide: {text}"
        );
        assert!(
            !text.contains("settings.json"),
            "the guide should not name settings.json; guide: {text}"
        );
        assert!(
            !text.contains("hooks.json"),
            "the guide should not name hooks.json; guide: {text}"
        );
        assert!(
            !text.contains(MARKER),
            "the marker belongs on the stub, not in the guide; guide: {text}"
        );
    }

    #[test]
    fn stub_points_at_internal_skill_and_is_not_the_guide() {
        assert_eq!(STUB_DIR_NAME, "grok-router");
        assert_eq!(STUB_FILE_NAME, "SKILL.md");
        assert_eq!(MARKER, "grok-router-skill-stub");
        assert!(
            STUB.contains(INVOCATION),
            "the stub body should tell the agent to run `{INVOCATION}`; stub: {STUB}"
        );
        assert!(
            STUB.lines().any(|line| line == MARKER),
            "the stub should carry the marker on its own line; stub: {STUB}"
        );
        assert!(
            !STUB.contains(NEXT_SESSION),
            "the long guide should not be pasted into the stub; stub: {STUB}"
        );
        assert_ne!(STUB, guide());
        assert!(
            !STUB.contains("settings.json") && !STUB.contains("hooks.json"),
            "the stub should not be a hook or a settings.json edit; stub: {STUB}"
        );
    }

    #[test]
    fn setup_writes_one_stub_and_second_setup_does_not_write() {
        let root = temp_root("setup");
        let report = setup(&root, WriteMode::Apply).expect("the first setup should write the stub");
        assert_eq!(report.effect, Effect::Wrote);
        assert_eq!(report.disk, Presence::Current);
        assert_eq!(names(&root), vec!["grok-router".to_owned()]);
        let stub = root.join("grok-router");
        assert_eq!(names(&stub), vec!["SKILL.md".to_owned()]);
        let file = stub.join("SKILL.md");
        let bytes = fs::read(&file).expect("SKILL.md should be readable after setup");
        assert_eq!(bytes, STUB.as_bytes());

        let mut perms = fs::metadata(&stub)
            .expect("the stub directory should have metadata")
            .permissions();
        perms.set_readonly(true);
        fs::set_permissions(&stub, perms).expect("the stub directory should become read-only");
        let again = setup(&root, WriteMode::Apply)
            .expect("a second setup should not need to write the read-only stub");
        assert_eq!(again.effect, Effect::Unchanged);
        assert_eq!(again.disk, Presence::Current);
        let after = fs::read(&file).expect("SKILL.md should still be readable");
        assert_eq!(
            after, bytes,
            "a second setup should leave the one stub in place"
        );
        assert_eq!(names(&root), vec!["grok-router".to_owned()]);
    }

    #[test]
    fn dry_run_setup_and_unconfirmed_remove_write_nothing() {
        let root = temp_root("dry");
        let preview = setup(&root, WriteMode::DryRun).expect("dry-run setup should succeed");
        assert_eq!(preview.effect, Effect::WouldWrite);
        assert_eq!(preview.disk, Presence::Missing);
        assert!(
            !root
                .join("grok-router")
                .try_exists()
                .expect("the missing stub path should be stat-able"),
            "dry-run should not create the stub directory"
        );

        let refused = match remove(&root, Confirmation::No, WriteMode::Apply) {
            Ok(report) => panic!("remove without --yes should refuse; report: {report:?}"),
            Err(err) => err,
        };
        assert_eq!(refused.kind(), "refused");
        assert!(
            matches!(refused, SkillFailure::NeedsYes),
            "the refusal should happen before a filesystem call; err: {refused}"
        );
        assert_eq!(names(&root), Vec::<String>::new());
    }

    #[test]
    fn remove_yes_deletes_only_the_marked_directory() {
        let root = temp_root("remove-yes");
        let installed =
            setup(&root, WriteMode::Apply).expect("setup should install the stub before remove");
        assert_eq!(installed.effect, Effect::Wrote);
        let sibling = root.join("notes");
        fs::create_dir_all(&sibling).expect("a sibling directory should be created");
        fs::write(sibling.join("keep.txt"), b"keep\n").expect("a sibling file should be written");
        let preview = remove(&root, Confirmation::Yes, WriteMode::DryRun)
            .expect("dry-run remove should preview the delete");
        assert_eq!(preview.effect, Effect::WouldRemove);
        assert!(
            root.join("grok-router")
                .join("SKILL.md")
                .try_exists()
                .expect("the stub should still exist after dry-run"),
            "dry-run remove should not delete the stub"
        );

        let deleted = remove(&root, Confirmation::Yes, WriteMode::Apply)
            .expect("remove --yes should delete our stub");
        assert_eq!(deleted.effect, Effect::Removed);
        assert_eq!(deleted.disk, Presence::Missing);
        assert!(
            !root
                .join("grok-router")
                .try_exists()
                .expect("the deleted stub path should be stat-able"),
            "remove --yes should delete the marked stub directory"
        );
        let kept =
            fs::read(sibling.join("keep.txt")).expect("the sibling file should still be readable");
        assert_eq!(kept, b"keep\n");
    }

    #[test]
    fn unmarked_directory_is_left_in_place() {
        let root = temp_root("foreign");
        let foreign = root.join("grok-router");
        fs::create_dir_all(&foreign).expect("a foreign stub directory should be created");
        let file = foreign.join("SKILL.md");
        fs::write(&file, b"not ours\n").expect("a foreign SKILL.md should be written");
        fs::write(foreign.join("extra.txt"), b"extra\n")
            .expect("a foreign extra file should be written");

        let setup_err = match setup(&root, WriteMode::Apply) {
            Ok(report) => {
                panic!("setup should refuse a directory without the marker; report: {report:?}")
            }
            Err(err) => err,
        };
        assert_eq!(setup_err.kind(), "refused");
        assert_eq!(
            fs::read(&file).expect("the foreign file should still be readable"),
            b"not ours\n"
        );
        assert_eq!(names(&root), vec!["grok-router".to_owned()]);

        let left = remove(&root, Confirmation::Yes, WriteMode::Apply)
            .expect("remove should succeed by leaving the foreign directory");
        assert_eq!(left.effect, Effect::Left);
        assert_eq!(left.disk, Presence::Foreign);
        assert_eq!(
            fs::read(foreign.join("extra.txt")).expect("the extra file should still be readable"),
            b"extra\n"
        );
    }

    #[test]
    fn empty_stub_directory_can_be_filled_without_a_second_copy() {
        let root = temp_root("empty");
        fs::create_dir_all(root.join("grok-router"))
            .expect("an empty stub directory should be created");
        let looked = inspect(&root).expect("an empty stub directory should be readable");
        assert_eq!(looked.disk, Presence::Missing);
        let report = setup(&root, WriteMode::Apply).expect("setup should fill the empty directory");
        assert_eq!(report.effect, Effect::Wrote);
        assert_eq!(
            names(&root.join("grok-router")),
            vec!["SKILL.md".to_owned()]
        );
    }

    #[test]
    fn stale_stub_is_rewritten_in_place() {
        let root = temp_root("stale");
        let dir = root.join("grok-router");
        fs::create_dir_all(&dir).expect("the stale stub directory should be created");
        let file = dir.join("SKILL.md");
        fs::write(&file, format!("{MARKER}\nold body\n")).expect("a stale stub should be written");
        let report = setup(&root, WriteMode::Apply).expect("setup should refresh a stale stub");
        assert_eq!(report.effect, Effect::Wrote);
        assert_eq!(report.disk, Presence::Current);
        assert_eq!(names(&dir), vec!["SKILL.md".to_owned()]);
        let bytes = fs::read(&file).expect("the refreshed stub should be readable");
        assert_eq!(bytes, STUB.as_bytes());
    }

    fn temp_root(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "grok-router-skill-lib-{label}-{}",
            std::process::id()
        ));
        remove_temp(&path);
        fs::create_dir_all(&path).expect("the temp skill root should be created");
        path
    }

    fn remove_temp(path: &Path) {
        make_writable(path);
        match fs::remove_dir_all(path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => panic!(
                "the temp directory {} should be removable: {err}",
                path.display()
            ),
        }
    }

    fn make_writable(path: &Path) {
        let Ok(meta) = fs::symlink_metadata(path) else {
            return;
        };
        let mut perms = meta.permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(perms.mode() | 0o200);
        }
        #[cfg(not(unix))]
        perms.set_readonly(false);
        match fs::set_permissions(path, perms) {
            Ok(()) | Err(_) => {}
        }
        if !meta.is_dir() {
            return;
        }
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            make_writable(&entry.path());
        }
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut found = Vec::new();
        let entries = fs::read_dir(dir).expect("the fixture directory should be readable");
        for entry in entries {
            let entry = entry.expect("a fixture entry should be readable");
            found.push(
                entry
                    .file_name()
                    .into_string()
                    .expect("fixture names should be utf-8"),
            );
        }
        found.sort();
        found
    }
}
