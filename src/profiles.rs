use crate::tunnel::Profile;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedProfile {
    pub name: String,
    pub connection: Profile,
    pub mode: Mode,
    pub authentication: Authentication,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encrypted_secret: Option<Vec<u8>>,
    #[serde(default)]
    pub ipv4_routing: Ipv4Routing,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ipv4Routing {
    pub split: bool,
    pub cidrs: String,
    #[serde(default)]
    pub by_application: bool,
    #[serde(default)]
    pub applications: Vec<PathBuf>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Ipv4Prefix {
    network: Ipv4Addr,
    length: u8,
}

impl Ipv4Prefix {
    pub fn contains(self, address: Ipv4Addr) -> bool {
        let mask = u32::MAX << (32 - self.length);
        u32::from(address) & mask == u32::from(self.network)
    }

    pub fn is_host(self) -> bool {
        self.length == 32
    }
}

impl std::fmt::Display for Ipv4Prefix {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(output, "{}/{}", self.network, self.length)
    }
}

impl Ipv4Routing {
    pub fn application_paths(&self) -> Result<Vec<String>, String> {
        if self.applications.is_empty() {
            return Err("Добавьте хотя бы один исполняемый файл (.exe).".into());
        }
        self.applications
            .iter()
            .map(|path| {
                let valid = path.is_absolute()
                    && path
                        .extension()
                        .and_then(|part| part.to_str())
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"));
                if !valid || !path.is_file() {
                    return Err(format!("Приложение недоступно: {}", path.display()));
                }
                let canonical = path
                    .canonicalize()
                    .map_err(|error| format!("{}: {error}", path.display()))?;
                let canonical = canonical
                    .to_str()
                    .ok_or("Путь к приложению содержит неподдерживаемые символы.")?;
                let path = if let Some(unc) = canonical.strip_prefix(r"\\?\UNC\") {
                    format!(r"\\{unc}")
                } else {
                    canonical
                        .strip_prefix(r"\\?\")
                        .unwrap_or(canonical)
                        .to_owned()
                };
                Ok(path)
            })
            .collect()
    }

    pub fn routes(&self) -> Result<Option<Vec<Ipv4Prefix>>, String> {
        if !self.split {
            return Ok(None);
        }
        let mut routes = Vec::new();
        for entry in self
            .cidrs
            .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        {
            if entry.is_empty() {
                continue;
            }
            let parsed = entry.split_once('/').and_then(|(address, length)| {
                let address = address.parse::<Ipv4Addr>().ok()?;
                let length = length.parse::<u8>().ok().filter(|n| (1..=32).contains(n))?;
                let network = Ipv4Addr::from(u32::from(address) & (u32::MAX << (32 - length)));
                Some(Ipv4Prefix { network, length })
            });
            let prefix = parsed.ok_or_else(|| {
                format!("Неверный IPv4 CIDR: {entry}. Используйте префикс от /1 до /32.")
            })?;
            if !routes.contains(&prefix) {
                routes.push(prefix);
            }
        }
        if routes.is_empty() {
            return Err("Добавьте хотя бы один IPv4 CIDR.".into());
        }
        Ok(Some(routes))
    }
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
            ipv4_routing: Ipv4Routing::default(),
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
                        ipv4_routing: Ipv4Routing::default(),
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
    fn split_routes_normalize_prefixes_and_reject_invalid_cidrs() {
        let mut routing = Ipv4Routing {
            split: true,
            cidrs: "192.0.2.8/24, 192.0.2.0/24\n198.51.100.7/32".into(),
            ..Ipv4Routing::default()
        };
        let prefixes = routing.routes().unwrap().unwrap();
        assert_eq!(prefixes.len(), 2);
        assert_eq!(prefixes[0].to_string(), "192.0.2.0/24");
        assert!(prefixes[0].contains(Ipv4Addr::new(192, 0, 2, 255)));
        assert!(!prefixes[0].contains(Ipv4Addr::new(192, 0, 3, 0)));
        assert!(prefixes[1].is_host());
        for invalid in ["", "0.0.0.0/0", "2001:db8::/32", "192.0.2.9/33"] {
            routing.cidrs = invalid.into();
            assert!(routing.routes().is_err());
        }
        routing.split = false;
        assert!(routing.routes().unwrap().is_none());
    }

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
