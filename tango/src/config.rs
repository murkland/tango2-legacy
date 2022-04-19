use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Keymapping {
    pub up: winit::event::VirtualKeyCode,
    pub down: winit::event::VirtualKeyCode,
    pub left: winit::event::VirtualKeyCode,
    pub right: winit::event::VirtualKeyCode,
    pub a: winit::event::VirtualKeyCode,
    pub b: winit::event::VirtualKeyCode,
    pub l: winit::event::VirtualKeyCode,
    pub r: winit::event::VirtualKeyCode,
    pub select: winit::event::VirtualKeyCode,
    pub start: winit::event::VirtualKeyCode,
}

impl Default for Keymapping {
    fn default() -> Self {
        Self {
            up: winit::event::VirtualKeyCode::Up,
            down: winit::event::VirtualKeyCode::Down,
            left: winit::event::VirtualKeyCode::Left,
            right: winit::event::VirtualKeyCode::Right,
            a: winit::event::VirtualKeyCode::Z,
            b: winit::event::VirtualKeyCode::X,
            l: winit::event::VirtualKeyCode::A,
            r: winit::event::VirtualKeyCode::S,
            select: winit::event::VirtualKeyCode::Back,
            start: winit::event::VirtualKeyCode::Return,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ICEServer {
    pub urls: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WebRTC {
    pub ice_servers: Vec<ICEServer>,
}

impl Default for WebRTC {
    fn default() -> Self {
        Self {
            ice_servers: vec![
                ICEServer {
                    urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                },
                ICEServer {
                    urls: vec!["stun:stun1.l.google.com:19302".to_owned()],
                },
                ICEServer {
                    urls: vec!["stun:stun2.l.google.com:19302".to_owned()],
                },
                ICEServer {
                    urls: vec!["stun:stun3.l.google.com:19302".to_owned()],
                },
                ICEServer {
                    urls: vec!["stun:stun4.l.google.com:19302".to_owned()],
                },
            ],
        }
    }
}

impl WebRTC {
    pub fn make_webrtc_config(&self) -> webrtc::peer_connection::configuration::RTCConfiguration {
        webrtc::peer_connection::configuration::RTCConfiguration {
            ice_servers: self
                .ice_servers
                .iter()
                .map(
                    |ice_server| webrtc::ice_transport::ice_server::RTCIceServer {
                        urls: ice_server.urls.clone(),
                        ..Default::default()
                    },
                )
                .collect(),
            ..Default::default()
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Matchmaking {
    pub connect_addr: String,
    pub bind_addr: String,
}

impl Default for Matchmaking {
    fn default() -> Self {
        Self {
            connect_addr: "wss://mm.tango.murk.land".to_owned(),
            bind_addr: "[::]:1984".to_owned(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Logging {
    pub filters: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Config {
    pub logging: Logging,
    pub keymapping: Keymapping,
    pub matchmaking: Matchmaking,
    pub webrtc: WebRTC,
}

const CONFIG_FILE: &str = "tango.toml";

pub fn save(config: &Config) -> anyhow::Result<()> {
    std::fs::write(CONFIG_FILE, toml::to_vec(config)?)?;
    Ok(())
}

pub fn load() -> anyhow::Result<Config> {
    Ok(toml::from_slice(&std::fs::read(CONFIG_FILE)?)?)
}
