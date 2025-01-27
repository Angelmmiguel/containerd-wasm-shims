use std::{
    os::fd::IntoRawFd,
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
};

use containerd_shim::run;
use containerd_shim_wasm::{
    libcontainer_instance::LibcontainerInstance,
    sandbox::{
        instance::ExitCode,
        instance_utils::{determine_rootdir, maybe_open_stdio},
        Error, InstanceConfig, ShimCli,
    },
};
use libcontainer::{
    container::{builder::ContainerBuilder, Container},
    syscall::syscall::create_syscall,
};

use anyhow::{Context, Result};

use crate::executor::LunaticExecutor;

mod common;
mod executor;

static DEFAULT_CONTAINER_ROOT_DIR: &str = "/run/containerd/lunatic";

pub struct Wasi {
    id: String,
    exit_code: ExitCode,
    bundle: String,
    rootdir: PathBuf,
    stdin: String,
    stdout: String,
    stderr: String,
}

impl LibcontainerInstance for Wasi {
    type Engine = ();

    fn new_libcontainer(id: String, cfg: Option<&InstanceConfig<Self::Engine>>) -> Self {
        let cfg = cfg.unwrap();
        let bundle = cfg.get_bundle().unwrap_or_default();

        Wasi {
            id,
            exit_code: Arc::new((Mutex::new(None), Condvar::new())),
            rootdir: determine_rootdir(
                bundle.as_str(),
                cfg.get_namespace().as_str(),
                DEFAULT_CONTAINER_ROOT_DIR,
            )
            .unwrap(),
            bundle,
            stdin: cfg.get_stdin().unwrap_or_default(),
            stdout: cfg.get_stdout().unwrap_or_default(),
            stderr: cfg.get_stderr().unwrap_or_default(),
        }
    }

    fn get_exit_code(&self) -> ExitCode {
        self.exit_code.clone()
    }

    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn get_root_dir(&self) -> std::result::Result<PathBuf, Error> {
        Ok(self.rootdir.clone())
    }

    fn build_container(&self) -> Result<Container, Error> {
        log::info!("Building container");

        let stdin = maybe_open_stdio(&self.stdin)
            .context("could not open stdin")?
            .map(|f| f.into_raw_fd());
        let stdout = maybe_open_stdio(&self.stdout)
            .context("could not open stdout")?
            .map(|f| f.into_raw_fd());
        let stderr = maybe_open_stdio(&self.stderr)
            .context("could not open stderr")?
            .map(|f| f.into_raw_fd());

        let syscall = create_syscall();
        let err_msg = |err| format!("failed to create container: {}", err);
        let container = ContainerBuilder::new(self.id.clone(), syscall.as_ref())
            .with_executor(vec![Box::new(LunaticExecutor {
                stdin,
                stdout,
                stderr,
            })])
            .map_err(|err| Error::Others(err_msg(err)))?
            .with_root_path(self.rootdir.clone())
            .map_err(|err| Error::Others(err_msg(err)))?
            .as_init(&self.bundle)
            .with_systemd(false)
            .build()
            .map_err(|err| Error::Others(err_msg(err)))?;

        log::info!(">>> Container built.");
        Ok(container)
    }
}

fn main() {
    run::<ShimCli<Wasi>>("io.containerd.lunatic.v1", None);
}
