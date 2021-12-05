use anyhow::Context as _;
use tarpc::server::Channel as _;

#[derive(argh::FromArgs, PartialEq, Debug)]
/// Start the daemon listening for config requests
#[argh(subcommand, name = "daemon")]
pub struct Args {
    #[argh(option)]
    /// path to the UNIX socket where commands are received from
    socket_path: std::path::PathBuf,
}

#[tarpc::service]
pub(crate) trait Service {
    async fn run(config: crate::Config, routes: Vec<crate::Route>) -> Result<(), String>;
}

#[derive(Clone)]
struct ServiceImpl;

#[tarpc::server]
impl Service for ServiceImpl {
    async fn run(
        self,
        _: tarpc::context::Context,
        config: crate::Config,
        routes: Vec<crate::Route>,
    ) -> Result<(), String> {
        crate::run_locally_noenv(config, routes).map_err(|e| format!("{}", e))
    }
}

pub async fn run(args: &Args) -> anyhow::Result<()> {
    let _ = std::fs::remove_file(&args.socket_path);
    let listener =
        tokio::net::UnixListener::bind(&args.socket_path).context("can't bind to server socket")?;
    let codec_builder = tokio_util::codec::length_delimited::LengthDelimitedCodec::builder();
    loop {
        let (conn, _addr) = listener.accept().await.unwrap();
        let framed = codec_builder.new_framed(conn);
        let transport =
            tarpc::serde_transport::new(framed, tarpc::tokio_serde::formats::Bincode::default());

        // it makes no sense to process more than one connection at a time
        tarpc::server::BaseChannel::with_defaults(transport)
            .execute(ServiceImpl.serve())
            .await;
    }
}
