//! Recovery authority anchored to the original browser profile objects.
use std::{fs::File, io, path::Path};

#[cfg(unix)]
mod native {
    use super::*;
    use std::{
        ffi::{CStr, CString, OsStr},
        os::unix::{
            ffi::OsStrExt,
            io::{AsRawFd, FromRawFd, RawFd},
        },
        path::Component,
    };

    fn component(value: &OsStr) -> io::Result<CString> {
        CString::new(value.as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid component"))
    }
    fn open_at(parent: RawFd, name: &CStr, flags: i32) -> io::Result<File> {
        let fd = unsafe {
            libc::openat(
                parent,
                name.as_ptr(),
                flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }
    fn directory_at(parent: RawFd, name: &CStr) -> io::Result<File> {
        open_at(parent, name, libc::O_RDONLY | libc::O_DIRECTORY)
    }
    fn identity(file: &File) -> io::Result<[u64; 2]> {
        use std::os::unix::fs::MetadataExt;
        let meta = file.metadata()?;
        Ok([meta.dev(), meta.ino()])
    }
    fn identity_at(parent: RawFd, name: &CStr) -> io::Result<([u64; 2], libc::mode_t)> {
        let mut info: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstatat(parent, name.as_ptr(), &mut info, libc::AT_SYMLINK_NOFOLLOW) }
            != 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(([info.st_dev as u64, info.st_ino as u64], info.st_mode))
    }
    fn list(directory: &File) -> io::Result<Vec<CString>> {
        let dup = unsafe { libc::dup(directory.as_raw_fd()) };
        if dup < 0 {
            return Err(io::Error::last_os_error());
        }
        let stream = unsafe { libc::fdopendir(dup) };
        if stream.is_null() {
            unsafe {
                libc::close(dup);
            }
            return Err(io::Error::last_os_error());
        }
        unsafe {
            libc::rewinddir(stream);
        }
        let mut names = Vec::new();
        loop {
            // A failed or partial enumeration must never authorize deletion.
            #[cfg(target_os = "linux")]
            unsafe {
                *libc::__errno_location() = 0;
            }
            #[cfg(target_os = "macos")]
            unsafe {
                *libc::__error() = 0;
            }
            let entry = unsafe { libc::readdir(stream) };
            if entry.is_null() {
                let error = io::Error::last_os_error();
                unsafe {
                    libc::closedir(stream);
                }
                if error.raw_os_error().unwrap_or(0) != 0 {
                    return Err(error);
                }
                return Ok(names);
            }
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
            if name.to_bytes() != b"." && name.to_bytes() != b".." {
                names.push(name.to_owned());
            }
        }
    }
    fn empty(directory: &File) -> io::Result<()> {
        let parent = directory.as_raw_fd();
        for name in list(directory)? {
            let (before, mode) = identity_at(parent, &name)?;
            let is_directory = mode & libc::S_IFMT == libc::S_IFDIR;
            if is_directory {
                let child = directory_at(parent, &name)?;
                if identity(&child)? != before {
                    return Err(io::Error::other("directory changed"));
                }
                empty(&child)?;
                if identity_at(parent, &name)?.0 != before {
                    return Err(io::Error::other("directory changed"));
                }
            }
            let flags = if is_directory { libc::AT_REMOVEDIR } else { 0 };
            if unsafe { libc::unlinkat(parent, name.as_ptr(), flags) } != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    pub(crate) struct RecoveryRoot {
        directory: File,
        id: [u64; 2],
    }
    pub(crate) struct RecoveryProfile {
        root: File,
        holder: Option<File>,
        directory: File,
        root_id: [u64; 2],
        id: [u64; 2],
        name: CString,
        quarantine: Option<CString>,
    }
    impl RecoveryRoot {
        pub(crate) fn open(path: &Path) -> io::Result<Self> {
            Self::open_inner(path, false)
        }
        pub(crate) fn open_or_create(path: &Path) -> io::Result<Self> {
            Self::open_inner(path, true)
        }
        fn open_inner(path: &Path, create: bool) -> io::Result<Self> {
            if !path.is_absolute() {
                return Err(io::Error::other("absolute root required"));
            }
            let mut directory = File::open("/")?;
            for part in path.components() {
                match part {
                    Component::RootDir => {}
                    Component::Normal(name) => {
                        let name = component(name)?;
                        directory = match directory_at(directory.as_raw_fd(), &name) {
                            Ok(next) => next,
                            Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
                                if unsafe {
                                    libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700)
                                } != 0
                                {
                                    let error = io::Error::last_os_error();
                                    if error.kind() != io::ErrorKind::AlreadyExists {
                                        return Err(error);
                                    }
                                }
                                directory.sync_all()?;
                                directory_at(directory.as_raw_fd(), &name)?
                            }
                            Err(error) => return Err(error),
                        };
                    }
                    _ => return Err(io::Error::other("invalid root")),
                }
            }
            let id = identity(&directory)?;
            Ok(Self { directory, id })
        }
        pub(crate) fn names(&self) -> io::Result<Vec<std::ffi::OsString>> {
            use std::os::unix::ffi::OsStringExt;
            Ok(list(&self.directory)?
                .into_iter()
                .map(|s| std::ffi::OsString::from_vec(s.into_bytes()))
                .collect())
        }
        pub(crate) fn profile(&self, name: &OsStr) -> io::Result<RecoveryProfile> {
            let name = component(name)?;
            let directory = directory_at(self.directory.as_raw_fd(), &name)?;
            let id = identity(&directory)?;
            Ok(RecoveryProfile {
                root: self.directory.try_clone()?,
                holder: None,
                directory,
                root_id: self.id,
                id,
                name,
                quarantine: None,
            })
        }
        pub(crate) fn create_profile(&self, name: &OsStr) -> io::Result<RecoveryProfile> {
            let part = component(name)?;
            if unsafe { libc::mkdirat(self.directory.as_raw_fd(), part.as_ptr(), 0o700) } != 0 {
                return Err(io::Error::last_os_error());
            }
            self.directory.sync_all()?;
            self.profile(name)
        }
        pub(crate) fn quarantined(&self, name: &OsStr) -> io::Result<RecoveryProfile> {
            let quarantine = component(name)?;
            let holder = directory_at(self.directory.as_raw_fd(), &quarantine)?;
            let directory = directory_at(holder.as_raw_fd(), c"profile")?;
            let id = identity(&directory)?;
            Ok(RecoveryProfile {
                root: self.directory.try_clone()?,
                holder: Some(holder),
                directory,
                root_id: self.id,
                id,
                name: CString::new("profile").unwrap(),
                quarantine: Some(quarantine),
            })
        }
    }
    impl RecoveryProfile {
        pub(crate) fn root_identity(&self) -> [u64; 2] {
            self.root_id
        }
        pub(crate) fn identity(&self) -> [u64; 2] {
            self.id
        }
        pub(crate) fn open_file(&self, name: &str, write: bool) -> io::Result<File> {
            let file = open_at(
                self.directory.as_raw_fd(),
                &CString::new(name).map_err(io::Error::other)?,
                if write { libc::O_RDWR } else { libc::O_RDONLY },
            )?;
            if !file.metadata()?.is_file() {
                return Err(io::Error::other("regular file required"));
            }
            Ok(file)
        }
        pub(crate) fn create_file(&self, name: &str) -> io::Result<File> {
            let name = CString::new(name).map_err(io::Error::other)?;
            let fd = unsafe {
                libc::openat(
                    self.directory.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDWR
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC,
                    0o600,
                )
            };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(unsafe { File::from_raw_fd(fd) })
        }
        pub(crate) fn same_file(&self, name: &str, file: &File) -> bool {
            self.open_file(name, false)
                .and_then(|current| Ok(identity(&current)? == identity(file)?))
                .unwrap_or(false)
        }
        pub(crate) fn sync(&self) -> io::Result<()> {
            self.directory.sync_all()
        }
        pub(crate) fn replace_receipt(&self, bytes: &[u8]) -> io::Result<()> {
            use std::io::Write;
            let name = CString::new(format!(".clovy-cancel-{}.tmp", uuid::Uuid::new_v4()))
                .map_err(io::Error::other)?;
            let fd = unsafe {
                libc::openat(
                    self.directory.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC,
                    0o600,
                )
            };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            let mut file = unsafe { File::from_raw_fd(fd) };
            file.write_all(bytes)?;
            file.sync_all()?;
            if unsafe {
                libc::renameat(
                    self.directory.as_raw_fd(),
                    name.as_ptr(),
                    self.directory.as_raw_fd(),
                    c".clovy-reserved.json".as_ptr(),
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
            self.directory.sync_all()
        }
        pub(crate) fn delete(self) -> io::Result<()> {
            let (holder, quarantine) = match (self.holder, self.quarantine) {
                (Some(holder), Some(quarantine)) => (holder, quarantine),
                (None, None) => {
                    let quarantine =
                        CString::new(format!(".clovy-gc-{}", self.name.to_string_lossy()))
                            .map_err(io::Error::other)?;
                    if unsafe { libc::mkdirat(self.root.as_raw_fd(), quarantine.as_ptr(), 0o700) }
                        != 0
                    {
                        return Err(io::Error::last_os_error());
                    }
                    let holder = directory_at(self.root.as_raw_fd(), &quarantine)?;
                    if unsafe {
                        libc::renameat(
                            self.root.as_raw_fd(),
                            self.name.as_ptr(),
                            holder.as_raw_fd(),
                            c"profile".as_ptr(),
                        )
                    } != 0
                    {
                        return Err(io::Error::last_os_error());
                    }
                    (holder, quarantine)
                }
                _ => return Err(io::Error::other("invalid quarantine")),
            };
            let moved = directory_at(holder.as_raw_fd(), c"profile")?;
            if identity(&moved)? != self.id {
                return Err(io::Error::other("profile changed during quarantine"));
            }
            empty(&moved)?;
            if identity_at(holder.as_raw_fd(), c"profile")?.0 != self.id {
                return Err(io::Error::other("profile changed"));
            }
            if unsafe {
                libc::unlinkat(holder.as_raw_fd(), c"profile".as_ptr(), libc::AT_REMOVEDIR)
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
            if unsafe {
                libc::unlinkat(
                    self.root.as_raw_fd(),
                    quarantine.as_ptr(),
                    libc::AT_REMOVEDIR,
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
            self.root.sync_all()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn profile_swap_does_not_delete_replacement_or_follow_symlink() {
            let root = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            std::fs::write(outside.path().join("sentinel"), b"keep").unwrap();
            let name = uuid::Uuid::new_v4().to_string();
            let profile = root.path().join(&name);
            std::fs::create_dir(&profile).unwrap();
            std::fs::write(profile.join("owned"), b"owned").unwrap();
            let root_handle = RecoveryRoot::open(root.path()).unwrap();
            let capability = root_handle.profile(OsStr::new(&name)).unwrap();
            std::fs::rename(&profile, root.path().join("original")).unwrap();
            std::os::unix::fs::symlink(outside.path(), &profile).unwrap();
            assert!(capability.delete().is_err());
            assert_eq!(
                std::fs::read(outside.path().join("sentinel")).unwrap(),
                b"keep"
            );
            assert_eq!(
                std::fs::read(root.path().join("original/owned")).unwrap(),
                b"owned"
            );
        }
        #[test]
        fn owned_tree_deletion_does_not_follow_its_symlinks() {
            let root = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            std::fs::write(outside.path().join("sentinel"), b"keep").unwrap();
            let name = uuid::Uuid::new_v4().to_string();
            let profile = root.path().join(&name);
            std::fs::create_dir_all(profile.join("nested")).unwrap();
            std::fs::write(profile.join("nested/item"), b"fixture").unwrap();
            std::os::unix::fs::symlink(outside.path(), profile.join("link")).unwrap();
            let root_handle = RecoveryRoot::open(root.path()).unwrap();
            root_handle
                .profile(OsStr::new(&name))
                .unwrap()
                .delete()
                .unwrap();
            assert!(!profile.exists());
            assert_eq!(
                std::fs::read(outside.path().join("sentinel")).unwrap(),
                b"keep"
            );
        }
    }
}

#[cfg(unix)]
pub(crate) use native::{RecoveryProfile, RecoveryRoot};

#[cfg(windows)]
mod native_windows {
    use super::*;
    use std::{
        ffi::{c_void, OsStr, OsString},
        io::Write,
        os::windows::{
            ffi::{OsStrExt, OsStringExt},
            fs::OpenOptionsExt,
            io::{AsRawHandle, FromRawHandle},
        },
        path::{Component, PathBuf},
        ptr,
    };
    #[repr(C)]
    struct UnicodeString {
        length: u16,
        maximum_length: u16,
        buffer: *mut u16,
    }
    #[repr(C)]
    struct ObjectAttributes {
        length: u32,
        root: *mut c_void,
        name: *mut UnicodeString,
        attributes: u32,
        security: *mut c_void,
        quality: *mut c_void,
    }
    #[repr(C)]
    struct IoStatus {
        status: usize,
        information: usize,
    }
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
    #[repr(C)]
    struct DirectoryInfo {
        next: u32,
        index: u32,
        creation: i64,
        access: i64,
        write: i64,
        change: i64,
        size: i64,
        allocation: i64,
        attributes: u32,
        name_len: u32,
        ea_size: u32,
        short_len: u8,
        short_name: [u16; 12],
        file_id: i64,
        file_name: [u16; 1],
    }
    #[repr(C)]
    struct RenameInfo {
        replace: u32,
        root: *mut c_void,
        name_len: u32,
        name: [u16; 1],
    }
    #[link(name = "ntdll")]
    extern "system" {
        fn NtCreateFile(
            handle: *mut *mut c_void,
            access: u32,
            attributes: *mut ObjectAttributes,
            status: *mut IoStatus,
            allocation: *const i64,
            file_attributes: u32,
            share: u32,
            disposition: u32,
            options: u32,
            ea: *mut c_void,
            ea_length: u32,
        ) -> i32;
        fn RtlNtStatusToDosError(status: i32) -> u32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileInformationByHandle(handle: *mut c_void, info: *mut FileInfo) -> i32;
        fn GetFileInformationByHandleEx(
            handle: *mut c_void,
            class: i32,
            info: *mut c_void,
            size: u32,
        ) -> i32;
        fn SetFileInformationByHandle(
            handle: *mut c_void,
            class: i32,
            info: *const c_void,
            size: u32,
        ) -> i32;
    }
    fn info(file: &File) -> io::Result<FileInfo> {
        let mut data = FileInfo::default();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut data) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(data)
    }
    fn identity(file: &File) -> io::Result<[u64; 2]> {
        let data = info(file)?;
        Ok([
            data.volume as u64,
            (u64::from(data.index_high) << 32) | u64::from(data.index_low),
        ])
    }
    fn open_relative(
        parent: &File,
        name: &OsStr,
        directory: bool,
        create: bool,
        access: u32,
        allow_reparse: bool,
    ) -> io::Result<File> {
        let mut wide: Vec<u16> = name.encode_wide().collect();
        if wide.is_empty() || wide.contains(&0) {
            return Err(io::Error::other("invalid component"));
        }
        let length = u16::try_from(wide.len() * 2).map_err(io::Error::other)?;
        let mut name = UnicodeString {
            length,
            maximum_length: length,
            buffer: wide.as_mut_ptr(),
        };
        let mut attrs = ObjectAttributes {
            length: std::mem::size_of::<ObjectAttributes>() as u32,
            root: parent.as_raw_handle(),
            name: &mut name,
            attributes: 0x40,
            security: ptr::null_mut(),
            quality: ptr::null_mut(),
        };
        let mut status = IoStatus {
            status: 0,
            information: 0,
        };
        let mut handle = ptr::null_mut();
        let options = 0x00200020
            | if create { 2 } else { 0 }
            | if directory {
                1
            } else if allow_reparse {
                0
            } else {
                0x40
            };
        let disposition = if create { 2 } else { 1 };
        let code = unsafe {
            NtCreateFile(
                &mut handle,
                access,
                &mut attrs,
                &mut status,
                ptr::null(),
                0,
                7,
                disposition,
                options,
                ptr::null_mut(),
                0,
            )
        };
        if code < 0 {
            return Err(io::Error::from_raw_os_error(
                unsafe { RtlNtStatusToDosError(code) } as i32,
            ));
        }
        let file = unsafe { File::from_raw_handle(handle) };
        let attrs = info(&file)?.attributes;
        if (!allow_reparse && attrs & 0x400 != 0)
            || (directory && attrs & 0x10 == 0)
            || (!directory && !allow_reparse && attrs & 0x10 != 0)
        {
            return Err(io::Error::other("unexpected reparse or object type"));
        }
        Ok(file)
    }
    fn rename(file: &File, parent: &File, name: &OsStr, replace: bool) -> io::Result<()> {
        let name: Vec<u16> = name.encode_wide().collect();
        let offset = std::mem::offset_of!(RenameInfo, name);
        let size = offset + name.len() * 2;
        let mut storage = vec![0u64; size.div_ceil(8)];
        let data = storage.as_mut_ptr().cast::<RenameInfo>();
        unsafe {
            (*data).replace = u32::from(replace);
            (*data).root = parent.as_raw_handle();
            (*data).name_len = (name.len() * 2) as u32;
            ptr::copy_nonoverlapping(
                name.as_ptr(),
                ptr::addr_of_mut!((*data).name).cast::<u16>(),
                name.len(),
            );
            if SetFileInformationByHandle(file.as_raw_handle(), 3, data.cast(), size as u32) == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }
    fn remove(file: &File) -> io::Result<()> {
        let delete: u8 = 1;
        if unsafe {
            SetFileInformationByHandle(file.as_raw_handle(), 4, ptr::from_ref(&delete).cast(), 1)
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    fn list(directory: &File) -> io::Result<Vec<OsString>> {
        let mut names = Vec::new();
        let mut first = true;
        loop {
            let mut storage = vec![0u64; 8192];
            let result = unsafe {
                GetFileInformationByHandleEx(
                    directory.as_raw_handle(),
                    if first { 11 } else { 10 },
                    storage.as_mut_ptr().cast(),
                    (storage.len() * 8) as u32,
                )
            };
            first = false;
            if result == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(18) {
                    return Ok(names);
                }
                return Err(error);
            }
            let size = storage.len() * 8;
            let mut offset = 0usize;
            loop {
                let fixed = std::mem::offset_of!(DirectoryInfo, file_name);
                if offset + fixed > size {
                    return Err(io::Error::other("invalid directory record"));
                }
                let data = unsafe {
                    &*storage
                        .as_ptr()
                        .cast::<u8>()
                        .add(offset)
                        .cast::<DirectoryInfo>()
                };
                let count = data.name_len as usize;
                if count % 2 != 0 || offset + fixed + count > size {
                    return Err(io::Error::other("invalid directory name"));
                }
                let name = unsafe {
                    std::slice::from_raw_parts(
                        ptr::addr_of!(data.file_name).cast::<u16>(),
                        count / 2,
                    )
                };
                if name != [46] && name != [46, 46] {
                    names.push(OsString::from_wide(name));
                }
                if data.next == 0 {
                    break;
                }
                let next = data.next as usize;
                if next < fixed || offset + next >= size {
                    return Err(io::Error::other("invalid directory offset"));
                }
                offset += next;
            }
        }
    }
    fn empty(directory: &File) -> io::Result<()> {
        for name in list(directory)? {
            // Open the entry itself, never following a junction or symlink.
            let child = open_relative(directory, &name, false, false, 0x00110081, true)?;
            let attrs = info(&child)?.attributes;
            if attrs & 0x10 != 0 && attrs & 0x400 == 0 {
                let nested = open_relative(directory, &name, true, false, 0x001100a1, false)?;
                if identity(&nested)? != identity(&child)? {
                    return Err(io::Error::other("directory changed"));
                }
                empty(&nested)?;
                remove(&nested)?;
            } else {
                remove(&child)?;
            }
        }
        Ok(())
    }
    pub(crate) struct RecoveryRoot {
        directory: File,
        id: [u64; 2],
    }
    pub(crate) struct RecoveryProfile {
        root: File,
        holder: Option<File>,
        directory: File,
        root_id: [u64; 2],
        id: [u64; 2],
        name: OsString,
        quarantine: Option<OsString>,
    }
    impl RecoveryRoot {
        pub(crate) fn open(path: &Path) -> io::Result<Self> {
            Self::open_inner(path, false)
        }
        pub(crate) fn open_or_create(path: &Path) -> io::Result<Self> {
            Self::open_inner(path, true)
        }
        fn open_inner(path: &Path, create: bool) -> io::Result<Self> {
            if !path.is_absolute() {
                return Err(io::Error::other("absolute root required"));
            }
            let mut parts = path.components();
            let Some(Component::Prefix(prefix)) = parts.next() else {
                return Err(io::Error::other("invalid root"));
            };
            if !matches!(parts.next(), Some(Component::RootDir)) {
                return Err(io::Error::other("invalid root"));
            }
            let mut base = PathBuf::from(prefix.as_os_str());
            base.push("\\");
            let mut directory = std::fs::OpenOptions::new()
                .access_mode(if create { 0x001000a5 } else { 0x001000a1 })
                .share_mode(7)
                .custom_flags(0x02000000 | 0x00200000)
                .open(base)?;
            if info(&directory)?.attributes & 0x400 != 0 {
                return Err(io::Error::other("reparse root"));
            }
            let mut parts = parts.peekable();
            while let Some(part) = parts.next() {
                match part {
                    Component::Normal(name) => {
                        let access = if parts.peek().is_none() {
                            0x001000a7
                        } else if create {
                            0x001000a5
                        } else {
                            0x001000a1
                        };
                        directory =
                            match open_relative(&directory, name, true, false, access, false) {
                                Ok(next) => next,
                                Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
                                    open_relative(&directory, name, true, true, access, false)?
                                }
                                Err(error) => return Err(error),
                            };
                    }
                    _ => return Err(io::Error::other("invalid root")),
                }
            }
            let id = identity(&directory)?;
            Ok(Self { directory, id })
        }
        pub(crate) fn names(&self) -> io::Result<Vec<OsString>> {
            list(&self.directory)
        }
        pub(crate) fn profile(&self, name: &OsStr) -> io::Result<RecoveryProfile> {
            let directory = open_relative(&self.directory, name, true, false, 0x001100a3, false)?;
            let id = identity(&directory)?;
            Ok(RecoveryProfile {
                root: self.directory.try_clone()?,
                holder: None,
                directory,
                root_id: self.id,
                id,
                name: name.to_owned(),
                quarantine: None,
            })
        }
        pub(crate) fn create_profile(&self, name: &OsStr) -> io::Result<RecoveryProfile> {
            let directory = open_relative(&self.directory, name, true, true, 0x001100a3, false)?;
            let id = identity(&directory)?;
            Ok(RecoveryProfile {
                root: self.directory.try_clone()?,
                holder: None,
                directory,
                root_id: self.id,
                id,
                name: name.to_owned(),
                quarantine: None,
            })
        }
        pub(crate) fn quarantined(&self, name: &OsStr) -> io::Result<RecoveryProfile> {
            let holder = open_relative(&self.directory, name, true, false, 0x001100a7, false)?;
            let directory = open_relative(
                &holder,
                OsStr::new("profile"),
                true,
                false,
                0x001100a3,
                false,
            )?;
            let id = identity(&directory)?;
            Ok(RecoveryProfile {
                root: self.directory.try_clone()?,
                holder: Some(holder),
                directory,
                root_id: self.id,
                id,
                name: OsString::from("profile"),
                quarantine: Some(name.to_owned()),
            })
        }
    }
    impl RecoveryProfile {
        pub(crate) fn root_identity(&self) -> [u64; 2] {
            self.root_id
        }
        pub(crate) fn identity(&self) -> [u64; 2] {
            self.id
        }
        pub(crate) fn open_file(&self, name: &str, write: bool) -> io::Result<File> {
            open_relative(
                &self.directory,
                OsStr::new(name),
                false,
                false,
                0x00100081 | if write { 2 } else { 0 },
                false,
            )
        }
        pub(crate) fn create_file(&self, name: &str) -> io::Result<File> {
            open_relative(
                &self.directory,
                OsStr::new(name),
                false,
                true,
                0x00110083,
                false,
            )
        }
        pub(crate) fn same_file(&self, name: &str, file: &File) -> bool {
            self.open_file(name, false)
                .and_then(|current| Ok(identity(&current)? == identity(file)?))
                .unwrap_or(false)
        }
        pub(crate) fn sync(&self) -> io::Result<()> {
            self.directory.sync_all()
        }
        pub(crate) fn replace_receipt(&self, bytes: &[u8]) -> io::Result<()> {
            let name = format!(".clovy-cancel-{}.tmp", uuid::Uuid::new_v4());
            let mut file = open_relative(
                &self.directory,
                OsStr::new(&name),
                false,
                true,
                0x00110082,
                false,
            )?;
            file.write_all(bytes)?;
            file.sync_all()?;
            rename(
                &file,
                &self.directory,
                OsStr::new(".clovy-reserved.json"),
                true,
            )?;
            self.directory.sync_all()
        }
        pub(crate) fn delete(self) -> io::Result<()> {
            let (holder, _quarantine) = match (self.holder, self.quarantine) {
                (Some(holder), Some(quarantine)) => (holder, quarantine),
                (None, None) => {
                    let name = OsString::from(format!(".clovy-gc-{}", self.name.to_string_lossy()));
                    let holder = open_relative(&self.root, &name, true, true, 0x001100a7, false)?;
                    rename(&self.directory, &holder, OsStr::new("profile"), false)?;
                    (holder, name)
                }
                _ => return Err(io::Error::other("invalid quarantine")),
            };
            let moved = open_relative(
                &holder,
                OsStr::new("profile"),
                true,
                false,
                0x001100a3,
                false,
            )?;
            if identity(&moved)? != self.id {
                return Err(io::Error::other("profile changed"));
            }
            empty(&moved)?;
            remove(&moved)?;
            drop(moved);
            drop(self.directory);
            remove(&holder)
        }
    }
}
#[cfg(windows)]
pub(crate) use native_windows::{RecoveryProfile, RecoveryRoot};
