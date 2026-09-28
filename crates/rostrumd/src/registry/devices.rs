//! Paired devices, persisted in `<state_dir>/devices.json`.
//!
//! The file holds each device's [`TokenHash`] and never the token: a copy of
//! the file (a backup, a dotfiles repo) does not let anyone in. It is
//! rewritten atomically with mode `0600` on every change, and memory is only
//! changed once the write has succeeded, so what the daemon believes and what
//! a restart would load cannot drift apart.

use std::{
    net::IpAddr,
    path::{Path, PathBuf},
};

use chrono::{DateTime, TimeDelta, Utc};
use rostrum_remote::{DeviceId, TokenHash};
use serde::{Deserialize, Serialize};

use crate::state_file::{StoreError, read_json, write_json};

/// A device's `last_seen`/`last_ip` are rewritten at most this often, so a
/// phone polling every few seconds does not rewrite the file every few
/// seconds. A changed address is written straight away.
pub const TOUCH_DEBOUNCE: TimeDelta = TimeDelta::seconds(60);
/// Device names come from the phone; they are shown on the page.
pub const MAX_NAME_CHARS: usize = 64;
const UNNAMED: &str = "Unnamed phone";

/// One paired device, as stored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceRecord {
    pub id: DeviceId,
    pub name: String,
    pub token_hash: TokenHash,
    pub paired_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub last_ip: IpAddr,
}

/// A paired device as the page shows it: everything but the hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceView {
    pub id: DeviceId,
    pub name: String,
    pub paired_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub last_ip: IpAddr,
}

impl From<&DeviceRecord> for DeviceView {
    fn from(record: &DeviceRecord) -> Self {
        Self {
            id: record.id.clone(),
            name: record.name.clone(),
            paired_at: record.paired_at,
            last_seen: record.last_seen,
            last_ip: record.last_ip,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DevicesFile {
    devices: Vec<DeviceRecord>,
}

/// Which earlier devices a new pairing replaced, and on what evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Superseded {
    /// The phone presented this device's token in `replaces`: proof that it
    /// is the same phone.
    Token(DeviceId),
    /// No token matched, and these devices had exactly the new device's name
    /// — a reinstalled phone whose token was wiped. Never empty.
    Name(Vec<DeviceId>),
}

/// The paired devices and the file they live in.
#[derive(Debug)]
pub struct DeviceBook {
    path: PathBuf,
    devices: Vec<DeviceRecord>,
}

impl DeviceBook {
    /// Load `path`, or start empty when it does not exist. A file that exists
    /// but does not parse is an error: starting empty would unpair every
    /// phone and then overwrite the evidence on the next pairing.
    pub fn load(path: PathBuf) -> Result<Self, StoreError> {
        let devices = read_json::<DevicesFile>(&path)?
            .map(|file| file.devices)
            .unwrap_or_default();
        Ok(Self { path, devices })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn records(&self) -> &[DeviceRecord] {
        &self.devices
    }

    pub fn views(&self) -> Vec<DeviceView> {
        self.devices.iter().map(DeviceView::from).collect()
    }

    /// Add a device and persist.
    pub fn add(&mut self, record: DeviceRecord) -> Result<(), StoreError> {
        let mut next = self.devices.clone();
        next.push(record);
        self.commit(next)
    }

    /// Record a newly paired device, dropping the device(s) it replaces, in
    /// one write of the file.
    ///
    /// `replaces` is the hash of the token the phone held before. When it
    /// matches a device, that device — and only that one — is dropped.
    /// Otherwise every device whose name is exactly `record.name` is dropped.
    /// A hash that matches nothing is ignored. If the write fails nothing
    /// changes, in memory or on disk.
    pub fn pair(
        &mut self,
        record: DeviceRecord,
        replaces: Option<&TokenHash>,
    ) -> Result<Option<Superseded>, StoreError> {
        let superseded = match replaces.and_then(|hash| self.find(hash)) {
            Some(ix) => Some(Superseded::Token(self.devices[ix].id.clone())),
            None => {
                let same_name: Vec<DeviceId> = self
                    .devices
                    .iter()
                    .filter(|device| device.name == record.name)
                    .map(|device| device.id.clone())
                    .collect();
                (!same_name.is_empty()).then_some(Superseded::Name(same_name))
            }
        };
        let dropped: &[DeviceId] = match &superseded {
            Some(Superseded::Token(id)) => std::slice::from_ref(id),
            Some(Superseded::Name(ids)) => ids,
            None => &[],
        };
        let mut next: Vec<DeviceRecord> = self
            .devices
            .iter()
            .filter(|device| !dropped.contains(&device.id))
            .cloned()
            .collect();
        next.push(record);
        self.commit(next)?;
        Ok(superseded)
    }

    /// The device holding a token with this hash. Compares against every
    /// stored hash without an early exit.
    pub fn find(&self, hash: &TokenHash) -> Option<usize> {
        let mut found = None;
        for (ix, device) in self.devices.iter().enumerate() {
            if device.token_hash.ct_eq(hash) {
                found = Some(ix);
            }
        }
        found
    }

    pub fn get(&self, ix: usize) -> Option<&DeviceRecord> {
        self.devices.get(ix)
    }

    /// Note that device `ix` was just seen from `ip`. Persists only when the
    /// address changed or [`TOUCH_DEBOUNCE`] has passed; returns whether it
    /// did.
    pub fn touch(&mut self, ix: usize, ip: IpAddr, now: DateTime<Utc>) -> Result<bool, StoreError> {
        let Some(device) = self.devices.get(ix) else {
            return Ok(false);
        };
        let ip = ip.to_canonical();
        if device.last_ip == ip && now - device.last_seen < TOUCH_DEBOUNCE {
            return Ok(false);
        }
        let mut next = self.devices.clone();
        next[ix].last_seen = now;
        next[ix].last_ip = ip;
        self.commit(next)?;
        Ok(true)
    }

    /// Forget a device and persist. Returns whether it existed.
    pub fn remove(&mut self, id: &DeviceId) -> Result<bool, StoreError> {
        if !self.devices.iter().any(|device| &device.id == id) {
            return Ok(false);
        }
        let next = self
            .devices
            .iter()
            .filter(|device| &device.id != id)
            .cloned()
            .collect();
        self.commit(next)?;
        Ok(true)
    }

    fn commit(&mut self, next: Vec<DeviceRecord>) -> Result<(), StoreError> {
        write_json(
            &self.path,
            &DevicesFile {
                devices: next.clone(),
            },
        )?;
        self.devices = next;
        Ok(())
    }
}

/// A device name as the phone sent it, made safe to store and show: control
/// characters dropped, whitespace collapsed, at most [`MAX_NAME_CHARS`].
pub fn clean_name(raw: &str) -> String {
    let collapsed = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let name: String = collapsed.chars().take(MAX_NAME_CHARS).collect();
    let name = name.trim().to_string();
    if name.is_empty() {
        UNNAMED.to_string()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use rostrum_remote::DeviceToken;

    use super::*;
    use crate::fsutil::ScratchDir;

    fn record(n: u8, token: &DeviceToken) -> DeviceRecord {
        let at = DateTime::parse_from_rfc3339("2026-09-28T12:00:00Z")
            .expect("time")
            .with_timezone(&Utc);
        DeviceRecord {
            id: DeviceId::from_bytes([n; 16]),
            name: format!("Phone {n}"),
            token_hash: token.hash(),
            paired_at: at,
            last_seen: at,
            last_ip: "192.168.0.50".parse().expect("ip"),
        }
    }

    #[test]
    fn devices_round_trip_through_the_file() {
        let scratch = ScratchDir::new("devices-round-trip");
        let path = scratch.join("state/devices.json");
        let token = DeviceToken::from_bytes([1; 32]);
        let mut book = DeviceBook::load(path.clone()).expect("empty");
        assert!(book.records().is_empty());
        book.add(record(1, &token)).expect("add");
        book.add(record(2, &DeviceToken::from_bytes([2; 32])))
            .expect("add");

        let reloaded = DeviceBook::load(path).expect("load");
        assert_eq!(reloaded.records(), book.records());
        assert_eq!(reloaded.find(&token.hash()), Some(0));
    }

    #[test]
    fn the_file_is_private_and_holds_hashes_never_tokens() {
        let scratch = ScratchDir::new("devices-private");
        let path = scratch.join("devices.json");
        let token = DeviceToken::from_bytes([7; 32]);
        let mut book = DeviceBook::load(path.clone()).expect("empty");
        book.add(record(1, &token)).expect("add");

        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(
            !text.contains(token.expose()),
            "the token must not be stored"
        );
        assert!(text.contains(&token.hash().to_hex()));
        for field in [
            "id",
            "name",
            "token_hash",
            "paired_at",
            "last_seen",
            "last_ip",
        ] {
            assert!(text.contains(&format!("\"{field}\"")), "{field}");
        }
    }

    #[test]
    fn revoking_removes_the_device_from_memory_and_disk() {
        let scratch = ScratchDir::new("devices-revoke");
        let path = scratch.join("devices.json");
        let token = DeviceToken::from_bytes([3; 32]);
        let mut book = DeviceBook::load(path.clone()).expect("empty");
        book.add(record(3, &token)).expect("add");
        let id = DeviceId::from_bytes([3; 16]);

        assert!(book.remove(&id).expect("remove"));
        assert_eq!(book.find(&token.hash()), None);
        assert!(!book.remove(&id).expect("second remove"));
        assert!(DeviceBook::load(path).expect("load").records().is_empty());
    }

    #[test]
    fn an_unknown_hash_finds_nothing() {
        let scratch = ScratchDir::new("devices-unknown");
        let mut book = DeviceBook::load(scratch.join("devices.json")).expect("empty");
        book.add(record(1, &DeviceToken::from_bytes([1; 32])))
            .expect("add");
        assert_eq!(book.find(&DeviceToken::from_bytes([9; 32]).hash()), None);
    }

    #[test]
    fn touching_is_debounced_unless_the_address_changes() {
        let scratch = ScratchDir::new("devices-touch");
        let path = scratch.join("devices.json");
        let mut book = DeviceBook::load(path.clone()).expect("empty");
        let original = record(1, &DeviceToken::from_bytes([1; 32]));
        let seen = original.last_seen;
        book.add(original).expect("add");
        let same_ip: IpAddr = "192.168.0.50".parse().expect("ip");

        assert!(
            !book
                .touch(0, same_ip, seen + TimeDelta::seconds(30))
                .expect("touch")
        );
        assert_eq!(book.records()[0].last_seen, seen);

        assert!(
            book.touch(
                0,
                "100.64.0.20".parse().expect("ip"),
                seen + TimeDelta::seconds(31)
            )
            .expect("touch")
        );
        assert_eq!(
            book.records()[0].last_ip,
            "100.64.0.20".parse::<IpAddr>().expect("ip")
        );

        let later = seen + TimeDelta::seconds(31) + TOUCH_DEBOUNCE;
        assert!(
            book.touch(0, "100.64.0.20".parse().expect("ip"), later)
                .expect("touch")
        );
        assert_eq!(
            DeviceBook::load(path).expect("load").records()[0].last_seen,
            later
        );
    }

    #[test]
    fn a_mapped_address_is_stored_as_ipv4() {
        let scratch = ScratchDir::new("devices-mapped");
        let mut book = DeviceBook::load(scratch.join("devices.json")).expect("empty");
        let original = record(1, &DeviceToken::from_bytes([1; 32]));
        let later = original.last_seen + TOUCH_DEBOUNCE;
        book.add(original).expect("add");
        book.touch(0, "::ffff:10.0.0.9".parse().expect("ip"), later)
            .expect("touch");
        assert_eq!(
            book.records()[0].last_ip,
            "10.0.0.9".parse::<IpAddr>().expect("ip")
        );
    }

    #[test]
    fn a_corrupt_file_is_an_error_not_an_empty_book() {
        let scratch = ScratchDir::new("devices-corrupt");
        let path = scratch.join("devices.json");
        std::fs::write(&path, "{\"devices\": [{]}").expect("write");
        assert!(matches!(
            DeviceBook::load(path),
            Err(StoreError::Parse { .. })
        ));
    }

    #[test]
    fn a_failed_write_leaves_memory_unchanged() {
        let scratch = ScratchDir::new("devices-readonly");
        let mut book = DeviceBook::load(scratch.join("blocker/devices.json")).expect("empty");
        // A path whose parent is a file cannot be written.
        std::fs::write(scratch.join("blocker"), "").expect("write");
        assert!(
            book.add(record(1, &DeviceToken::from_bytes([1; 32])))
                .is_err()
        );
        assert!(book.records().is_empty());
    }

    fn with(book: &mut DeviceBook, n: u8, name: &str) -> DeviceToken {
        let token = DeviceToken::from_bytes([n; 32]);
        let mut device = record(n, &token);
        device.name = name.into();
        book.add(device).expect("add");
        token
    }

    fn named(n: u8, name: &str) -> (DeviceRecord, DeviceToken) {
        let token = DeviceToken::from_bytes([n; 32]);
        let mut device = record(n, &token);
        device.name = name.into();
        (device, token)
    }

    #[test]
    fn pairing_with_a_known_token_supersedes_that_device_in_one_write() {
        let scratch = ScratchDir::new("devices-supersede-token");
        let path = scratch.join("devices.json");
        let mut book = DeviceBook::load(path.clone()).expect("empty");
        let old = with(&mut book, 1, "Pixel");
        with(&mut book, 2, "Tablet");
        let (new, _) = named(3, "Pixel (work)");

        let superseded = book.pair(new.clone(), Some(&old.hash())).expect("pair");
        assert_eq!(
            superseded,
            Some(Superseded::Token(DeviceId::from_bytes([1; 16])))
        );
        let names: Vec<&str> = book.records().iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["Tablet", "Pixel (work)"]);
        assert_eq!(
            DeviceBook::load(path).expect("load").records(),
            book.records()
        );
    }

    #[test]
    fn pairing_under_an_existing_name_supersedes_every_device_so_named() {
        let scratch = ScratchDir::new("devices-supersede-name");
        let mut book = DeviceBook::load(scratch.join("devices.json")).expect("empty");
        with(&mut book, 1, "Google Pixel 9");
        with(&mut book, 2, "Google Pixel 9");
        with(&mut book, 3, "Tablet");
        let (new, _) = named(4, "Google Pixel 9");
        let superseded = book.pair(new, None).expect("pair");
        assert_eq!(
            superseded,
            Some(Superseded::Name(vec![
                DeviceId::from_bytes([1; 16]),
                DeviceId::from_bytes([2; 16])
            ]))
        );
        assert_eq!(book.records().len(), 2);
    }

    #[test]
    fn the_token_path_takes_precedence_over_the_name_path() {
        let scratch = ScratchDir::new("devices-supersede-precedence");
        let mut book = DeviceBook::load(scratch.join("devices.json")).expect("empty");
        let old = with(&mut book, 1, "Pixel");
        with(&mut book, 2, "Tablet");
        // The token names device 1; device 2 shares the new name but stays.
        let (new, _) = named(3, "Tablet");
        let superseded = book.pair(new, Some(&old.hash())).expect("pair");
        assert_eq!(
            superseded,
            Some(Superseded::Token(DeviceId::from_bytes([1; 16])))
        );
        assert_eq!(book.records().len(), 2);
    }

    #[test]
    fn an_unknown_replaces_hash_and_a_new_name_keep_everything() {
        let scratch = ScratchDir::new("devices-supersede-none");
        let mut book = DeviceBook::load(scratch.join("devices.json")).expect("empty");
        with(&mut book, 1, "Pixel");
        let (new, _) = named(2, "Tablet");
        let bogus = DeviceToken::from_bytes([9; 32]).hash();
        assert_eq!(book.pair(new, Some(&bogus)).expect("pair"), None);
        assert_eq!(book.records().len(), 2);
    }

    #[test]
    fn a_failed_pairing_write_supersedes_nothing() {
        let scratch = ScratchDir::new("devices-supersede-fail");
        let mut book = DeviceBook::load(scratch.join("blocker/devices.json")).expect("empty");
        std::fs::create_dir_all(scratch.join("blocker")).expect("dir");
        let old = with(&mut book, 1, "Pixel");
        std::fs::remove_dir_all(scratch.join("blocker")).expect("rm");
        std::fs::write(scratch.join("blocker"), "").expect("block");
        let (new, _) = named(2, "Pixel");
        assert!(book.pair(new, Some(&old.hash())).is_err());
        assert_eq!(book.records().len(), 1);
        assert_eq!(book.find(&old.hash()), Some(0));
    }

    #[test]
    fn names_from_the_phone_are_cleaned() {
        assert_eq!(clean_name("  Pixel\t8\n Pro "), "Pixel 8 Pro");
        assert_eq!(clean_name("\u{0}\u{7}"), UNNAMED);
        assert_eq!(clean_name(""), UNNAMED);
        assert_eq!(clean_name(&"x".repeat(200)).chars().count(), MAX_NAME_CHARS);
        assert_eq!(
            clean_name("<script>"),
            "<script>",
            "escaping is the page's job"
        );
    }
}
