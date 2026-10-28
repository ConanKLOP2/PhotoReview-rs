//! Port of ShellProcessTests.cs::ShellProcess (W8-20a); always launches the real application.
#![allow(dead_code)] // The startup and exe test binaries use different parts of this fixture.
use pr_platform_win::ui_driver;
use pr_test_support::TempRoot;
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

const BOUND: Duration = Duration::from_secs(60);

pub struct ShellProcess {
    pub root: TempRoot,
    pub prefix: String,
    pub pid: u32,
    exit: Receiver<std::io::Result<ExitStatus>>,
    status: Option<ExitStatus>,
}

impl ShellProcess {
    pub fn start(args: &[String], prefix: Option<&str>) -> Self {
        let root = TempRoot::new("w8-20-real-exe");
        let prefix = prefix.map(str::to_owned).unwrap_or_else(|| {
            format!(
                "PhotoReviewRsTest{}",
                root.path().file_name().unwrap().to_string_lossy()
            )
        });
        let data = root.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        // Prevent first-run import from ever inspecting the C# configuration.
        std::fs::write(data.join("config.json"), "{}").unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_PhotoReviewRs"))
            .args(args)
            .env("PHOTOREVIEW_RS_DATA_ROOT", &data)
            .env("PHOTOREVIEW_RS_ISOLATE_CONFIG", "1")
            .env("PHOTOREVIEW_RS_INSTANCE_PREFIX", &prefix)
            .env("PHOTOREVIEW_RS_WARP", "1")
            .env("PHOTOREVIEW_RS_NOWARM", "1")
            .env("PHOTOREVIEW_RS_PERF_TRACE", root.path().join("perf"))
            .env_remove("PHOTOREVIEW_RS_DIAG_FORCE_LOG")
            .env_remove("PHOTOREVIEW_RS_FRAME_LOG")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start real PhotoReviewRs.exe");
        let pid = child.id();
        let (tx, exit) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(child.wait());
        });
        Self {
            root,
            prefix,
            pid,
            exit,
            status: None,
        }
    }

    pub fn count_events(&self, name: &str) -> usize {
        std::fs::read_dir(self.root.path().join("perf"))
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|entry| std::fs::read(entry.path()).ok())
            .map(|bytes| String::from_utf8_lossy(&bytes).matches(&format!(",{name},")).count())
            .sum()
    }

    pub fn wait_for(&mut self, what: &str, condition: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + BOUND;
        loop {
            assert!(self.status.is_none(), "child exited before {what}: {:?}", self.status);
            if condition(self) {
                return;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "timed out waiting for {what}, pid={}", self.pid);
            // No external notification exists for CSV writes or a foreign HWND. An exit cuts this tick short.
            match self.exit.recv_timeout(remaining.min(Duration::from_millis(200))) {
                Ok(status) => self.status = Some(status.expect("wait child")),
                Err(RecvTimeoutError::Timeout) => {}
                Err(error) => panic!("child waiter disconnected: {error}"),
            }
        }
    }

    pub fn wait_exit(&mut self) -> ExitStatus {
        if let Some(status) = self.status {
            return status;
        }
        let status = self
            .exit
            .recv_timeout(BOUND)
            .expect("child exit within 60 seconds")
            .expect("wait child");
        self.status = Some(status);
        status
    }

    pub fn close(&mut self) {
        self.wait_for("main window close request", |p| {
            ui_driver::close_main_window_of_process(p.pid)
        });
        assert_eq!(self.wait_exit().code(), Some(0));
    }
}

impl Drop for ShellProcess {
    fn drop(&mut self) {
        if self.status.is_none() && self.exit.try_recv().is_err() {
            // Only the exact child PID created above; never kill by image name or walk a process tree.
            let _ = Command::new("taskkill")
                .args(["/PID", &self.pid.to_string(), "/F"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let _ = self.exit.recv_timeout(Duration::from_secs(10));
        }
    }
}

pub fn photos() -> (TempRoot, PathBuf, PathBuf) {
    let root = TempRoot::new("w8-20-process-photos");
    let bytes = std::fs::read(pr_test_support::golden::repo_root().join("tests/fixtures/turbojpeg/parity-400x300.jpg"))
        .unwrap();
    let a = root.path().join("a.jpg");
    let b = root.path().join("b.jpg");
    std::fs::write(&a, &bytes).unwrap();
    std::fs::write(&b, &bytes).unwrap();
    (root, a, b)
}
