#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Packet {
    #[prost(oneof = "packet::Which", tags = "1, 2, 3, 4")]
    pub foo: Option<packet::Which>,
}

pub mod packet {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Which {
        #[prost(message, tag = "1")]
        Hello(super::Hello),
        #[prost(message, tag = "2")]
        Hello2(super::Hello2),
        #[prost(message, tag = "3")]
        Init(super::Init),
        #[prost(message, tag = "4")]
        Input(super::Input),
    }
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Hello {
    #[prost(uint32, tag = "1")]
    pub protocol_version: u32,

    #[prost(string, tag = "2")]
    pub game_title: String,

    #[prost(fixed32, tag = "3")]
    pub game_crc32: u32,

    #[prost(uint32, tag = "4")]
    pub match_type: u32,

    #[prost(bytes, tag = "5")]
    pub rng_commitment: Vec<u8>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Hello2 {
    #[prost(bytes, tag = "1")]
    pub rng_nonce: Vec<u8>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Init {
    #[prost(uint32, tag = "1")]
    pub battle_number: u32,

    #[prost(uint32, tag = "2")]
    pub input_delay: u32,

    #[prost(bytes, tag = "3")]
    pub marshaled: Vec<u8>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Input {
    #[prost(uint32, tag = "1")]
    pub battle_number: u32,

    #[prost(uint32, tag = "2")]
    pub local_tick: u32,

    #[prost(uint32, tag = "3")]
    pub remote_tick: u32,

    #[prost(uint32, tag = "4")]
    pub joyflags: u32,

    #[prost(uint32, tag = "5")]
    pub custom_screen_state: u32,

    #[prost(bytes, tag = "6")]
    pub turn: Vec<u8>,
}
