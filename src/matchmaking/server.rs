struct Session {
    clients: std::sync::atomic::AtomicIsize,
    offer_sdp: String,
    streams: Vec<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
}

pub struct Server {
    listener: tokio::net::TcpListener,
    sessions: std::sync::Arc<parking_lot::Mutex<std::collections::HashMap<String, Session>>>,
}

async fn handle_connection(
    sessions: std::sync::Arc<parking_lot::Mutex<std::collections::HashMap<String, Session>>>,
    raw_stream: tokio::net::TcpStream,
    _addr: std::net::SocketAddr,
) -> anyhow::Result<()> {
    let stream = tokio_tungstenite::accept_async(raw_stream).await?;
    Ok(())
}

impl Server {
    pub fn new(listener: tokio::net::TcpListener) -> Server {
        Server {
            listener,
            sessions: std::sync::Arc::new(
                parking_lot::Mutex::new(std::collections::HashMap::new()),
            ),
        }
    }

    pub async fn listen(&mut self) {
        while let Ok((stream, addr)) = self.listener.accept().await {
            tokio::spawn(handle_connection(self.sessions.clone(), stream, addr));
        }
    }
}
