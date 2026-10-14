// RULOGMAN PATCH: keep the relaunch helper independent of AppKit so its process
// handoff can also be exercised on Linux with `rustc --test restart.rs`.
use std::{
    ffi::OsString,
    os::unix::process::CommandExt as _,
    path::Path,
    process::{Command, Stdio},
};

// A dead PID does not mean LaunchServices has forgotten the old application.
// Request a new instance of the exact installed bundle, and retry temporary
// launch failures while the old registration is being torn down.
const SCRIPT: &str = r#"
    while kill -0 "$0" 2>/dev/null; do
        /bin/sleep 0.1
    done
    opener="$1"
    app_path="$2"
    shift 2
    case "$app_path" in
        *.[aA][pP][pP])
            if [ "$#" -gt 0 ]; then
                set -- --args "$@"
            fi
            for attempt in 1 2 3 4 5; do
                "$opener" -n -a "$app_path" "$@" && exit 0
                status=$?
                if [ "$attempt" -lt 5 ]; then
                    /bin/sleep 0.2
                fi
            done
            exit "$status"
            ;;
        *) exec "$app_path" "$@" ;;
    esac
"#;

pub(crate) fn command(pid: u32, app_path: &Path, arguments: &[OsString]) -> Command {
    command_with_opener(pid, app_path, arguments, Path::new("/usr/bin/open"))
}

fn command_with_opener(
    pid: u32,
    app_path: &Path,
    arguments: &[OsString],
    opener: &Path,
) -> Command {
    #[allow(
        clippy::disallowed_methods,
        reason = "The relaunch helper must outlive the application"
    )]
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(SCRIPT)
        .arg(pid.to_string())
        .arg(opener)
        .arg(app_path)
        .args(arguments)
        // Do not retain a directory inside a bundle the updater renamed, or
        // pipes owned by the process that is about to exit.
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::PermissionsExt as _,
        path::PathBuf,
        sync::atomic::{AtomicU32, Ordering},
        thread,
        time::{Duration, Instant},
    };

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let root = std::env::temp_dir().join(format!(
                "gpui-restart-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }

        fn script(&self, name: &str, body: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            path
        }

        fn launch(&self, pid: u32, path: &Path, args: &[OsString]) -> Command {
            let opener = self.script(
                "open",
                r#"
                # Model a stale LaunchServices registration: ordinary open
                # reports success without starting a new process.
                [ "$1" = -n ] || exit 0
                printf 'attempt\n' >> "$RESTART_ATTEMPTS"
                attempts=$(wc -l < "$RESTART_ATTEMPTS")
                [ "$attempts" -gt "${RESTART_FAILURES:-0}" ] || exit 17
                printf '%s\000' "$@" > "$RESTART_RESULT"
                "#,
            );
            let mut command = command_with_opener(pid, path, args, &opener);
            command
                .env("RESTART_ATTEMPTS", self.0.join("attempts"))
                .env("RESTART_RESULT", self.0.join("result"));
            command
        }

        fn result(&self) -> Vec<u8> {
            fs::read(self.0.join("result")).unwrap()
        }

        fn attempts(&self) -> usize {
            fs::read_to_string(self.0.join("attempts"))
                .unwrap()
                .lines()
                .count()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn exited_pid() -> u32 {
        let mut child = Command::new("/usr/bin/true").spawn().unwrap();
        let pid = child.id();
        assert!(child.wait().unwrap().success());
        pid
    }

    fn wait(child: &mut std::process::Child) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                return status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("relaunch helper did not finish");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn launches_a_fresh_instance_of_each_updated_bundle() {
        for name in ["rulogman", "rudbman", "rudbgen"] {
            let fixture = Fixture::new();
            let path = PathBuf::from(format!("/Applications/{name}.app"));
            let mut child = fixture.launch(exited_pid(), &path, &[]).spawn().unwrap();
            assert!(wait(&mut child).success());
            assert_eq!(
                fixture.result(),
                format!("-n\0-a\0{}\0", path.display()).as_bytes()
            );
            assert_eq!(fixture.attempts(), 1);
        }
    }

    #[test]
    fn waits_for_the_old_process_before_relaunching() {
        let fixture = Fixture::new();
        let mut old = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let mut helper = fixture
            .launch(old.id(), Path::new("/Applications/rulogman.app"), &[])
            .spawn()
            .unwrap();
        thread::sleep(Duration::from_millis(250));
        let premature_launch = fixture.0.join("result").exists();
        old.kill().unwrap();
        old.wait().unwrap();
        assert!(wait(&mut helper).success());
        assert!(!premature_launch);
        assert!(!fixture.result().is_empty());
    }

    #[test]
    fn preserves_paths_and_arguments_without_shell_expansion() {
        let fixture = Fixture::new();
        let path = Path::new("/Applications/한글 user's $(false).app");
        let args = ["--workspace", "a b", "'\"$()`\\", "", "한글"];
        let mut child = fixture
            .launch(exited_pid(), path, &args.map(OsString::from))
            .spawn()
            .unwrap();
        assert!(wait(&mut child).success());
        let expected = ["-n", "-a", path.to_str().unwrap(), "--args"]
            .into_iter()
            .chain(args)
            .collect::<Vec<_>>()
            .join("\0")
            + "\0";
        assert_eq!(fixture.result(), expected.as_bytes());
    }

    #[test]
    fn retries_transient_launch_failures() {
        let fixture = Fixture::new();
        let mut child = fixture
            .launch(exited_pid(), Path::new("/Applications/rudbman.app"), &[])
            .env("RESTART_FAILURES", "2")
            .spawn()
            .unwrap();
        assert!(wait(&mut child).success());
        assert_eq!(fixture.attempts(), 3);
        assert!(!fixture.result().is_empty());
    }

    #[test]
    fn permanent_failure_is_bounded_and_keeps_the_error_status() {
        let fixture = Fixture::new();
        let mut child = fixture
            .launch(exited_pid(), Path::new("/Applications/rudbgen.app"), &[])
            .env("RESTART_FAILURES", "99")
            .spawn()
            .unwrap();
        assert_eq!(wait(&mut child).code(), Some(17));
        assert_eq!(fixture.attempts(), 5);
        assert!(!fixture.0.join("result").exists());
    }

    #[test]
    fn executes_unbundled_builds_directly() {
        let fixture = Fixture::new();
        let binary = fixture.script("bare app", "printf '%s' \"$1\" > \"$RESTART_RESULT\"");
        let mut child = command(exited_pid(), &binary, &[OsString::from("hello world")])
            .env("RESTART_RESULT", fixture.0.join("result"))
            .spawn()
            .unwrap();
        assert!(wait(&mut child).success());
        assert_eq!(fixture.result(), b"hello world");
    }
}
