//! Private, host-validated authority for one browser profile.
use super::clovy_browser_process_identity::process_birth;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::{Path, PathBuf},
};

pub(crate) fn plain_metadata(path: &Path) -> io::Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(io::Error::other("link rejected"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other("reparse point rejected"));
        }
    }
    Ok(metadata)
}

pub(crate) fn open_existing_plain(path: &Path, write: bool) -> io::Result<File> {
    if !plain_metadata(path)?.is_file() {
        return Err(io::Error::other("regular file required"));
    }
    let mut options = OpenOptions::new();
    options.read(true).write(write);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("regular file required"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if file.metadata()?.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other("reparse point rejected"));
        }
    }
    Ok(file)
}

pub(crate) fn open_file_identity(file: &File) -> io::Result<[u64; 2]> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = file.metadata()?;
        Ok([m.dev(), m.ino()])
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        #[repr(C)]
        #[derive(Default)]
        struct FileInfo {
            attributes: u32,
            created: [u32; 2],
            accessed: [u32; 2],
            written: [u32; 2],
            volume: u32,
            size_high: u32,
            size_low: u32,
            links: u32,
            index_high: u32,
            index_low: u32,
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GetFileInformationByHandle(
                handle: *mut std::ffi::c_void,
                info: *mut FileInfo,
            ) -> i32;
        }
        let mut info = FileInfo::default();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if info.attributes & 0x400 != 0 {
            return Err(io::Error::other("reparse point rejected"));
        }
        Ok([
            info.volume as u64,
            ((info.index_high as u64) << 32) | info.index_low as u64,
        ])
    }
}

pub(crate) fn profile_object_identity(path: &Path) -> io::Result<[u64; 2]> {
    let metadata = plain_metadata(path)?;
    if !metadata.is_dir() {
        return Err(io::Error::other("directory required"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok([metadata.dev(), metadata.ino()])
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let file = OpenOptions::new()
            .access_mode(0)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(path)?;
        open_file_identity(&file)
    }
}

pub(crate) fn same_open_file(file: &File, path: &Path) -> bool {
    matches!((open_file_identity(file), open_existing_plain(path, false).and_then(|current| open_file_identity(&current))), (Ok(held), Ok(current)) if held == current)
}

fn grant_directory_identity(path: &Path, persistent: bool) -> io::Result<[u64; 2]> {
    if persistent {
        profile_object_identity(&fs::canonicalize(path)?)
    } else {
        profile_object_identity(path)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Grant {
    version: u32,
    pub(crate) profile: PathBuf,
    root: PathBuf,
    nonce: String,
    root_identity: [u64; 2],
    profile_identity: [u64; 2],
    parent_pid: u32,
    parent_birth: u64,
    pub(crate) persistent: bool,
}

impl Grant {
    pub(crate) fn new(profile: &Path, persistent: bool) -> io::Result<Self> {
        let root = profile
            .parent()
            .ok_or_else(|| io::Error::other("missing profile root"))?;
        if profile.to_str().is_none() || root.to_str().is_none() {
            return Err(io::Error::other("profile path is not valid Unicode"));
        }
        let nonce = if persistent {
            uuid::Uuid::new_v4().to_string()
        } else {
            profile
                .file_name()
                .and_then(|v| v.to_str())
                .ok_or_else(|| io::Error::other("invalid nonce"))?
                .to_owned()
        };
        let parent_pid = std::process::id();
        let grant = Self {
            version: 1,
            profile: profile.to_owned(),
            root: root.to_owned(),
            nonce,
            root_identity: grant_directory_identity(root, persistent)?,
            profile_identity: grant_directory_identity(profile, persistent)?,
            parent_pid,
            parent_birth: process_birth(parent_pid)?,
            persistent,
        };
        grant.validate()?;
        Ok(grant)
    }

    pub(crate) fn validate(&self) -> io::Result<()> {
        if self.version != 1
            || !self.profile.is_absolute()
            || self.profile.parent() != Some(self.root.as_path())
            || uuid::Uuid::parse_str(&self.nonce)
                .ok()
                .map(|v| v.to_string())
                != Some(self.nonce.clone())
            || (!self.persistent
                && self
                    .root
                    .ancestors()
                    .any(|p| !plain_metadata(p).is_ok_and(|m| m.is_dir())))
            || grant_directory_identity(&self.root, self.persistent)? != self.root_identity
            || grant_directory_identity(&self.profile, self.persistent)? != self.profile_identity
            || process_birth(self.parent_pid)? != self.parent_birth
        {
            return Err(io::Error::other("invalid browser grant"));
        }
        if !self.persistent {
            if self.profile.file_name().and_then(|v| v.to_str()) != Some(self.nonce.as_str()) {
                return Err(io::Error::other("invalid browser grant"));
            }
            let path = self.profile.join(".clovy-reserved.json");
            let mut receipt = open_existing_plain(&path, false)?;
            let mut bytes = Vec::new();
            (&mut receipt).take(4097).read_to_end(&mut bytes)?;
            if bytes.len() > 4096 || !same_open_file(&receipt, &path) {
                return Err(io::Error::other("invalid browser grant"));
            }
            let value: Value = serde_json::from_slice(&bytes)?;
            if value["version"] != 1
                || value["phase"] != "Starting"
                || value["nonce"] != self.nonce
                || value["ownerProcessId"] != self.parent_pid
                || value["ownerBirth"] != self.parent_birth
                || value["rootIdentity"] != json!(self.root_identity)
                || value["profileIdentity"] != json!(self.profile_identity)
            {
                return Err(io::Error::other("invalid browser grant"));
            }
            let mut marker = open_existing_plain(&self.profile.join(".clovy-starting"), false)?;
            let mut content = Vec::new();
            (&mut marker).take(32).read_to_end(&mut content)?;
            if content != b"Starting\n"
                || !same_open_file(&marker, &self.profile.join(".clovy-starting"))
            {
                return Err(io::Error::other("invalid browser grant"));
            }
        }
        if grant_directory_identity(&self.root, self.persistent)? != self.root_identity
            || grant_directory_identity(&self.profile, self.persistent)? != self.profile_identity
            || process_birth(self.parent_pid)? != self.parent_birth
        {
            return Err(io::Error::other("invalid browser grant"));
        }
        Ok(())
    }
}
