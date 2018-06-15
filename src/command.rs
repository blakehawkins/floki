use dind;
use errors::FlokiError;
use quicli::prelude::*;
use std::env;
use std::path;
use std::process::{Command, ExitStatus, Stdio};

#[derive(Debug, Clone)]
pub struct DockerCommandBuilder {
    volumes: Vec<(String, String)>,
    environment: Vec<(String, String)>,
    shell: String,
    switches: Vec<String>,
    image: String,
}

impl DockerCommandBuilder {
    pub fn run(&self, subshell_command: String) -> Result<ExitStatus> {
        debug!(
            "Spawning docker command with configuration: {:?} args: {}",
            self, &subshell_command
        );
        let mut command = Command::new("docker")
            .args(&["run", "--rm", "-it"])
            .args(&self.build_volume_switches())
            .args(&self.build_environment_switches())
            .args(&self.build_docker_switches())
            .arg(&self.image)
            .arg(&self.shell)
            .arg("-c")
            .arg(subshell_command)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .stdin(Stdio::inherit())
            .spawn()
            .map_err(|e| FlokiError::FailedToLaunchDocker { error: e })?;

        let exit_status = command
            .wait()
            .map_err(|e| FlokiError::FailedToCompleteDockerCommand { error: e })?;

        Ok(exit_status)
    }

    pub fn new(image: &str, shell: &str) -> Self {
        DockerCommandBuilder {
            volumes: Vec::new(),
            environment: Vec::new(),
            shell: shell.into(),
            switches: Vec::new(),
            image: image.into(),
        }
    }

    pub fn add_volume(mut self, spec: &(String, String)) -> Self {
        self.volumes.push(spec.clone());
        self
    }

    pub fn add_environment(mut self, spec: &(String, String)) -> Self {
        self.environment.push(spec.clone());
        self
    }

    pub fn add_docker_switch(mut self, switch: &String) -> Self {
        self.switches.push(switch.clone());
        self
    }

    fn build_volume_switches(&self) -> Vec<String> {
        let mut switches = Vec::new();
        for (s, d) in self.volumes.iter() {
            switches.push("-v".into());
            switches.push(format!("{}:{}", s, d));
        }
        switches
    }

    fn build_environment_switches(&self) -> Vec<String> {
        let mut switches = Vec::new();
        for (var, bind) in self.environment.iter() {
            switches.push("-e".into());
            switches.push(format!("{}={}", var, bind));
        }
        switches
    }

    fn build_docker_switches(&self) -> Vec<String> {
        let mut switches = Vec::new();
        for docker_switch in self.switches.iter() {
            let pieces = docker_switch.split_whitespace();
            for s in pieces {
                switches.push(s.into());
            }
        }
        switches
    }
}

pub fn enable_forward_ssh_agent(command: DockerCommandBuilder) -> Result<DockerCommandBuilder> {
    let agent_socket = env::var("SSH_AUTH_SOCK")?;
    debug!("Got SSH_AUTH_SOCK={}", agent_socket);
    if let Some(dir) = path::Path::new(&agent_socket)
        .parent()
        .and_then(|p| p.to_str())
    {
        Ok(command
            .add_environment(&("SSH_AUTH_SOCK".into(), agent_socket.clone()))
            .add_volume(&(dir.into(), dir.into())))
    } else {
        Err(FlokiError::NoSshAuthSock {})?
    }
}

pub fn enable_forward_tmux_socket(command: DockerCommandBuilder) -> Result<DockerCommandBuilder> {
    let tmux_env = env::var("TMUX")?;
    debug!("Got TMUX={}", tmux_env);
    let tmux_params: Vec<&str> = tmux_env.split(',').collect();
    match tmux_params.get(0) {
        Some(path) => {
            let tmux_path = path::Path::new(path);
            if let (Some(dir), Some(name)) = (
                tmux_path.parent().and_then(|d| d.to_str()),
                tmux_path.file_name().and_then(|f| f.to_str()),
            ) {
                debug!(
                    "tmux socket directory: {}, tmux socket filename: {}",
                    dir, name
                );
                Ok(command
                    .add_environment(&("TMUX_SOCKET".into(), String::from("/run/tmux/") + name))
                    .add_volume(&(dir.into(), "/run/tmux".into())))
            } else {
                Err(FlokiError::TmuxForwardError {
                    msg: "tmux socket in env has bad filename".into(),
                })?
            }
        }
        None => Err(FlokiError::TmuxForwardError {
            msg: "Could not get tmux socket from environment".into(),
        })?,
    }
}

pub fn enable_docker_in_docker(
    command: DockerCommandBuilder,
    dind: &mut dind::Dind,
) -> Result<DockerCommandBuilder> {
    debug!("docker-in-docker: {:?}", &dind);
    dind::dind_preflight()?;
    dind.launch()?;
    Ok(command
        .add_docker_switch(&format!("--link {}:floki-docker", dind.name))
        .add_environment(&("DOCKER_HOST".into(), "tcp://floki-docker:2375".into())))
}