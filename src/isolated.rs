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
//! * Environment varialbes that tell `svccfg` to point to a different
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
use std::thread;
use std::time::Duration;
use std::time::Instant;

// When we try to kill `svc.configd` via `SIGINT`, how long do we wait before we
// give up and send `SIGKILL`?
const SIGINT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

pub struct IsolatedConfigd {
    dir: Utf8TempDir,
    door_path: Utf8PathBuf,
    configd_child: Option<Child>,
}

impl Drop for IsolatedConfigd {
    fn drop(&mut self) {
        // Attempt to shutdown, but ignore errors - we can't report them and
        // don't to double panic.
        _ = self.shutdown_impl();
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
    #[error("error during `svccfg -s {fmri} refresh` (exited {status}): {err}")]
    SvccfgRefreshError { fmri: String, status: i32, err: String },
}

#[derive(Debug, thiserror::Error)]
pub enum IsolatedConfigdShutdownError {
    #[error("failed to kill svc.configd child process")]
    ConfigdKill(#[source] io::Error),
    #[error("failed to wait on killed svc.configd child process")]
    ConfigdWaitAfterKill(#[source] io::Error),
}

impl IsolatedConfigd {
    pub fn builder(service: impl Into<String>) -> IsolatedConfigdBuilder {
        IsolatedConfigdBuilder { services: BTreeSet::from([service.into()]) }
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
        check_command_output(output).map_err(|(status, err)| {
            IsolatedConfigdRefreshError::SvccfgRefreshError {
                fmri: fmri.to_owned(),
                status,
                err,
            }
        })
    }

    pub(crate) fn door_path(&self) -> &Utf8Path {
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
        self.shutdown_impl()
    }

    fn shutdown_impl(&mut self) -> Result<(), IsolatedConfigdShutdownError> {
        // We might be called twice: once by `shutdown()` and once by `Drop`. If
        // that happens, `Drop` ignores our return value anyway, so claim we
        // succeeded. (We don't know whether the attempt that took
        // `self.configd_child` out actually did succeed.)
        let Some(child) = self.configd_child.take() else {
            return Ok(());
        };

        match try_kill_via_sigint(child, SIGINT_SHUTDOWN_TIMEOUT) {
            Ok(()) => Ok(()),
            Err(mut child) => {
                // SIGINT didn't work; use SIGKILL instead.
                //
                // We can't do much about errors here; just ignore them.
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
    #[error("faild writing to fake service manifest file `{path}`")]
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
    #[error("error during `svccfg import {path}` (exited {status}): {err}")]
    SvccfgImportError { path: Utf8PathBuf, status: i32, err: String },
    #[error("failed to exec `svc.configd` pointed at the isolated repo")]
    SvcConfigdExec(#[source] io::Error),
}

pub struct IsolatedConfigdBuilder {
    services: BTreeSet<String>,
}

impl IsolatedConfigdBuilder {
    const DOOR_FILENAME: &str = "door";
    const REPO_FILENAME: &str = "repo";

    pub fn add_service(mut self, service: impl Into<String>) -> Self {
        self.services.insert(service.into());
        self
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
            check_command_output(output).map_err(|(status, err)| {
                IsolatedConfigdBuildError::SvccfgImportError {
                    path: path.clone(),
                    status,
                    err,
                }
            })?;
        }

        // Spawn a `svc.configd` pointed to our isolated repo.
        let configd_child = Command::new("/lib/svc/bin/svc.configd")
            .arg("-n") // don't daemonize
            .args(["-d", door_path.as_str()])
            .args(["-r", repo_path.as_str()])
            .spawn()
            .map_err(IsolatedConfigdBuildError::SvcConfigdExec)?;

        Ok(IsolatedConfigd {
            dir,
            door_path,
            configd_child: Some(configd_child),
        })
    }
}

fn check_command_output(output: Output) -> Result<(), (i32, String)> {
    if output.status.success() {
        Ok(())
    } else {
        let status = output.status.into_raw();
        let mut err = String::new();
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
        Err((status, err))
    }
}

fn try_kill_via_sigint(
    mut child: Child,
    wait_time: Duration,
) -> Result<(), Child> {
    // Sent SIGINT.
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
    .map_err(|err| IsolatedConfigdBuildError::FakeServiceManifestWrite {
        path: path.to_path_buf(),
        err,
    })
}
