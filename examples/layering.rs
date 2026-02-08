use neolith::server::connection::handle_conn;
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .try_init()?;

    let listener = TcpListener::bind("[::1]:5500").await?;
    let mut id = 1u32;
    loop {
        let conn_id = id;
        id = id.wrapping_add(1);
        let (socket, addr) = listener.accept().await?;
        tracing::info!("accept from {addr:?}!");
        let (r, w) = socket.into_split();
        tokio::spawn(async move {
            if let Err(e) = handle_conn(conn_id, r, w).await {
                eprintln!("bad conn from {addr:?}: {e:?}");
            }
        });
    }
}
