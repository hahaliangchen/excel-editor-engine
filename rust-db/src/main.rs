use db_studio_engine::{server, tool_rpc};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--tool") => tool_rpc::serve(args.next().as_deref()).await,
        None => server::serve().await,
        _ => anyhow::bail!("usage: db-server [--tool [db__tool_name]]"),
    }
}
