use crate::profiles::Ipv4Prefix;
use crate::tunnel::system_program;
use std::fs;
use std::io::Write;
use std::net::Ipv4Addr;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

const SOURCE: &str = "https://www.scanitex.com/ru/resources/ip-ranges/ru/download/cidr";
const MAX_BYTES: usize = 512 * 1024;
const REFRESH_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
const IPV4_END: u64 = 1 << 32;

pub struct RuRanges(Vec<(u64, u64)>);

fn interval(prefix: Ipv4Prefix) -> (u64, u64) {
    let (address, length) = prefix.parts();
    let start = u64::from(u32::from(address));
    (start, start + (1 << (32 - length)))
}

impl RuRanges {
    fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_BYTES {
            return Err("Список IPv4 РФ слишком большой.".into());
        }
        let text = std::str::from_utf8(bytes).map_err(|_| "Список IPv4 РФ должен быть в UTF-8.")?;
        let mut ranges = text
            .trim_start_matches('\u{feff}')
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(|line| {
                line.contains('/')
                    .then(|| Ipv4Prefix::parse(line))
                    .flatten()
                    .map(interval)
                    .ok_or("ScaniteX вернул некорректный список IPv4.")
            })
            .collect::<Result<Vec<_>, _>>()?;
        ranges.sort_unstable();
        let mut merged: Vec<(u64, u64)> = Vec::new();
        for (start, end) in ranges {
            if let Some(last) = merged.last_mut().filter(|last| start <= last.1) {
                last.1 = last.1.max(end);
            } else {
                merged.push((start, end));
            }
        }
        let addresses = merged.iter().map(|(start, end)| end - start).sum::<u64>();
        if merged.is_empty() || addresses >= IPV4_END / 2 {
            return Err("ScaniteX вернул некорректный список IPv4.".into());
        }
        Ok(Self(merged))
    }

    pub fn validate_included(&self, routes: &[Ipv4Prefix]) -> Result<(), String> {
        for &route in routes {
            let (start, end) = interval(route);
            let index = self.0.partition_point(|range| range.1 <= start);
            if self.0.get(index).is_some_and(|range| range.0 < end) {
                return Err(format!(
                    "В списке есть IP из РФ ({route}). Выключите «Исключение РФ Зоны», чтобы добавить этот адрес или диапазон."
                ));
            }
        }
        Ok(())
    }

    pub fn non_ru_prefixes(&self) -> Vec<(Ipv4Addr, u8)> {
        let mut prefixes = Vec::new();
        let mut start = 0;
        for &(next, end) in self.0.iter().chain([(IPV4_END, IPV4_END)].iter()) {
            while start < next {
                let available = (next - start).ilog2();
                let bits = start.trailing_zeros().min(available);
                prefixes.push((Ipv4Addr::from(start as u32), (32 - bits) as u8));
                start += 1 << bits;
            }
            start = end;
        }
        prefixes
    }
}

pub fn load(directory: &Path) -> Result<RuRanges, String> {
    let path = directory.join("ru-ipv4.cidr");
    let metadata = fs::metadata(&path).ok();
    let cached = metadata
        .as_ref()
        .filter(|meta| meta.len() <= MAX_BYTES as u64)
        .and_then(|_| fs::read(&path).ok())
        .and_then(|bytes| RuRanges::parse(&bytes).ok());
    let fresh = metadata
        .and_then(|meta| meta.modified().ok())
        .and_then(|stamp| SystemTime::now().duration_since(stamp).ok())
        .is_some_and(|age| age < REFRESH_AFTER);
    if fresh {
        if let Some(ranges) = cached {
            return Ok(ranges);
        }
    }
    refresh(directory, &path).or_else(|error| {
        cached.ok_or_else(|| format!("Не удалось загрузить список IPv4 РФ из ScaniteX. {error}"))
    })
}

fn refresh(directory: &Path, path: &Path) -> Result<RuRanges, String> {
    let result = Command::new(system_program("curl.exe")?)
        .creation_flags(0x0800_0000)
        .args([
            "--disable",
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "5",
            "--max-time",
            "12",
            "--max-filesize",
            "524288",
            SOURCE,
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| error.to_string())?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).trim().to_string());
    }
    let ranges = RuRanges::parse(&result.stdout)?;
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(directory).map_err(|error| error.to_string())?;
    file.write_all(&result.stdout).map_err(|error| error.to_string())?;
    file.persist(path).map_err(|error| error.to_string())?;
    Ok(ranges)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_russian_address_and_any_overlapping_cidr() {
        let ranges = RuRanges::parse(b"203.0.113.0/25\n203.0.113.128/25\n").unwrap();
        assert_eq!(ranges.0.len(), 1);
        for input in ["203.0.113.255", "203.0.112.0/23"] {
            let error = ranges
                .validate_included(&[Ipv4Prefix::parse(input).unwrap()])
                .unwrap_err();
            assert!(error.contains("Выключите «Исключение РФ Зоны»"));
        }
        assert!(ranges
            .validate_included(&[Ipv4Prefix::parse("203.0.112.255").unwrap()])
            .is_ok());
    }

    #[test]
    fn generated_routes_cover_exactly_the_complement() {
        let ranges = RuRanges::parse(b"0.0.0.0/32\n203.0.113.0/24\n255.255.255.255/32\n").unwrap();
        let mut coverage = ranges.0.clone();
        for (ip, len) in ranges.non_ru_prefixes() {
            let prefix = Ipv4Prefix::parse(&format!("{ip}/{len}")).unwrap();
            assert!(ranges.validate_included(&[prefix]).is_ok());
            coverage.push(interval(prefix));
        }
        coverage.sort_unstable();
        assert_eq!(coverage.first().unwrap().0, 0);
        assert_eq!(coverage.last().unwrap().1, IPV4_END);
        assert!(coverage.windows(2).all(|pair| pair[0].1 == pair[1].0));
    }

    #[test]
    fn uses_a_fresh_cache_and_rejects_invalid_downloads() {
        for input in ["", "<html>", "203.0.113.1", "0.0.0.0/0"] {
            assert!(RuRanges::parse(input.as_bytes()).is_err());
        }
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("ru-ipv4.cidr"), "203.0.113.0/24\r\n").unwrap();
        assert_eq!(load(directory.path()).unwrap().0.len(), 1);
    }
}
