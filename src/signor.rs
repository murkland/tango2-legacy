#[allow(dead_code)]
mod api;

use futures_util::{sink::SinkExt, TryStreamExt};

#[derive(Eq, PartialEq, Clone, Copy)]
pub enum ConnectionSide {
    Polite,
    Impolite,
}

pub struct Client {
    client: api::SessionServiceClient,
}

#[derive(Debug)]
pub enum Error {
    InvalidHandshake,
    WebRTC(webrtc::Error),
    Grpc(grpcio::Error),
    Other(anyhow::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::InvalidHandshake => write!(f, "invalid handshake"),
            Error::WebRTC(e) => write!(f, "WebRTC error: {:?}", e),
            Error::Grpc(e) => write!(f, "grpc: {:?}", e),
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

impl From<grpcio::Error> for Error {
    fn from(e: grpcio::Error) -> Self {
        Error::Grpc(e)
    }
}

impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        Error::Other(e)
    }
}

lazy_static! {
    static ref CERTS: Vec<rustls_native_certs::Certificate> =
        rustls_native_certs::load_native_certs().unwrap_or(vec![]);
    static ref GRPC_ROOT_CERTS: Vec<u8> = CERTS
        .iter()
        .flat_map(|raw| pem::encode(&pem::Pem {
            tag: String::from("CERTIFICATE"),
            contents: raw.0.clone(),
        })
        .as_bytes()
        .to_vec())
        .collect();
}

impl Client {
    pub fn new(addr: &str, insecure: bool) -> Result<Client, Error> {
        let env = std::sync::Arc::new(grpcio::Environment::new(2));
        let channel_builder = grpcio::ChannelBuilder::new(env);
        let channel = if !insecure {
            channel_builder.secure_connect(
                addr,
                grpcio::ChannelCredentialsBuilder::new()
                    .root_cert(GRPC_ROOT_CERTS.clone())
                    .build(),
            )
        } else {
            channel_builder.connect(addr)
        };
        let client = api::SessionServiceClient::new(channel);
        Ok(Client { client })
    }

    pub async fn connect<T, F, Fut>(
        &self,
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
        let mut side = ConnectionSide::Polite;

        let (mut sink, mut receiver) = self.client.negotiate()?;

        log::info!("negotiation started");

        let (mut peer_conn, mut r) = make_peer_conn().await?;

        let mut gather_complete = peer_conn.gathering_complete_promise().await;
        let offer = peer_conn.create_offer(None).await?;
        peer_conn.set_local_description(offer).await?;
        gather_complete.recv().await;

        sink.send((
            api::NegotiateRequest {
                which: Some(api::negotiate_request::Which::Start(
                    api::negotiate_request::Start {
                        session_id: session_id.to_string(),
                        offer_sdp: peer_conn.local_description().await.expect("local sdp").sdp,
                    },
                )),
            },
            grpcio::WriteFlags::default(),
        ))
        .await?;
        log::info!("negotiation start sent");

        match if let Some(api::NegotiateResponse { which: Some(which) }) =
            receiver.try_next().await?
        {
            which
        } else {
            return Err(Error::InvalidHandshake);
        } {
            api::negotiate_response::Which::Offer(offer) => {
                log::info!("received an offer, this is the polite side");

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

                sink.send((
                    api::NegotiateRequest {
                        which: Some(api::negotiate_request::Which::Answer(
                            api::negotiate_request::Answer {
                                sdp: peer_conn.local_description().await.expect("remote sdp").sdp,
                            },
                        )),
                    },
                    grpcio::WriteFlags::default(),
                ))
                .await?;
                log::info!("sent answer to impolite side");
            }
            api::negotiate_response::Which::Answer(answer) => {
                log::info!("received an answer, this is the impolite side");

                side = ConnectionSide::Impolite;
                let mut sdp = webrtc::peer_connection::sdp::session_description::RTCSessionDescription::default();
                sdp.sdp_type = webrtc::peer_connection::sdp::sdp_type::RTCSdpType::Answer;
                sdp.sdp = answer.sdp;
                peer_conn.set_remote_description(sdp).await?;
            }
            api::negotiate_response::Which::IceCandidate(_) => {
                return Err(Error::InvalidHandshake);
            }
        };
        sink.close().await?;
        Ok((peer_conn, r, side))
    }
}
