use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
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

#[derive(Serialize, Deserialize, Debug)]
pub struct ICEServer {
    pub urls: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
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

#[derive(Serialize, Deserialize, Debug)]
pub struct Matchmaking {
    pub connect_addr: String,
}

impl Default for Matchmaking {
    fn default() -> Self {
        Self {
            connect_addr: "https://mm.tango.murk.land".to_owned(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    pub keymapping: Keymapping,
    pub matchmaking: Matchmaking,
    pub webrtc: WebRTC,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            keymapping: Default::default(),
            matchmaking: Default::default(),
            webrtc: Default::default(),
        }
    }
}

const CONFIG_FILE: &str = "tango.toml";

pub fn save_config(config: &Config) -> anyhow::Result<()> {
    std::fs::write(CONFIG_FILE, toml::to_vec(config)?)?;
    Ok(())
}

pub fn load_config() -> anyhow::Result<Config> {
    Ok(toml::from_slice(&std::fs::read(CONFIG_FILE)?)?)
}
