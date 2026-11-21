use crate::tunnel::Profile;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedProfile {
    pub name: String,
    pub connection: Profile,
    pub mode: Mode,
    pub authentication: Authentication,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encrypted_secret: Option<Vec<u8>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    SystemVpn,
    Socks5,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::SystemVpn => "System VPN (IPv4)",
            Self::Socks5 => "SOCKS5",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Authentication {
    Password,
    Key,
}

impl Default for SavedProfile {
    fn default() -> Self {
        Self {
            name: String::new(),
            connection: Profile::default(),
            mode: Mode::SystemVpn,
            authentication: Authentication::Password,
            encrypted_secret: None,
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Profiles {
    pub entries: Vec<SavedProfile>,
    pub selected: Option<usize>,
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<T> {
    let json = fs::read_to_string(path)?;
    serde_json::from_str(json.trim_start_matches('\u{feff}')).map_err(io::Error::other)
}

impl Profiles {
    pub fn directory() -> io::Result<PathBuf> {
        let local = std::env::var_os("LOCALAPPDATA")
            .ok_or_else(|| io::Error::other("LOCALAPPDATA is unavailable."))?;
        Ok(PathBuf::from(local).join("SshTunnelClient"))
    }

    pub fn load(directory: &Path) -> io::Result<Self> {
        match read_json::<Self>(&directory.join("profiles.json")) {
            Ok(mut profiles) => {
                profiles.selected = profiles
                    .selected
                    .filter(|index| *index < profiles.entries.len());
                Ok(profiles)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let connection: Profile = match read_json(&directory.join("profile.json")) {
                    Ok(profile) => profile,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        return Ok(Self::default())
                    }
                    Err(error) => return Err(error),
                };
                let migrated = Self {
                    selected: Some(0),
                    entries: vec![SavedProfile {
                        name: if connection.server.trim().is_empty() {
                            "Imported profile".into()
                        } else {
                            connection.server.clone()
                        },
                        authentication: if connection.key.is_empty() {
                            Authentication::Password
                        } else {
                            Authentication::Key
                        },
                        connection,
                        mode: Mode::SystemVpn,
                        encrypted_secret: None,
                    }],
                };
                migrated.save(directory)?;
                Ok(migrated)
            }
            Err(error) => Err(error),
        }
    }

    pub fn save(&self, directory: &Path) -> io::Result<()> {
        fs::create_dir_all(directory)?;
        let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
        serde_json::to_writer_pretty(temporary.as_file_mut(), self)?;
        temporary.as_file_mut().flush()?;
        temporary.persist(directory.join("profiles.json"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_bom_profile_and_reopens_multiple_profiles() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("profile.json"), "\u{feff}{\"Server\":\"first.example\",\"User\":\"alice\",\"SocksPort\":2080,\"Key\":\"keyfile\"}").unwrap();
        let mut profiles = Profiles::load(temp.path()).unwrap();
        assert_eq!(profiles.selected, Some(0));
        assert!(profiles.entries[0].authentication == Authentication::Key);
        assert_eq!(profiles.entries[0].connection.socks_port, 2080);

        profiles.entries.push(SavedProfile {
            name: "Office".into(),
            mode: Mode::Socks5,
            ..SavedProfile::default()
        });
        profiles.selected = Some(1);
        profiles.save(temp.path()).unwrap();
        let reloaded = Profiles::load(temp.path()).unwrap();
        assert!(reloaded.entries == profiles.entries);
        assert_eq!(reloaded.selected, Some(1));

        profiles.entries.clear();
        profiles.selected = None;
        profiles.save(temp.path()).unwrap();
        assert!(Profiles::load(temp.path()).unwrap().entries.is_empty());
    }
}
