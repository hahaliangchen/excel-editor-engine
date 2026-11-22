use excel_editor_engine::{server, tool_rpc};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--tool") {
        tool_rpc::serve(args.get(1).map(String::as_str))
    } else {
        server::serve().await
    }
}
