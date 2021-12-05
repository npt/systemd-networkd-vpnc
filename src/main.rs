use argh::FromArgs;
use systemd_networkd_vpnc::*;

#[derive(FromArgs, PartialEq, Debug)]
/// systemd-networkd-vpnc CLI
struct Args {
    #[argh(subcommand)]
    subcommand: SubCommand,
}

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand)]
enum SubCommand {
    #[cfg(feature = "daemon")]
    Daemon(daemon::Args),
    Run(Run),
}

#[derive(FromArgs, PartialEq, Debug)]
/// Run the script based on the config inside the environment variables
#[argh(subcommand, name = "run")]
struct Run {
    #[cfg(feature = "daemon")]
    #[argh(option)]
    /// path to the UNIX socket to connect to.
    ///
    /// if missing, the script is run locally without a daemon.
    server_socket: Option<std::path::PathBuf>,
}

#[cfg(feature = "daemon")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Args = argh::from_env();

    match &args.subcommand {
        SubCommand::Daemon(args_daemon) => {
            daemon::run(args_daemon).await?;
        }
        SubCommand::Run(args_run) => {
            if let Some(server_socket) = &args_run.server_socket {
                run_remotely(server_socket).await?;
            } else {
                run_locally()?;
            }
        }
    }

    Ok(())
}

#[cfg(not(feature = "daemon"))]
fn main() -> anyhow::Result<()> {
    let args: Args = argh::from_env();

    match &args.subcommand {
        SubCommand::Run(_) => {
            run_locally()?;
        }
    }

    Ok(())
}
