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

    pub async fn connect<T, F, Fut>(
        &mut self,
        make_peer_conn: F,
        session_id: &str,
    ) -> anyhow::Result<(
        webrtc::peer_connection::RTCPeerConnection,
        T,
        ConnectionSide,
    )>
    where
        Fut: std::future::Future<
            Output = anyhow::Result<(webrtc::peer_connection::RTCPeerConnection, T)>,
        >,
        F: Fn() -> Fut,
    {
        let (mut peer_conn, mut r) = make_peer_conn().await?;

        let mut gather_complete = peer_conn.gathering_complete_promise().await;
        let offer = peer_conn.create_offer(None).await?;
        peer_conn.set_local_description(offer).await?;
        gather_complete.recv().await;

        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        sender
            .send(pb::NegotiateRequest {
                which: Some(pb::negotiate_request::Which::Start(
                    pb::negotiate_request::Start {
                        session_id: session_id.to_string(),
                        offer_sdp: peer_conn.local_description().await.unwrap().sdp,
                    },
                )),
            })
            .await?;

        let negotiation = self
            .client
            .negotiate(tonic::Request::new(
                tokio_stream::wrappers::ReceiverStream::new(receiver),
            ))
            .await?;
        let mut inbound = negotiation.into_inner();

        let mut side = ConnectionSide::Polite;

        match if let Some(pb::NegotiateResponse { which: Some(which) }) = inbound.message().await? {
            which
        } else {
            anyhow::bail!("failed to receive message");
        } {
            pb::negotiate_response::Which::Offer(offer) => {
                let (peer_conn2, r2) = make_peer_conn().await?;
                peer_conn = peer_conn2;
                r = r2;

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

                sender
                    .send(pb::NegotiateRequest {
                        which: Some(pb::negotiate_request::Which::Answer(
                            pb::negotiate_request::Answer {
                                sdp: peer_conn.local_description().await.unwrap().sdp,
                            },
                        )),
                    })
                    .await?;

                if let Some(pb::NegotiateResponse {
                    which:
                        Some(pb::negotiate_response::Which::Answered(
                            pb::negotiate_response::Answered {},
                        )),
                }) = inbound.message().await?
                {
                } else {
                    anyhow::bail!("failed to receive answered message");
                }
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
            r => {
                anyhow::bail!("unknown message: {:?}", r)
            }
        };
        Ok((peer_conn, r, side))
    }
}
