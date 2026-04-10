// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Module for running an isolated `svc.configd` instance.
//!
//! This makes use of several undocumented and uncommitted interfaces, including
//! at least:
//!
//! * Command line flags to run `svc.configd` as not root, not daemonized, and
//!   pointed to a different door and repository
//! * Environment variables that tell `svccfg` to point to a different
//!   `svc.configd` door and repository

use camino::Utf8Path;
use camino::Utf8PathBuf;
use camino_tempfile::Utf8TempDir;
use std::collections::BTreeSet;
use std::fs::File;
use std::io;
use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::process::Child;
use std::process::Command;
use std::process::Output;
use std::process::Stdio;
use std::thread;
use std::time::Duration;
use std::time::Instant;

// When we try to kill `svc.configd` via `SIGINT`, how long do we wait before we
// give up and send `SIGKILL`?
const SIGINT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

// When we start `svc.configd`, how long are we willing to wait for the door to
// show up?
const SVC_CONFIGD_DOOR_CREATE_TIMEOUT: Duration = Duration::from_secs(10);

pub struct IsolatedConfigd {
    dir: Utf8TempDir,
    // This is always `dir.join(IsolatedConfigdBuilder::DOOR_FILENAME)`, but we
    // keep it cached here so we can return a `&Utf8Path` when asked.
    door_path: Utf8PathBuf,
    configd_child: KillOnDrop,
}

impl Drop for IsolatedConfigd {
    fn drop(&mut self) {
        // Ensure the child has shut down, but ignore errors - we can't report
        // them and don't want to double panic. The child will also shutdown on
        // drop, but we want to ensure that happens before we drop `dir`.
        _ = self.configd_child.shutdown();
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IsolatedConfigdRefreshError {
    #[error("failed to exec `svccfg -s {fmri} refresh`")]
    SvccfgRefreshExec {
        fmri: String,
        #[source]
        err: io::Error,
    },
    #[error("error during `svccfg -s {fmri} refresh`: {err}")]
    SvccfgRefreshError { fmri: String, err: String },
}

#[derive(Debug, thiserror::Error)]
pub enum IsolatedConfigdShutdownError {
    #[error("failed to kill svc.configd child process")]
    ConfigdKill(#[source] io::Error),
    #[error("failed to wait on killed svc.configd child process")]
    ConfigdWaitAfterKill(#[source] io::Error),
}

impl IsolatedConfigd {
    pub fn builder(
        service: impl Into<String>,
    ) -> Result<IsolatedConfigdBuilder, InvalidFakeServiceName> {
        let builder = IsolatedConfigdBuilder { services: BTreeSet::new() };
        builder.add_service(service)
    }

    /// Path to the temporary directory containing the internal guts of this
    /// isolated `svc.configd` instance.
    pub fn path(&self) -> &Utf8Path {
        self.dir.path()
    }

    /// Send an `svccfg -s {fmri} refresh` to this isolated `svc.configd`.
    ///
    /// We expose this method because the `libscf` function
    /// `smf_refresh_instance()` expects to be talking to the real, functional
    /// `svc.configd`. `svccfg refresh` uses a different path when talking to an
    /// isolated instance where it directly modifies the instance's snapshots,
    /// which _does_ work with an isolated `svc.configd`.
    pub fn refresh(
        &self,
        fmri: &str,
    ) -> Result<(), IsolatedConfigdRefreshError> {
        let output = Command::new("svccfg")
            .env("SVCCFG_DOOR", self.door_path().as_str())
            .args(["-s", fmri])
            .arg("refresh")
            .output()
            .map_err(|err| IsolatedConfigdRefreshError::SvccfgRefreshExec {
                fmri: fmri.to_owned(),
                err,
            })?;
        check_command_output(output).map_err(|err| {
            IsolatedConfigdRefreshError::SvccfgRefreshError {
                fmri: fmri.to_owned(),
                err,
            }
        })
    }

    pub fn door_path(&self) -> &Utf8Path {
        &self.door_path
    }

    /// Attempt to shut down this isolated `svc.configd`.
    ///
    /// This will send the process a `SIGINT`, wait briefly for it to exit, then
    /// if it's still running, send a `SIGKILL` then `wait()`. If this method is
    /// not called before this `IsolatedConfigd` is dropped, the `drop()`
    /// implementation will also attempt to shut it down (but any errors will
    /// not be visible).
    pub fn shutdown(mut self) -> Result<(), IsolatedConfigdShutdownError> {
        self.configd_child.shutdown()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IsolatedConfigdBuildError {
    #[error("failed to create temp directory")]
    CreateTempDir(#[source] io::Error),

    #[error("failed creating fake service manifest file `{path}`")]
    FakeServiceManifestCreate {
        path: Utf8PathBuf,
        #[source]
        err: io::Error,
    },

    #[error("failed writing to fake service manifest file `{path}`")]
    FakeServiceManifestWrite {
        path: Utf8PathBuf,
        #[source]
        err: io::Error,
    },

    #[error("failed to exec `svccfg import {path}`")]
    SvccfgImportExec {
        path: Utf8PathBuf,
        #[source]
        err: io::Error,
    },

    #[error("error during `svccfg import {path}`: {err}")]
    SvccfgImportError { path: Utf8PathBuf, err: String },

    #[error("failed to exec `svc.configd` pointed at the isolated repo")]
    SvcConfigdExec(#[source] io::Error),

    #[error("failed creating svc.configd output file `{path}`")]
    SvcConfigdCreateOutputFile {
        path: Utf8PathBuf,
        #[source]
        err: io::Error,
    },

    // Caller will need to hold on to the `tempdir` to actually inspect the
    // contents, or call `.keep()` on it. We don't want to do that by default to
    // avoid leaving behind detritus.
    #[error(
        "svc.configd did not create door file; consider inspecting \
         contents of `{}`",
         .tempdir.path(),
    )]
    SvcConfigdNoDoor { tempdir: Utf8TempDir },
}

#[derive(Debug, thiserror::Error)]
#[error("invalid fake service name: {0:?}")]
pub struct InvalidFakeServiceName(pub String);

pub struct IsolatedConfigdBuilder {
    services: BTreeSet<String>,
}

impl IsolatedConfigdBuilder {
    const DOOR_FILENAME: &str = "door";
    const REPO_FILENAME: &str = "repo";

    pub fn add_service(
        mut self,
        service: impl Into<String>,
    ) -> Result<Self, InvalidFakeServiceName> {
        // Do some _very basic_ validation on `service` to ensure we don't have
        // any XML injection. We only expect tests to want service names like
        // "foo/bar/baz", so we just ensure that the name contains only
        // alphanumbers plus `-`, `_`, and `/`.
        let service = service.into();
        if service.is_empty()
            || !service.chars().all(|c| {
                c.is_alphanumeric() || c == '-' || c == '_' || c == '/'
            })
        {
            return Err(InvalidFakeServiceName(service.to_string()));
        }

        self.services.insert(service);
        Ok(self)
    }

    pub fn build(self) -> Result<IsolatedConfigd, IsolatedConfigdBuildError> {
        let dir = Utf8TempDir::new()
            .map_err(IsolatedConfigdBuildError::CreateTempDir)?;
        self.build_with_tempdir(dir)
    }

    pub fn build_in<P: AsRef<Utf8Path>>(
        self,
        dir: P,
    ) -> Result<IsolatedConfigd, IsolatedConfigdBuildError> {
        let dir = Utf8TempDir::new_in(dir)
            .map_err(IsolatedConfigdBuildError::CreateTempDir)?;
        self.build_with_tempdir(dir)
    }

    fn build_with_tempdir(
        self,
        dir: Utf8TempDir,
    ) -> Result<IsolatedConfigd, IsolatedConfigdBuildError> {
        let door_path = dir.path().join(Self::DOOR_FILENAME);
        let repo_path = dir.path().join(Self::REPO_FILENAME);

        // Set up the isolated repo by running `SVCCFG_REPOSITORY={repo_path}
        // svccfg import manifest.xml` for each of the requested fake services.
        for (i, service) in self.services.iter().enumerate() {
            let path = dir.path().join(format!("fake-service-{i}.xml"));
            write_service_manifest(service, &path)?;

            let output = Command::new("svccfg")
                .env("SVCCFG_REPOSITORY", &repo_path)
                .args(["import", path.as_str()])
                .output()
                .map_err(|err| IsolatedConfigdBuildError::SvccfgImportExec {
                    path: path.clone(),
                    err,
                })?;
            check_command_output(output).map_err(|err| {
                IsolatedConfigdBuildError::SvccfgImportError {
                    path: path.clone(),
                    err,
                }
            })?;
        }

        // Redirect the `svc.configd` instance's stdout/stderr to files within
        // our temp directory.
        let stdout_path = dir.path().join("svc.configd.stdout");
        let stdout_f = File::create_new(&stdout_path).map_err(|err| {
            IsolatedConfigdBuildError::SvcConfigdCreateOutputFile {
                path: stdout_path.to_owned(),
                err,
            }
        })?;
        let stderr_path = dir.path().join("svc.configd.stderr");
        let stderr_f = File::create_new(&stderr_path).map_err(|err| {
            IsolatedConfigdBuildError::SvcConfigdCreateOutputFile {
                path: stderr_path.to_owned(),
                err,
            }
        })?;

        // Spawn a `svc.configd` pointed to our isolated repo.
        let mut configd_child = Command::new("/lib/svc/bin/svc.configd")
            .arg("-n") // don't daemonize
            .args(["-d", door_path.as_str()])
            .args(["-r", repo_path.as_str()])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(IsolatedConfigdBuildError::SvcConfigdExec)?;

        // Spawn threads to copy data from the pipes to the files.
        //
        // We let these threads detach; we can't really do anything if they fail
        // anyway.
        let child_stdout = configd_child
            .stdout
            .take()
            .expect("child configured with piped stdout");
        thread::spawn(move || {
            let mut stdout_f = io::BufWriter::new(stdout_f);
            let mut child_stdout = io::BufReader::new(child_stdout);
            _ = io::copy(&mut child_stdout, &mut stdout_f);
        });
        let child_stderr = configd_child
            .stderr
            .take()
            .expect("child configured with piped stderr");
        thread::spawn(move || {
            let mut stderr_f = io::BufWriter::new(stderr_f);
            let mut child_stderr = io::BufReader::new(child_stderr);
            _ = io::copy(&mut child_stderr, &mut stderr_f);
        });

        // Wait for `svc.configd` to create the door.
        let wait_for_door_start = Instant::now();
        let mut configd_child = KillOnDrop(Some(configd_child));
        loop {
            if door_path.exists() {
                return Ok(IsolatedConfigd { dir, door_path, configd_child });
            }

            // Is svc.configd still running?
            if configd_child.has_exited() {
                return Err(IsolatedConfigdBuildError::SvcConfigdNoDoor {
                    tempdir: dir,
                });
            }

            // Are we past the deadline?
            if wait_for_door_start.elapsed() >= SVC_CONFIGD_DOOR_CREATE_TIMEOUT
            {
                return Err(IsolatedConfigdBuildError::SvcConfigdNoDoor {
                    tempdir: dir,
                });
            }

            // Sleep briefly then check again.
            thread::sleep(Duration::from_millis(100));
        }
    }
}

fn check_command_output(output: Output) -> Result<(), String> {
    if output.status.success() {
        return Ok(());
    }

    let mut err = match (output.status.code(), output.status.signal()) {
        (Some(code), _) => format!("(exited {code}): "),
        (_, Some(signal)) => format!("(exited with signal {signal}): "),
        _ => String::new(),
    };
    if !output.stdout.is_empty() {
        err.push_str(&String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() {
        if !err.is_empty() {
            err.push('\n');
        }
        err.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    if err.is_empty() {
        err.push_str("(no output from stdout or stderr!)");
    }
    Err(err)
}

// "Best effort" try to kill a child process via SIGINT:
//
// 1. Send `SIGINT`
// 2. Check for exit status periodically until `wait_time`
//
// On any error or timeout elapsed, returns the `Child` handle; any errors are
// swallowed.
fn try_kill_via_sigint(
    mut child: Child,
    wait_time: Duration,
) -> Result<(), Child> {
    // Send SIGINT.
    let Ok(pid) = i32::try_from(child.id()) else {
        return Err(child);
    };
    let sig = libc::SIGINT;
    // SAFETY: We're sending SIGINT to a process we spawned, by pid.
    let ret = unsafe { libc::kill(pid, sig) };
    if ret != 0 {
        return Err(child);
    }

    let start = Instant::now();
    loop {
        let Ok(maybe_status) = child.try_wait() else {
            return Err(child);
        };
        if maybe_status.is_some() {
            // Child exited! We're done.
            return Ok(());
        }

        // Child still running; sleep briefly then check again, until we hit our
        // deadline.
        if start.elapsed() <= wait_time {
            thread::sleep(Duration::from_millis(100));
            continue;
        } else {
            return Err(child);
        }
    }
}

fn write_service_manifest(
    service: &str,
    path: &Utf8Path,
) -> Result<(), IsolatedConfigdBuildError> {
    let f = File::create_new(path).map_err(|err| {
        IsolatedConfigdBuildError::FakeServiceManifestCreate {
            path: path.to_path_buf(),
            err,
        }
    })?;
    let mut f = io::BufWriter::new(f);

    let last_service_component = match service.rsplit_once('/') {
        Some((_, suffix)) => suffix,
        None => service,
    };

    writeln!(
        f,
        r#"<?xml version="1.0"?>
<!DOCTYPE service_bundle SYSTEM "/usr/share/lib/xml/dtd/service_bundle.dtd.1">

<service_bundle type="manifest" name="{last_service_component}">

  <service name="{service}" type="service" version="1">

    <create_default_instance enabled="false" />

    <single_instance />

    <dependency name="milestone"
                grouping="require_all"
                restart_on="none"
                type="service">
      <service_fmri value="svc:/milestone/single-user" />
    </dependency>

    <exec_method type="method"
                 name="start"
                 exec="/usr/bin/sleep 99999"
                 timeout_seconds="0">
      <method_context>
        <method_credential user="nobody" group="nobody" />
      </method_context>
    </exec_method>

    <exec_method type="method"
                 name="stop"
                 exec=":kill"
                 timeout_seconds="30" />

    <stability value="Unstable" />

  </service>

</service_bundle>
"#
    )
    .map_err(|err| {
        IsolatedConfigdBuildError::FakeServiceManifestWrite {
            path: path.to_path_buf(),
            err,
        }
    })?;

    f.flush().map_err(|err| {
        IsolatedConfigdBuildError::FakeServiceManifestWrite {
            path: path.to_path_buf(),
            err,
        }
    })
}

struct KillOnDrop(Option<Child>);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        // Attempt to shutdown, but ignore errors - we can't report them and
        // don't want to double panic.
        _ = self.shutdown();
    }
}

impl KillOnDrop {
    // Best effort "has this process exited" - only returns true if we get a
    // definitive result that indicates it has, or if we've previously had
    // `shutdown()` called on us. Returns false if it's still running or if we
    // get an error from `try_wait()`.
    fn has_exited(&mut self) -> bool {
        let Some(child) = self.0.as_mut() else {
            // if `child` is gone, we've shutdown already.
            return true;
        };
        matches!(child.try_wait(), Ok(Some(_exit_status)))
    }

    fn shutdown(&mut self) -> Result<(), IsolatedConfigdShutdownError> {
        // We might be called twice: once by `shutdown()` and once by
        // `Drop`. If that happens, `Drop` ignores our return value anyway,
        // so claim we succeeded. (We don't know whether the attempt that
        // took `self.configd_child` out actually did succeed.)
        let Some(child) = self.0.take() else {
            return Ok(());
        };

        match try_kill_via_sigint(child, SIGINT_SHUTDOWN_TIMEOUT) {
            Ok(()) => Ok(()),
            Err(mut child) => {
                // SIGINT didn't work; use SIGKILL instead.
                child
                    .kill()
                    .map_err(IsolatedConfigdShutdownError::ConfigdKill)?;
                child.wait().map_err(
                    IsolatedConfigdShutdownError::ConfigdWaitAfterKill,
                )?;
                Ok(())
            }
        }
    }
}
