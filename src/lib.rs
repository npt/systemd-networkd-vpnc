#[cfg(feature = "daemon")]
pub mod daemon;

use std::io::prelude::*;
use std::path::{Path, PathBuf};

const NETWORKCTL: &str = "networkctl";
const SYSTEMD_NETWORKD_CONFIG_DIR: &str = "/etc/systemd/network/";

/// looks up a binary in `PATH`
///
/// There may be two reasons to do this:
/// - verify that a command exists before trying to run it
/// - always use the same binary even if the contents of the directories listed
///   in `PATH` changes.
fn find_bin_file(file: &str) -> Option<PathBuf> {
    std::env::var("PATH").ok().and_then(|path| {
        path.split(':')
            .map(|p| Path::new(p).join(file))
            .find(|p| p.exists())
    })
}

/// Wrapper around the systemd's `networkctl` command
struct Networkctl {
    bin: PathBuf,
}

impl Networkctl {
    pub fn new() -> Networkctl {
        Networkctl {
            bin: find_bin_file(NETWORKCTL).expect("`networkctl` not found"),
        }
    }

    pub fn reload(&self) -> Result<(), std::io::Error> {
        std::process::Command::new(&self.bin)
            .arg("reload")
            .status()
            .map(|_| ())
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct Route {
    addr: String,
    mask: String,
    masklen: u8,
    #[serde(default = "Route::default_protocol")]
    protocol: u8,
    #[serde(default = "Route::default_port")]
    sport: u16,
    #[serde(default = "Route::default_port")]
    dport: u16,
}

impl Route {
    fn default_protocol() -> u8 {
        0
    }

    fn default_port() -> u16 {
        0
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
enum Reason {
    Connect,
    Disconnect,
    PreInit,
    AttemptReconnect,
    Reconnect,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[allow(dead_code)]
pub(crate) struct Config {
    reason: Reason,
    vpngateway: String,
    tundev: String,

    #[serde(rename = "internal_ip4_address")]
    address: String,
    #[serde(rename = "internal_ip4_mtu")]
    mtu: Option<u32>,
    #[serde(rename = "internal_ip4_netmask")]
    netmask: Option<String>,
    #[serde(
        rename = "internal_ip4_netmasklen",
        default = "Config::default_netmasklen"
    )]
    netmasklen: u8,
    #[serde(rename = "internal_ip4_netaddr")]
    netaddr: Option<String>,
    #[serde(rename = "internal_ip4_dns")]
    dns: Option<String>,
    #[serde(rename = "internal_ip4_nbns")]
    nbns: Option<String>,

    #[serde(rename = "cisco_def_domain")]
    def_domain: Option<String>,
    #[serde(rename = "cisco_banner")]
    banner: Option<String>,
    #[serde(rename = "cisco_split_inc", default = "Config::default_split_routes")]
    split_routes_inc: usize,
}

impl Config {
    fn default_netmasklen() -> u8 {
        32
    }

    fn default_split_routes() -> usize {
        0
    }
}

fn split_routes(config: &Config) -> Result<Vec<Route>, envy::Error> {
    (0..config.split_routes_inc)
        .map(|n| envy::prefixed(format!("CISCO_SPLIT_INC_{}_", n)).from_env::<Route>())
        .collect::<Result<Vec<_>, _>>()
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Env(#[from] envy::Error),
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}

/// specifies if the network configuration has changed
enum Changed {
    Yes,
    No,
}

struct Process {
    config: Config,
    split_routes_inc: Vec<Route>,
    network_file: PathBuf,
}

impl Process {
    pub fn new(config: Config, split_routes_inc: Vec<Route>) -> Process {
        Process {
            network_file: PathBuf::from(SYSTEMD_NETWORKD_CONFIG_DIR)
                .join(&config.tundev)
                .with_extension("network"),
            split_routes_inc,
            config,
        }
    }

    pub fn run(&self) -> Result<Changed, std::io::Error> {
        use Reason::*;
        match self.config.reason {
            PreInit => self.pre_init(),
            Connect => self.connect(),
            Disconnect => self.disconnect(),
            AttemptReconnect => self.attempt_reconnect(),
            Reconnect => self.reconnect(),
        }
    }

    fn reconnect(&self) -> Result<Changed, std::io::Error> {
        Ok(Changed::No)
    }

    fn pre_init(&self) -> Result<Changed, std::io::Error> {
        Ok(Changed::No)
    }

    fn attempt_reconnect(&self) -> Result<Changed, std::io::Error> {
        Ok(Changed::No)
    }

    fn disconnect(&self) -> Result<Changed, std::io::Error> {
        std::fs::remove_file(&self.network_file)?;
        Ok(Changed::Yes)
    }

    fn connect(&self) -> Result<Changed, std::io::Error> {
        if let Some(ref banner) = self.config.banner {
            println!("Connect Banner:\n{}", banner);
        }

        if let Some(config_dir) = self.network_file.parent() {
            std::fs::create_dir_all(config_dir)?;
        }

        let mut file = std::fs::File::create(&self.network_file)?;

        writeln!(
            file,
            r#"
[Link]
MTUBytes={0}

[Address]
Address={1}/32

[Route]
Destination={1}/32
Gateway={1}
"#,
            self.config.mtu.unwrap_or(1412),
            self.config.address,
        )?;

        if self.config.netmask.is_some() {
            writeln!(
                file,
                r#"
[Route]
Destination={}/{}
Scope=link
"#,
                self.config.netaddr.as_deref().unwrap(),
                self.config.netmasklen
            )?;
        }

        let mut default_route = false;
        if self.config.split_routes_inc > 0 {
            for route in &self.split_routes_inc {
                if route.addr == "0.0.0.0" {
                    default_route = true;
                } else {
                    writeln!(
                        file,
                        r#"
[Route]
Scope=link
Destination={}/{}
"#,
                        route.addr, route.masklen
                    )?;
                }
            }
        } else {
            default_route = !self.config.address.is_empty();
        }

        writeln!(
            file,
            r#"
[Match]
Name={}

[Network]
Description=Cisco VPN to {}
DHCP=no
IPv6AcceptRA=no
"#,
            self.config.tundev, self.config.vpngateway
        )?;

        if default_route {
            writeln!(file, "DefaultRouteOnDevice=yes")?;
        }

        if let Some(ref dns) = self.config.dns {
            if let Some(ref def_domain) = self.config.def_domain {
                writeln!(file, "Domains={}", def_domain)?;
            }

            for ns in dns.split_ascii_whitespace() {
                writeln!(file, "DNS={}", ns)?;
            }
        }
        Ok(Changed::Yes)
    }
}

pub(crate) fn run_locally_noenv(config: Config, routes: Vec<Route>) -> Result<(), Error> {
    if matches!(Process::new(config, routes).run()?, Changed::Yes) {
        Networkctl::new().reload()?;
    }

    Ok(())
}

pub fn run_locally() -> Result<(), Error> {
    let config = envy::from_env::<Config>()?;
    let routes = split_routes(&config)?;

    run_locally_noenv(config, routes)
}

#[cfg(feature = "daemon")]
pub async fn run_remotely<P: AsRef<std::path::Path>>(path: P) -> anyhow::Result<()> {
    let config = envy::from_env::<Config>()?;
    let routes = split_routes(&config)?;

    let codec_builder = tokio_util::codec::length_delimited::LengthDelimitedCodec::builder();
    let conn = tokio::net::UnixStream::connect(path).await?;
    let transport = tarpc::serde_transport::new(
        codec_builder.new_framed(conn),
        tarpc::tokio_serde::formats::Bincode::default(),
    );

    daemon::ServiceClient::new(Default::default(), transport)
        .spawn()
        .run(tarpc::context::current(), config, routes)
        .await?
        .map_err(|s| anyhow::anyhow!(s))?;

    Ok(())
}
