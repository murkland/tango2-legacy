mod pb {
    tonic::include_proto!("signor");
}

pub enum ConnectionSide {
    Polite,
    Impolite,
}

pub struct Client {
    client: pb::session_service_client::SessionServiceClient<tonic::transport::Channel>,
}

impl Client {
    pub async fn new(host: &str) -> anyhow::Result<Client> {
        let client =
            pb::session_service_client::SessionServiceClient::connect(host.to_string()).await?;
        Ok(Client { client })
    }

    pub async fn connect(
        &mut self,
        peer_conn: &webrtc::peer_connection::RTCPeerConnection,
        session_id: &str,
    ) -> anyhow::Result<ConnectionSide> {
        let mut gather_complete = peer_conn.gathering_complete_promise().await;
        let offer = peer_conn.create_offer(None).await?;
        peer_conn.set_local_description(offer).await?;
        gather_complete.recv().await;

        let (mut sender, receiver) = futures::channel::mpsc::channel(1);
        let negotiation = self.client.negotiate(tonic::Request::new(receiver)).await?;
        let mut inbound = negotiation.into_inner();

        sender.start_send(pb::NegotiateRequest {
            which: Some(pb::negotiate_request::Which::Start(
                pb::negotiate_request::Start {
                    session_id: session_id.to_string(),
                    offer_sdp: peer_conn.local_description().await.unwrap().sdp,
                },
            )),
        })?;

        let mut side = ConnectionSide::Polite;

        match if let Some(pb::NegotiateResponse { which: Some(which) }) = inbound.message().await? {
            which
        } else {
            anyhow::bail!("failed to receive message");
        } {
            pb::negotiate_response::Which::Offer(offer) => {
                {
                    let mut sdp = webrtc::peer_connection::sdp::session_description::RTCSessionDescription::default();
                    sdp.sdp_type = webrtc::peer_connection::sdp::sdp_type::RTCSdpType::Rollback;
                    peer_conn.set_local_description(sdp).await?;
                }

                {
                    let mut sdp = webrtc::peer_connection::sdp::session_description::RTCSessionDescription::default();
                    sdp.sdp_type = webrtc::peer_connection::sdp::sdp_type::RTCSdpType::Offer;
                    sdp.sdp = offer.sdp;
                    peer_conn.set_remote_description(sdp).await?;
                }

                let mut gather_complete = peer_conn.gathering_complete_promise().await;
                let offer = peer_conn.create_answer(None).await?;
                peer_conn.set_local_description(offer).await?;
                gather_complete.recv().await;
            }
            pb::negotiate_response::Which::Answer(answer) => {
                side = ConnectionSide::Impolite;
                let mut sdp = webrtc::peer_connection::sdp::session_description::RTCSessionDescription::default();
                sdp.sdp_type = webrtc::peer_connection::sdp::sdp_type::RTCSdpType::Answer;
                sdp.sdp = answer.sdp;
                peer_conn.set_remote_description(sdp).await?;
            }
            pb::negotiate_response::Which::IceCandidate(_) => {
                anyhow::bail!("trickle ice not supported")
            }
        };

        Ok(side)
    }
}
