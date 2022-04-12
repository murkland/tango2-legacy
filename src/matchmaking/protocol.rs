use bincode::Options;

lazy_static! {
    static ref BINCODE_OPTIONS: bincode::config::WithOtherIntEncoding<
        bincode::config::DefaultOptions,
        bincode::config::FixintEncoding,
    > = bincode::DefaultOptions::new().with_fixint_encoding();
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum Packet {
    Start(Start),
    Offer(Offer),
    Answer(Answer),
    ICECandidate(ICECandidate),
}

impl Packet {
    pub fn serialize(&self) -> bincode::Result<Vec<u8>> {
        BINCODE_OPTIONS.serialize(self)
    }

    pub fn deserialize(d: &[u8]) -> bincode::Result<Self> {
        BINCODE_OPTIONS.deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Start {
    pub session_id: String,
    pub offer_sdp: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Offer {
    pub sdp: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Answer {
    pub sdp: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct ICECandidate {
    pub ice_candidate: String,
}
