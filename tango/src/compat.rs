use serde::de::Error;

fn from_hex<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: &str = serde::Deserialize::deserialize(deserializer)?;
    u32::from_str_radix(&s[2..], 16).map_err(D::Error::custom)
}

fn to_hex<S>(v: &u32, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serde::Serialize::serialize(&format!("{:08x}", v), serializer)
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Game {
    pub title: String,
    #[serde(deserialize_with = "from_hex", serialize_with = "to_hex")]
    pub crc32: u32,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Entry {
    pub game: Game,
    pub compatible_with: Vec<Game>,
}

pub type List = Vec<Entry>;
