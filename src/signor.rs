pub struct Client {}

impl Client {
    pub async fn new(host: &str) -> anyhow::Result<Client> {
        todo!()
    }

    pub async fn connect(
        &self,
        peer_conn: &mut webrtc::peer_connection::RTCPeerConnection,
        session_id: &str,
    ) -> anyhow::Result<()> {
        todo!()
    }
}
