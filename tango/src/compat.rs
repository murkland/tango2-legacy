use serde::de::Error;

fn from_hex<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: &str = serde::Deserialize::deserialize(deserializer)?;
    u32::from_str_radix(&s, 16).map_err(D::Error::custom)
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
    pub hooks: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct Raw {
    pub games: std::collections::HashMap<String, Game>,
    pub compatibility: Vec<std::collections::HashSet<String>>,
}

#[derive(Clone)]
pub struct CompatList {
    games: std::collections::HashMap<String, Game>,
    title_and_crc32_to_id: std::collections::HashMap<(String, u32), String>,
    compatibility: Vec<std::collections::HashSet<String>>,
}

impl CompatList {
    fn from_raw(raw: Raw) -> Self {
        let title_and_crc32_to_id = raw
            .games
            .iter()
            .map(|(k, v)| ((v.title.clone(), v.crc32), k.clone()))
            .collect();
        Self {
            games: raw.games,
            title_and_crc32_to_id,
            compatibility: raw.compatibility,
        }
    }

    pub fn id_by_title_and_crc32(&self, title: &str, crc32: u32) -> Option<&String> {
        self.title_and_crc32_to_id.get(&(title.to_string(), crc32))
    }

    pub fn game_by_id(&self, id: &str) -> Option<&Game> {
        self.games.get(&id.to_string())
    }

    pub fn is_compatible(&self, id1: &str, id2: &str) -> bool {
        self.compatibility
            .iter()
            .any(|ids| ids.contains(id1) && ids.contains(id2))
    }
}

const COMPAT_FILE: &str = "games.toml";

pub fn load() -> anyhow::Result<CompatList> {
    Ok(CompatList::from_raw(toml::from_slice(&std::fs::read(
        COMPAT_FILE,
    )?)?))
}
