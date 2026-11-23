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
const FALLBACK_SOURCE: &str = "https://www.ipdeny.com/ipblocks/data/aggregated/ru-aggregated.zone";
const MAX_BYTES: usize = 512 * 1024;
const REFRESH_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
const IPV4_END: u64 = 1 << 32;

pub struct RuRanges {
    ranges: Vec<(u64, u64)>,
    pub from_ipdeny: bool,
}

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
                    .ok_or("Получен некорректный список IPv4.")
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
            return Err("Получен некорректный список IPv4.".into());
        }
        Ok(Self {
            ranges: merged,
            from_ipdeny: false,
        })
    }

    pub fn validate_included(&self, routes: &[Ipv4Prefix]) -> Result<(), String> {
        for &route in routes {
            let (start, end) = interval(route);
            let index = self.ranges.partition_point(|range| range.1 <= start);
            if self.ranges.get(index).is_some_and(|range| range.0 < end) {
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
        for &(next, end) in self.ranges.iter().chain([(IPV4_END, IPV4_END)].iter()) {
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
    load_with(directory, download)
}

fn open_cache(path: &Path, fallback: bool) -> Option<(RuRanges, bool)> {
    let metadata = fs::metadata(path).ok()?;
    if metadata.len() > MAX_BYTES as u64 {
        return None;
    }
    let mut ranges = RuRanges::parse(&fs::read(path).ok()?).ok()?;
    ranges.from_ipdeny = fallback;
    let fresh = metadata
        .modified()
        .ok()
        .and_then(|stamp| SystemTime::now().duration_since(stamp).ok())
        .is_some_and(|age| age < REFRESH_AFTER);
    Some((ranges, fresh))
}

fn load_with(
    directory: &Path,
    fetch: impl Fn(&str, &str) -> Result<Vec<u8>, String>,
) -> Result<RuRanges, String> {
    let scanitex = directory.join("ru-ipv4.cidr");
    let ipdeny = directory.join("ru-ipv4-ipdeny.cidr");
    let previous_scanitex = match open_cache(&scanitex, false) {
        Some((ranges, true)) => return Ok(ranges),
        other => other.map(|(ranges, _)| ranges),
    };
    let previous_ipdeny = match open_cache(&ipdeny, true) {
        Some((ranges, true)) => return Ok(ranges),
        other => other.map(|(ranges, _)| ranges),
    };

    let scanitex_error = match fetch(SOURCE, "45").and_then(|data| save(&scanitex, &data, false)) {
        Ok(ranges) => return Ok(ranges),
        Err(error) => error,
    };
    if let Some(ranges) = previous_scanitex {
        return Ok(ranges);
    }
    fetch(FALLBACK_SOURCE, "120")
        .and_then(|data| save(&ipdeny, &data, true))
        .or_else(|error| {
            previous_ipdeny.ok_or_else(|| {
                format!("Не удалось загрузить список IPv4 РФ. ScaniteX: {scanitex_error}. IPdeny: {error}")
            })
        })
}

fn download(url: &str, timeout: &str) -> Result<Vec<u8>, String> {
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
            "8",
            "--max-time",
            timeout,
            "--max-filesize",
            "524288",
            url,
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| error.to_string())?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).trim().to_string());
    }
    Ok(result.stdout)
}

fn save(path: &Path, data: &[u8], fallback: bool) -> Result<RuRanges, String> {
    let mut ranges = RuRanges::parse(data)?;
    ranges.from_ipdeny = fallback;
    let directory = path.parent().ok_or("Не найдена папка списка IPv4 РФ.")?;
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(directory).map_err(|error| error.to_string())?;
    file.write_all(data).map_err(|error| error.to_string())?;
    file.persist(path).map_err(|error| error.to_string())?;
    Ok(ranges)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_russian_address_and_any_overlapping_cidr() {
        let ranges = RuRanges::parse(b"203.0.113.0/25\n203.0.113.128/25\n").unwrap();
        assert_eq!(ranges.ranges.len(), 1);
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
        let mut coverage = ranges.ranges.clone();
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
        assert_eq!(load(directory.path()).unwrap().ranges.len(), 1);
    }

    #[test]
    fn unavailable_primary_can_use_a_separate_fallback_cache() {
        let directory = tempfile::tempdir().unwrap();
        let calls = std::cell::RefCell::new(Vec::new());
        let ranges = load_with(directory.path(), |url, timeout| {
            calls.borrow_mut().push((url.to_owned(), timeout.to_owned()));
            if url == SOURCE {
                Err("timeout".into())
            } else {
                Ok(b"203.0.113.0/24\n".to_vec())
            }
        })
        .unwrap();
        assert!(ranges.from_ipdeny);
        assert_eq!(
            calls.into_inner(),
            [
                (SOURCE.to_owned(), "45".into()),
                (FALLBACK_SOURCE.to_owned(), "120".into()),
            ]
        );
        assert!(!directory.path().join("ru-ipv4.cidr").exists());
        assert!(load_with(directory.path(), |_, _| panic!("fresh cache must be used"))
            .unwrap()
            .from_ipdeny);
    }
}
