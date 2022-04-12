mod pb {
    tonic::include_proto!("signor");
}

#[derive(Eq, PartialEq, Clone, Copy)]
pub enum ConnectionSide {
    Polite,
    Impolite,
}

pub struct Client {
    client: pb::session_service_client::SessionServiceClient<tonic::transport::Channel>,
}

#[derive(Debug)]
pub enum Error {
    InvalidHandshake,
    WebRTC(webrtc::Error),
    TonicTransport(tonic::transport::Error),
    TonicStatus(tonic::Status),
    Other(anyhow::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::InvalidHandshake => write!(f, "invalid handshake"),
            Error::WebRTC(e) => write!(f, "WebRTC error: {:?}", e),
            Error::TonicTransport(e) => write!(f, "tonic transport error: {:?}", e),
            Error::TonicStatus(e) => write!(f, "tonic status: {:?}", e),
            Error::Other(e) => write!(f, "other error: {:?}", e),
        }
    }
}

impl std::error::Error for Error {}

impl From<webrtc::Error> for Error {
    fn from(e: webrtc::Error) -> Self {
        Error::WebRTC(e)
    }
}

impl From<tonic::transport::Error> for Error {
    fn from(e: tonic::transport::Error) -> Self {
        Error::TonicTransport(e)
    }
}

impl From<tonic::Status> for Error {
    fn from(e: tonic::Status) -> Self {
        Error::TonicStatus(e)
    }
}

impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        Error::Other(e)
    }
}

impl Client {
    pub async fn new(host: &str) -> Result<Client, Error> {
        let client =
            pb::session_service_client::SessionServiceClient::connect(host.to_string()).await?;
        Ok(Client { client })
    }

    pub async fn connect<T, F, Fut>(
        &mut self,
        make_peer_conn: F,
        session_id: &str,
    ) -> Result<
        (
            webrtc::peer_connection::RTCPeerConnection,
            T,
            ConnectionSide,
        ),
        Error,
    >
    where
        Fut: std::future::Future<
            Output = anyhow::Result<(webrtc::peer_connection::RTCPeerConnection, T)>,
        >,
        F: Fn() -> Fut,
    {
        let (sender, receiver) = tokio::sync::mpsc::channel(1);

        let (mut peer_conn, mut r) = make_peer_conn().await?;

        let mut gather_complete = peer_conn.gathering_complete_promise().await;
        let offer = peer_conn.create_offer(None).await?;
        peer_conn.set_local_description(offer).await?;
        gather_complete.recv().await;
        sender
            .send(pb::NegotiateRequest {
                which: Some(pb::negotiate_request::Which::Start(
                    pb::negotiate_request::Start {
                        session_id: session_id.to_string(),
                        offer_sdp: peer_conn.local_description().await.expect("local sdp").sdp,
                    },
                )),
            })
            .await
            .expect("negotiation start sent");
        log::info!("negotiation started");

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
            return Err(Error::InvalidHandshake);
        } {
            pb::negotiate_response::Which::Offer(offer) => {
                log::info!("this is the polite side");

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
                                sdp: peer_conn.local_description().await.expect("remote sdp").sdp,
                            },
                        )),
                    })
                    .await
                    .expect("send Answer");

                if let Some(pb::NegotiateResponse {
                    which:
                        Some(pb::negotiate_response::Which::Answered(
                            pb::negotiate_response::Answered {},
                        )),
                }) = inbound.message().await?
                {
                } else {
                    return Err(Error::InvalidHandshake);
                }
            }
            pb::negotiate_response::Which::Answer(answer) => {
                log::info!("this is the impolite side");

                side = ConnectionSide::Impolite;
                let mut sdp = webrtc::peer_connection::sdp::session_description::RTCSessionDescription::default();
                sdp.sdp_type = webrtc::peer_connection::sdp::sdp_type::RTCSdpType::Answer;
                sdp.sdp = answer.sdp;
                peer_conn.set_remote_description(sdp).await?;
            }
            pb::negotiate_response::Which::IceCandidate(_) => {
                return Err(Error::InvalidHandshake);
            }
            _ => {
                return Err(Error::InvalidHandshake);
            }
        };
        Ok((peer_conn, r, side))
    }
}
