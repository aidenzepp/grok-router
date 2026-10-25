#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    reason = "these files are integration-test crates, so clippy.toml allow-expect-in-tests and allow-panic-in-tests do not see the helpers"
)]
#![allow(
    clippy::cargo_common_metadata,
    clippy::multiple_crate_versions,
    reason = "license is unset, and windows-sys 0.52 and 0.61 are transitive"
)]

//! `grok-router skill` installs a short stub and prints the guide.
//!
//! These tests pass `--skill-root` and point `HOME` at a sentinel directory.
//! They do not read or write the real home directory.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const INVOCATION: &str = "grok-router __internal skill";
const NEXT_SESSION: &str = "The saved default applies to the next session.";
const COMMANDS: &str = "Commands: list, search, benchmarks, add, set-default, remove, skill.";
const MARKER: &str = "grok-router-skill-stub";

#[derive(Debug, PartialEq, Eq)]
enum Node {
    /// A directory.
    Dir,
    /// A file and its bytes.
    File(Vec<u8>),
}

struct Fixture {
    dir: PathBuf,
    home: PathBuf,
    skills: PathBuf,
    skills_arg: String,
}

#[test]
fn setup_status_show_and_internal_guide_agree() {
    let fixture = Fixture::open("happy");
    let setup = fixture.run(&[
        "skill",
        "setup",
        "--skill-root",
        &fixture.skills_arg,
        "--json",
    ]);
    assert_eq!(
        exit_code(&setup),
        0,
        "setup should succeed; stderr: {}",
        stderr_text(&setup)
    );
    assert!(
        stderr_text(&setup).is_empty(),
        "setup should keep diagnostics off stdout and stderr empty on success; stderr: {}",
        stderr_text(&setup)
    );
    let setup_json = json_stdout(&setup);
    assert_eq!(json_field(&setup_json, "status"), "current");
    assert_eq!(json_field(&setup_json, "effect"), "wrote");
    assert_eq!(json_field(&setup_json, "dry_run"), false);

    let file = fixture.skills.join("grok-router").join("SKILL.md");
    let body = fs::read_to_string(&file).expect("setup should write SKILL.md");
    assert!(
        body.contains(INVOCATION),
        "the stub body should tell the agent to run `{INVOCATION}`; body: {body}"
    );
    assert!(
        body.lines().any(|line| line == MARKER),
        "the stub should carry our marker; body: {body}"
    );
    assert!(
        !body.contains(NEXT_SESSION),
        "the stub should not contain the long guide; body: {body}"
    );
    assert_no_hook_or_settings(&fixture.skills);

    let status = fixture.run(&[
        "skill",
        "status",
        "--skill-root",
        &fixture.skills_arg,
        "--quiet",
    ]);
    assert_eq!(exit_code(&status), 0, "stderr: {}", stderr_text(&status));
    assert_eq!(stdout_text(&status), "current\n");

    let status_json = fixture.run(&[
        "skill",
        "status",
        "--skill-root",
        &fixture.skills_arg,
        "--output",
        "json",
    ]);
    assert_eq!(
        exit_code(&status_json),
        0,
        "stderr: {}",
        stderr_text(&status_json)
    );
    assert_eq!(json_field(&json_stdout(&status_json), "status"), "current");

    let show = fixture.run(&["skill", "show"]);
    let internal = fixture.run(&["__internal", "skill"]);
    assert_eq!(exit_code(&show), 0, "stderr: {}", stderr_text(&show));
    assert_eq!(
        exit_code(&internal),
        0,
        "stderr: {}",
        stderr_text(&internal)
    );
    let guide = stdout_text(&show);
    assert_eq!(
        guide,
        stdout_text(&internal),
        "skill show and __internal skill should print the same guide"
    );
    assert!(
        guide.contains(COMMANDS),
        "the guide should name the commands; guide: {guide}"
    );
    assert!(
        guide.contains(NEXT_SESSION),
        "the guide should say the saved default applies to the next session; guide: {guide}"
    );
    let lower = guide.to_lowercase();
    assert!(!lower.contains("api key"), "guide: {guide}");
    assert!(!lower.contains("config file"), "guide: {guide}");
    assert_no_hook_or_settings(&fixture.skills);
}

#[test]
fn second_setup_does_not_write_a_second_stub() {
    let fixture = Fixture::open("second");
    let first = fixture.run(&["skill", "setup", "--skill-root", &fixture.skills_arg]);
    assert_eq!(
        exit_code(&first),
        0,
        "the first setup should succeed; stderr: {}",
        stderr_text(&first)
    );
    let before = fs::read(fixture.skills.join("grok-router").join("SKILL.md"))
        .expect("the first stub should be readable");
    let dir = fixture.skills.join("grok-router");
    let mut perms = fs::metadata(&dir)
        .expect("the stub directory should have metadata")
        .permissions();
    perms.set_readonly(true);
    fs::set_permissions(&dir, perms).expect("the stub directory should become read-only");

    let again = fixture.run(&[
        "skill",
        "setup",
        "--skill-root",
        &fixture.skills_arg,
        "--json",
    ]);
    assert_eq!(
        exit_code(&again),
        0,
        "a second setup should not write; stderr: {}",
        stderr_text(&again)
    );
    assert_eq!(json_field(&json_stdout(&again), "effect"), "unchanged");
    assert_eq!(json_field(&json_stdout(&again), "status"), "current");
    let after = fs::read(fixture.skills.join("grok-router").join("SKILL.md"))
        .expect("the stub should still be readable");
    assert_eq!(before, after);
    assert_eq!(child_names(&fixture.skills), vec!["grok-router".to_owned()]);
    assert_eq!(
        child_names(&fixture.skills.join("grok-router")),
        vec!["SKILL.md".to_owned()]
    );
}

#[test]
fn remove_yes_deletes_only_a_marked_stub_directory() {
    let fixture = Fixture::open("remove-yes");
    let installed = fixture.run(&["skill", "setup", "--skill-root", &fixture.skills_arg]);
    assert_eq!(
        exit_code(&installed),
        0,
        "stderr: {}",
        stderr_text(&installed)
    );
    let notes = fixture.skills.join("notes");
    fs::create_dir_all(&notes).expect("a sibling directory should be created");
    fs::write(notes.join("keep.txt"), b"keep\n").expect("a sibling file should be written");

    let preview = fixture.run(&[
        "skill",
        "remove",
        "--skill-root",
        &fixture.skills_arg,
        "--yes",
        "--dry-run",
        "--json",
    ]);
    assert_eq!(exit_code(&preview), 0, "stderr: {}", stderr_text(&preview));
    assert_eq!(json_field(&json_stdout(&preview), "effect"), "would_remove");
    assert_eq!(json_field(&json_stdout(&preview), "dry_run"), true);
    assert!(
        fixture
            .skills
            .join("grok-router")
            .join("SKILL.md")
            .try_exists()
            .expect("the stub should still exist after dry-run"),
        "dry-run should not delete the stub"
    );

    let removed = fixture.run(&[
        "skill",
        "remove",
        "--skill-root",
        &fixture.skills_arg,
        "--yes",
    ]);
    assert_eq!(exit_code(&removed), 0, "stderr: {}", stderr_text(&removed));
    assert!(
        stdout_text(&removed).starts_with("removed\n"),
        "remove --yes should report removed; stdout: {}",
        stdout_text(&removed)
    );
    assert!(
        !fixture
            .skills
            .join("grok-router")
            .try_exists()
            .expect("the deleted stub path should be stat-able"),
        "remove --yes should delete the marked stub directory"
    );
    assert_eq!(
        fs::read(notes.join("keep.txt")).expect("the sibling file should remain"),
        b"keep\n"
    );
    assert_no_hook_or_settings(&fixture.skills);
}

#[test]
fn unmarked_directory_is_left_in_place() {
    let fixture = Fixture::open("foreign");
    let foreign = fixture.skills.join("grok-router");
    fs::create_dir_all(&foreign).expect("a foreign directory should be created");
    fs::write(foreign.join("SKILL.md"), b"not ours\n").expect("a foreign file should be written");
    fs::write(foreign.join("extra.txt"), b"extra\n").expect("an extra file should be written");
    let before = tree(&fixture.skills);

    let setup = fixture.run(&["skill", "setup", "--skill-root", &fixture.skills_arg]);
    assert_eq!(
        exit_code(&setup),
        1,
        "setup should refuse a foreign directory"
    );
    assert_eq!(
        tree(&fixture.skills),
        before,
        "setup should not replace a foreign directory"
    );

    let removed = fixture.run(&[
        "skill",
        "remove",
        "--skill-root",
        &fixture.skills_arg,
        "--yes",
        "--json",
    ]);
    assert_eq!(exit_code(&removed), 0, "stderr: {}", stderr_text(&removed));
    assert_eq!(json_field(&json_stdout(&removed), "effect"), "left");
    assert_eq!(tree(&fixture.skills), before);
}

#[test]
fn remove_without_yes_and_dry_run_change_nothing() {
    let fixture = Fixture::open("no-write");
    let absent = fixture.dir.join("absent");
    let dry_setup = fixture.run(&[
        "skill",
        "setup",
        "--skill-root",
        absent.to_str().expect("the absent path should be utf-8"),
        "--dry-run",
    ]);
    assert_eq!(
        exit_code(&dry_setup),
        0,
        "stderr: {}",
        stderr_text(&dry_setup)
    );
    assert!(
        stdout_text(&dry_setup).contains("would write"),
        "dry-run setup should say what it would write; stdout: {}",
        stdout_text(&dry_setup)
    );
    assert!(
        !absent
            .try_exists()
            .expect("the absent path should be stat-able"),
        "dry-run setup should not create the skill root"
    );

    let installed = fixture.run(&["skill", "setup", "--skill-root", &fixture.skills_arg]);
    assert_eq!(
        exit_code(&installed),
        0,
        "stderr: {}",
        stderr_text(&installed)
    );
    let before = tree(&fixture.skills);

    let refused = fixture.run(&[
        "skill",
        "remove",
        "--skill-root",
        &fixture.skills_arg,
        "--json",
    ]);
    assert_eq!(exit_code(&refused), 1);
    assert!(
        stdout_text(&refused).is_empty(),
        "a refusal should keep stdout empty; stdout: {}",
        stdout_text(&refused)
    );
    let err = stderr_json(&refused);
    assert_eq!(json_field(&err, "kind"), "refused");
    assert!(
        json_field(&err, "message")
            .as_str()
            .expect("the refusal message should be a string")
            .contains("--yes"),
        "the refusal should name --yes; stderr: {}",
        stderr_text(&refused)
    );
    assert_eq!(tree(&fixture.skills), before);

    let dry_refused = fixture.run(&[
        "skill",
        "remove",
        "--skill-root",
        &fixture.skills_arg,
        "--dry-run",
    ]);
    assert_eq!(exit_code(&dry_refused), 1);
    assert!(
        stderr_text(&dry_refused).contains("--yes"),
        "dry-run without --yes should still refuse; stderr: {}",
        stderr_text(&dry_refused)
    );
    assert_eq!(tree(&fixture.skills), before);
    assert_no_hook_or_settings(&fixture.skills);
}

#[test]
fn session_start_is_not_a_command_and_writes_nothing() {
    let fixture = Fixture::open("session");
    let before = tree(&fixture.skills);
    let skill = fixture.run(&[
        "skill",
        "session-start",
        "--skill-root",
        &fixture.skills_arg,
    ]);
    assert_ne!(exit_code(&skill), 0);
    assert_eq!(tree(&fixture.skills), before);
    let internal = fixture.run(&["__internal", "session-start"]);
    assert_ne!(exit_code(&internal), 0);
    assert_eq!(tree(&fixture.skills), before);
    let root_help = stdout_text(&fixture.run(&["--help"]));
    assert!(
        !root_help.contains("session-start"),
        "root help should not offer session-start; help: {root_help}"
    );
    assert!(
        !root_help.contains("__internal"),
        "__internal is not a human command and should stay off root help; help: {root_help}"
    );
}

#[test]
fn help_states_examples_and_whether_the_command_writes() {
    let fixture = Fixture::open("help");
    let skill = stdout_text(&succeed(&fixture, &["skill", "--help"]));
    assert!(
        skill.to_lowercase().contains("example"),
        "skill --help should contain an example; help: {skill}"
    );
    assert!(!skill.contains("session-start"), "help: {skill}");

    let setup = stdout_text(&succeed(&fixture, &["skill", "setup", "--help"]));
    assert!(
        setup.to_lowercase().contains("example"),
        "setup help should contain an example; help: {setup}"
    );
    assert!(
        setup.contains("--dry-run"),
        "setup help should show --dry-run; help: {setup}"
    );
    assert!(
        setup.to_lowercase().contains("writes"),
        "setup help should say that it writes; help: {setup}"
    );

    let status = stdout_text(&succeed(&fixture, &["skill", "status", "--help"]));
    assert!(
        status.to_lowercase().contains("does not modify"),
        "status help should say it does not modify anything; help: {status}"
    );
    let show = stdout_text(&succeed(&fixture, &["skill", "show", "--help"]));
    assert!(
        show.to_lowercase().contains("does not modify"),
        "show help should say it does not modify anything; help: {show}"
    );
    let remove = stdout_text(&succeed(&fixture, &["skill", "remove", "--help"]));
    assert!(
        remove.contains("--yes"),
        "remove help should show --yes; help: {remove}"
    );
    assert!(
        remove.contains("--dry-run"),
        "remove help should show --dry-run; help: {remove}"
    );
    assert_eq!(tree(&fixture.skills), BTreeMap::new());
}

impl Fixture {
    fn open(label: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("grok-router-skill-{label}-{}", std::process::id()));
        remove_temp(&dir);
        let home = dir.join("home");
        let skills = dir.join("skills");
        fs::create_dir_all(&home).expect("the home sentinel should be created");
        fs::create_dir_all(&skills).expect("the skill root should be created");
        let skills_arg = skills
            .to_str()
            .expect("the temp skill root should be utf-8")
            .to_owned();
        Self {
            dir,
            home,
            skills,
            skills_arg,
        }
    }

    /// `HOME` is a sentinel. The command must not write it.
    fn run(&self, args: &[&str]) -> Output {
        let output = Command::new(env!("CARGO_BIN_EXE_grok-router"))
            .args(args)
            .stdin(Stdio::null())
            .env("HOME", &self.home)
            .env_remove("GROK_HOME")
            .env_remove("OPENROUTER_API_KEY")
            .env_remove("GROK_ROUTER_OUTPUT")
            .env_remove("GROK_ROUTER_QUIET")
            .env_remove("GROK_ROUTER_TIMEOUT")
            .env("NO_COLOR", "1")
            .current_dir(&self.dir)
            .output()
            .expect("the grok-router binary should start");
        assert!(
            tree(&self.home).is_empty(),
            "the command should not write HOME; args: {args:?}; home: {:?}",
            tree(&self.home)
        );
        output
    }
}

fn succeed(fixture: &Fixture, args: &[&str]) -> Output {
    let output = fixture.run(args);
    assert_eq!(
        exit_code(&output),
        0,
        "{args:?} should succeed; stderr: {}",
        stderr_text(&output)
    );
    output
}

fn stdout_text(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("skill stdout should be UTF-8")
}

fn stderr_text(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("skill stderr should be UTF-8")
}

fn exit_code(output: &Output) -> i32 {
    output
        .status
        .code()
        .expect("the child should have an exit status")
}

fn json_stdout(output: &Output) -> serde_json::Value {
    serde_json::from_str(stdout_text(output).trim()).expect("stdout should be one JSON object")
}

fn json_field<'a>(value: &'a serde_json::Value, key: &str) -> &'a serde_json::Value {
    match value.get(key) {
        Some(field) => field,
        None => panic!("{key} should be present in {value}"),
    }
}

fn stderr_json(output: &Output) -> serde_json::Value {
    serde_json::from_str(stderr_text(output).trim()).expect("stderr should be one JSON object")
}

fn assert_no_hook_or_settings(root: &Path) {
    for path in tree(root).keys() {
        assert!(
            !path.ends_with("settings.json")
                && !path.ends_with("hooks.json")
                && !path.contains("hooks"),
            "the skill root should not contain a hook or settings.json; found {path}"
        );
    }
}

fn child_names(dir: &Path) -> Vec<String> {
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

fn tree(dir: &Path) -> BTreeMap<String, Node> {
    let mut out = BTreeMap::new();
    record_tree(dir, "", &mut out);
    out
}

fn record_tree(dir: &Path, prefix: &str, out: &mut BTreeMap<String, Node>) {
    let entries = fs::read_dir(dir).expect("the fixture directory should be readable");
    for entry in entries {
        let entry = entry.expect("a fixture entry should be readable");
        let name = entry
            .file_name()
            .into_string()
            .expect("fixture names should be utf-8");
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let file_type = entry
            .file_type()
            .expect("the fixture file type should be readable");
        if file_type.is_dir() {
            let previous = out.insert(rel.clone(), Node::Dir);
            assert!(previous.is_none(), "duplicate fixture path {rel}");
            record_tree(&entry.path(), &rel, out);
        } else {
            let bytes = fs::read(entry.path()).expect("the fixture file should be readable");
            let previous = out.insert(rel.clone(), Node::File(bytes));
            assert!(previous.is_none(), "duplicate fixture path {rel}");
        }
    }
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
