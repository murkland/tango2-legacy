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
    pub compatible_with: std::collections::HashSet<String>,
}

pub struct CompatList {
    games: std::collections::HashMap<String, Game>,
    title_and_crc32_to_id: std::collections::HashMap<(String, u32), String>,
}

impl CompatList {
    fn from_games(games: std::collections::HashMap<String, Game>) -> Self {
        let title_and_crc32_to_id = games
            .iter()
            .map(|(k, v)| ((v.title.clone(), v.crc32), k.clone()))
            .collect();
        Self {
            games,
            title_and_crc32_to_id,
        }
    }

    pub fn id_by_title_and_crc32(&self, title: &str, crc32: u32) -> Option<&String> {
        self.title_and_crc32_to_id.get(&(title.to_string(), crc32))
    }

    pub fn game_by_id(&self, id: &str) -> Option<&Game> {
        self.games.get(&id.to_string())
    }
}

const COMPAT_FILE: &str = "compat.toml";

pub fn load() -> anyhow::Result<CompatList> {
    Ok(CompatList::from_games(toml::from_slice(&std::fs::read(
        COMPAT_FILE,
    )?)?))
}
