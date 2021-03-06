use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use std::backtrace::Backtrace;
use thiserror::Error;

pub struct DockerRunArgs {
    pub args: Vec<String>,
    pub container_name: Option<String>,
    pub detached: bool,
    pub entry_point: Option<String>,
    pub environment_vars: Vec<(String, String)>,
    pub expose_all_ports: bool,
    pub expose_ports: Vec<u16>,
    pub expose_port_map: Vec<(u16, u16)>,
    pub expose_port_map_range: Vec<((u16, u16), (u16, u16))>,
    pub hostname: Option<String>,
    pub image: String,
    pub interactive: bool,
    pub network: Option<String>,
    pub remove: bool,
    pub tty: bool,
    pub user: Option<String>,
    pub volume_map: Vec<(PathBuf, PathBuf)>,
    pub work_dir: Option<PathBuf>,
}

pub struct ExecArgs {
    pub tty: bool,
    pub interactive: bool,
    pub work_dir: Option<PathBuf>,
}

impl Default for DockerRunArgs {
    fn default() -> Self {
        Self {
            args: Vec::new(),
            container_name: None,
            detached: false,
            entry_point: None,
            environment_vars: Vec::new(),
            expose_all_ports: false,
            expose_ports: Vec::new(),
            expose_port_map: Vec::new(),
            expose_port_map_range: Vec::new(),
            hostname: None,
            image: "".to_owned(),
            interactive: false,
            network: None,
            remove: true,
            tty: false,
            user: None,
            volume_map: Vec::new(),
            work_dir: None,
        }
    }
}

impl Default for ExecArgs {
    fn default() -> Self {
        Self {
            tty: false,
            interactive: false,
            work_dir: None,
        }
    }
}

#[derive(Clone)]
pub struct DockerClient;

impl DockerClient {
    pub fn new() -> Self {
        Self {}
    }

    pub fn run_cmd(&self, args: DockerRunArgs) -> Command {
        let mut cmd = Command::new("docker");
        cmd.arg("run");
        if args.remove {
            cmd.arg("--rm");
        }
        if args.tty {
            cmd.arg("-t");
        }
        if args.interactive {
            cmd.arg("-i");
        }
        if args.detached {
            cmd.arg("-d");
        }
        args.container_name
            .map(|v| cmd.arg(format!("--name={}", v)));
        args.hostname.map(|v| cmd.arg(format!("--hostname={}", v)));
        args.network.map(|v| cmd.arg(format!("--network={}", v)));
        if args.expose_all_ports {
            cmd.arg("-P");
        }
        args.expose_ports.iter().for_each(|v| {
            cmd.arg("-p");
            cmd.arg(format!("{}", v));
        });
        args.expose_port_map.iter().for_each(|(h, c)| {
            cmd.arg("-p");
            cmd.arg(format!("{}:{}", h, c));
        });
        args.expose_port_map_range
            .iter()
            .for_each(|((hl, hr), (cl, cr))| {
                cmd.arg("-p");
                cmd.arg(format!("{}-{}:{}-{}", hl, hr, cl, cr));
            });
        args.volume_map.iter().for_each(|(h, c)| {
            cmd.arg("-v");
            cmd.arg(format!("{}:{}", h.display(), c.display()));
        });
        args.user.map(|v| cmd.arg(format!("--user={}", v)));
        args.work_dir
            .map(|v| cmd.arg(format!("--workdir={}", v.display())));
        args.environment_vars.iter().for_each(|(n, v)| {
            cmd.arg("-e");
            cmd.arg(format!("{}={}", n, v));
        });
        args.entry_point
            .map(|v| cmd.arg(format!("--entrypoint={}", v)));
        cmd.arg(args.image);
        cmd.args(args.args);
        cmd
    }

    pub fn run_shell_cmd<I, S>(&self, args: DockerRunArgs, shell_cmd: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let shell_cmd_vec: Vec<String> = shell_cmd
            .into_iter()
            .map(|s| s.as_ref().to_string())
            .collect();

        self.run_cmd(DockerRunArgs {
            entry_point: Some("bash".to_owned()),
            args: vec!["-c".to_owned(), shell_cmd_vec.join(" ")],
            ..args
        })
    }

    pub fn exec_cmd<I, S>(&self, exec_args: ExecArgs, container_name: &str, cmd_args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let mut cmd = Command::new("docker");
        cmd.arg("exec");
        if exec_args.tty {
            cmd.arg("-t");
        }
        if exec_args.interactive {
            cmd.arg("-i");
        }
        exec_args
            .work_dir
            .map(|v| cmd.arg(format!("--workdir={}", v.display())));
        cmd.arg(container_name);
        cmd.args(cmd_args);
        cmd
    }

    pub fn exec_shell_cmd<I, S>(
        &self,
        exec_args: ExecArgs,
        container_name: &str,
        shell_cmd: I,
    ) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let shell_cmd_vec: Vec<String> = shell_cmd
            .into_iter()
            .map(|s| s.as_ref().to_string())
            .collect();

        let args = vec!["bash".to_owned(), "-c".to_owned(), shell_cmd_vec.join(" ")];
        self.exec_cmd(exec_args, container_name, args)
    }

    pub fn kill_cmd(&self, container_name: &str) -> Command {
        let mut cmd = Command::new("docker");
        cmd.arg("kill").arg(container_name);
        cmd
    }

    pub fn create_network_cmd(&self, network_name: &str) -> Command {
        let mut cmd = Command::new("docker");
        cmd.arg("network").arg("create").arg(network_name);
        cmd
    }

    pub fn remove_network_cmd(&self, network_name: &str) -> Command {
        let mut cmd = Command::new("docker");
        cmd.arg("network").arg("rm").arg(network_name);
        cmd
    }

    pub fn create_network(&self, network_name: &str) -> NetworkHandle {
        let output = self
            .create_network_cmd(network_name)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();

        if !output.status.success() {
            panic!(
                "Failed to create docker network: exit code: {} stdout: {} stderr: {}",
                output.status,
                std::str::from_utf8(&output.stdout).unwrap(),
                std::str::from_utf8(&output.stderr).unwrap(),
            )
        }

        let remove = self.remove_network_cmd(network_name);
        NetworkHandle::new(remove)
    }

    pub fn get_host_port(&self, container_name: &str, container_port: u16) -> Option<u16> {
        let format = format!(
            "--format={{{{ (index (index .NetworkSettings.Ports \"{}/tcp\") 0).HostPort }}}}",
            container_port
        );

        //let formatEscaped =
        //  if (!OS.isWindows) format else format.replace("\"", "\\\"")

        let res = Command::new("docker")
            .arg("inspect")
            .arg(format)
            .arg(container_name)
            .output();

        match res {
            Ok(output) => std::str::from_utf8(&output.stdout)
                .unwrap()
                .trim_matches(&['\r', '\n'][..])
                .parse()
                .ok(),
            _ => None,
        }
    }

    pub fn wait_for_container(
        &self,
        container_name: &str,
        timeout: Duration,
    ) -> Result<(), TimeoutError> {
        let start = Instant::now();

        loop {
            let res = Command::new("docker")
                .arg("inspect")
                .arg(container_name)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();

            if res.is_ok() && res.unwrap().success() {
                break Ok(());
            } else if start.elapsed() >= timeout {
                break Err(TimeoutError::new(timeout));
            } else {
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }

    pub fn wait_for_host_port(
        &self,
        container_name: &str,
        container_port: u16,
        timeout: Duration,
    ) -> Result<u16, TimeoutError> {
        let start = Instant::now();
        loop {
            let res = self.get_host_port(container_name, container_port);
            if let Some(hp) = res {
                break Ok(hp);
            } else if start.elapsed() >= timeout {
                break Err(TimeoutError::new(timeout));
            } else {
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }

    pub fn check_socket(&self, host_port: u16) -> bool {
        use std::io::Read;
        use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};

        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), host_port);
        let mut stream = match TcpStream::connect_timeout(&addr, Duration::from_millis(100)) {
            Ok(s) => s,
            _ => return false,
        };

        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();

        let mut buf = [0; 1];
        match stream.read(&mut buf) {
            Ok(0) => false,
            Ok(_) => true,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => true,
            Err(_) => false,
        }
    }

    pub fn wait_for_socket(&self, host_port: u16, timeout: Duration) -> Result<(), TimeoutError> {
        let start = Instant::now();
        loop {
            if self.check_socket(host_port) {
                break Ok(());
            } else if start.elapsed() >= timeout {
                break Err(TimeoutError::new(timeout));
            } else {
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }
}

#[derive(Error, Debug)]
#[error("Timed out after {duration:?}")]
pub struct TimeoutError {
    duration: Duration,
    backtrace: Backtrace,
}
impl TimeoutError {
    pub fn new(d: Duration) -> Self {
        Self {
            duration: d,
            backtrace: Backtrace::capture(),
        }
    }
}

#[derive(Debug)]
pub struct NetworkHandle {
    remove: Command,
}

impl NetworkHandle {
    fn new(remove: Command) -> Self {
        Self { remove: remove }
    }
}

impl Drop for NetworkHandle {
    fn drop(&mut self) {
        self.remove
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
    }
}

///////////////////////////////////////////////////////////////////////////////
// DropContainer
///////////////////////////////////////////////////////////////////////////////

pub struct DropContainer {
    docker: DockerClient,
    name: String,
}

impl DropContainer {
    pub fn new(docker: DockerClient, name: &str) -> Self {
        Self {
            docker: docker,
            name: name.to_owned(),
        }
    }
}

impl Drop for DropContainer {
    fn drop(&mut self) {
        let _ = self
            .docker
            .kill_cmd(&self.name)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}
