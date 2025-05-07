#[allow(unused_macros)]
macro_rules! get_inode {
    ($ef:expr, $inum:expr) => {
        $ef.get_inode($inum).unwrap()
    };
}

macro_rules! get_inode_mut {
    ($ef:expr, $inum:expr) => {
        $ef.get_inode_mut($inum).unwrap()
    };
}

macro_rules! debug_req {
    ($req:expr, $cond:expr) => {
        if $cond {
            log::debug!("{:?}", $req);
        }
    };
}

static MTX: std::sync::LazyLock<std::sync::Mutex<i32>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(0));

macro_rules! mtx_lock {
    ($mtx:expr) => {
        $mtx.lock().unwrap()
    };
}

const TTL: std::time::Duration = std::time::Duration::from_secs(1);

fn stat2attr(st: &libhammer2::hammer2::Stat) -> fuser::FileAttr {
    let attr = crate::util::stat2attr(st);
    log::debug!("{attr:?}");
    attr
}

#[allow(clippy::needless_pass_by_value)]
fn h2i(e: libhammer2::Error) -> i32 {
    (match e {
        libhammer2::Error::Errno(e) => e,
        libhammer2::Error::Error(e) => match libfs::os::error2errno(&e) {
            Some(v) => v,
            None => nix::errno::Errno::EINVAL,
        },
    }) as i32
}

impl fuser::Filesystem for crate::Hammer2Fuse {
    fn init(
        &mut self,
        req: &fuser::Request<'_>,
        config: &mut fuser::KernelConfig,
    ) -> Result<(), libc::c_int> {
        debug_req!(req, self.debug > 1);
        log::debug!("config {config:?}");
        let _mtx = mtx_lock!(MTX);
        Ok(())
    }

    fn destroy(&mut self) {
        log::debug!("destroy");
        let _mtx = mtx_lock!(MTX);
        assert_eq!(self.total_open, 0);
        self.pmp.unmount().unwrap();
    }

    fn lookup(
        &mut self,
        req: &fuser::Request<'_>,
        dinum: u64,
        name: &std::ffi::OsStr,
        reply: fuser::ReplyEntry,
    ) {
        debug_req!(req, self.debug > 1);
        log::debug!("dinum {dinum} name {name:?}");
        let _mtx = mtx_lock!(MTX);
        let Some(name) = name.to_str() else {
            reply.error(libc::EINVAL);
            return;
        };
        let inum = match self.pmp.nresolve(dinum, name) {
            Ok(v) => v,
            Err(e) => {
                reply.error(h2i(e));
                return;
            }
        };
        match self.pmp.stat(inum) {
            Ok(v) => reply.entry(&TTL, &stat2attr(&v), 0),
            Err(e) => reply.error(h2i(e)),
        }
    }

    fn getattr(
        &mut self,
        req: &fuser::Request<'_>,
        inum: u64,
        fh: Option<u64>,
        reply: fuser::ReplyAttr,
    ) {
        debug_req!(req, self.debug > 1);
        log::debug!("inum {inum}");
        let _mtx = mtx_lock!(MTX);
        if let Some(fh) = fh {
            assert_eq!(inum, fh);
        }
        match self.pmp.stat(inum) {
            Ok(v) => reply.attr(&TTL, &stat2attr(&v)),
            Err(e) => reply.error(h2i(e)),
        }
    }

    fn open(&mut self, req: &fuser::Request<'_>, inum: u64, flags: i32, reply: fuser::ReplyOpen) {
        debug_req!(req, self.debug > 1);
        log::debug!("inum {inum} flags {flags:#x}");
        let _mtx = mtx_lock!(MTX);
        let Some(ip) = self.pmp.get_inode(inum) else {
            reply.error(libc::ENOENT);
            return;
        };
        assert_eq!(ip.get_meta().inum, inum);
        get_inode_mut!(self.pmp, inum).get(); // put on release
        self.total_open += 1;
        reply.opened(inum, fuser::consts::FOPEN_KEEP_CACHE);
    }

    fn readlink(&mut self, req: &fuser::Request<'_>, inum: u64, reply: fuser::ReplyData) {
        debug_req!(req, self.debug > 1);
        log::debug!("inum {inum}");
        let _mtx = mtx_lock!(MTX);
        match self.pmp.readlinkx(inum) {
            Ok(v) => reply.data(v.as_bytes()),
            Err(e) => reply.error(h2i(e)),
        }
    }

    fn read(
        &mut self,
        req: &fuser::Request<'_>,
        inum: u64,
        fh: u64,
        offset: i64,
        size: u32,
        flags: i32,
        lock_owner: Option<u64>,
        reply: fuser::ReplyData,
    ) {
        debug_req!(req, self.debug > 1);
        log::debug!(
            "inum {inum} fh {fh} offset {offset} size {size} flags {flags:#x} \
            lock_owner {lock_owner:?}"
        );
        let _mtx = mtx_lock!(MTX);
        assert_eq!(inum, fh);
        match self
            .pmp
            .preadx(inum, size.into(), offset.try_into().unwrap())
        {
            Ok(v) => reply.data(&v),
            Err(e) => reply.error(h2i(e)),
        }
    }

    fn flush(
        &mut self,
        req: &fuser::Request<'_>,
        inum: u64,
        fh: u64,
        lock_owner: u64,
        reply: fuser::ReplyEmpty,
    ) {
        debug_req!(req, self.debug > 1);
        log::debug!("inum {inum} fh {fh} lock_owner {lock_owner:?}");
        let _mtx = mtx_lock!(MTX);
        assert_eq!(inum, fh);
        reply.ok();
    }

    fn release(
        &mut self,
        req: &fuser::Request<'_>,
        inum: u64,
        fh: u64,
        flags: i32,
        lock_owner: Option<u64>,
        flush: bool,
        reply: fuser::ReplyEmpty,
    ) {
        debug_req!(req, self.debug > 1);
        log::debug!(
            "inum {inum} fh {fh} flags {flags:#x} flush {flush} \
            lock_owner {lock_owner:?}"
        );
        let _mtx = mtx_lock!(MTX);
        assert_eq!(inum, fh);
        assert!(self.total_open > 0);
        self.total_open -= 1;
        get_inode_mut!(self.pmp, inum).put();
        reply.ok();
    }

    fn opendir(
        &mut self,
        req: &fuser::Request<'_>,
        inum: u64,
        flags: i32,
        reply: fuser::ReplyOpen,
    ) {
        debug_req!(req, self.debug > 1);
        log::debug!("inum {inum} flags {flags:#x}");
        let _mtx = mtx_lock!(MTX);
        let Some(ip) = self.pmp.get_inode(inum) else {
            reply.error(libc::ENOENT);
            return;
        };
        assert_eq!(ip.get_meta().inum, inum);
        get_inode_mut!(self.pmp, inum).get(); // put on releasedir
        self.total_open += 1;
        reply.opened(inum, fuser::consts::FOPEN_KEEP_CACHE);
    }

    fn readdir(
        &mut self,
        req: &fuser::Request<'_>,
        dinum: u64,
        fh: u64,
        offset: i64,
        mut reply: fuser::ReplyDirectory,
    ) {
        debug_req!(req, self.debug > 1);
        log::debug!("dinum {dinum} fh {fh} offset {offset}");
        let _mtx = mtx_lock!(MTX);
        assert_eq!(dinum, fh);
        let Some(dip) = self.pmp.get_inode(dinum) else {
            reply.error(libc::ENOENT);
            return;
        };
        if !dip.is_directory() {
            reply.error(libc::ENOTDIR);
            return;
        }
        match self.pmp.readdir(dinum) {
            Ok(v) => {
                assert_eq!(v[0].name, ".", "{v:?}");
                assert_eq!(v[1].name, "..", "{v:?}");
                log::debug!("{v:?}");
                if offset >= v.len().try_into().unwrap() {
                    reply.ok();
                    return;
                }
                for (i, e) in v[offset.try_into().unwrap()..].iter().enumerate() {
                    if reply.add(
                        e.inum,
                        offset + i64::try_from(i + 1).unwrap(),
                        crate::util::obj2kind(e.typ),
                        e.name.clone(),
                    ) {
                        break;
                    }
                }
                reply.ok();
            }
            Err(e) => reply.error(h2i(e)),
        }
    }

    fn releasedir(
        &mut self,
        req: &fuser::Request<'_>,
        inum: u64,
        fh: u64,
        flags: i32,
        reply: fuser::ReplyEmpty,
    ) {
        debug_req!(req, self.debug > 1);
        log::debug!("inum {inum} fh {fh} flags {flags:#x}");
        let _mtx = mtx_lock!(MTX);
        assert_eq!(inum, fh);
        assert!(self.total_open > 0);
        self.total_open -= 1;
        get_inode_mut!(self.pmp, inum).put();
        reply.ok();
    }

    fn statfs(&mut self, req: &fuser::Request<'_>, inum: u64, reply: fuser::ReplyStatfs) {
        debug_req!(req, self.debug > 1);
        log::debug!("inum {inum}");
        let _mtx = mtx_lock!(MTX);
        match self.pmp.statfs() {
            Ok(v) => reply.statfs(
                v.f_blocks,
                v.f_bfree,
                v.f_bavail,
                v.f_files,
                v.f_ffree,
                v.f_bsize,
                v.f_namelen,
                v.f_frsize,
            ),
            Err(e) => reply.error(h2i(e)),
        }
    }

    // https://docs.rs/fuser/latest/fuser/trait.Filesystem.html
    // If the default_permissions mount option is given, this method is not called.
    fn access(&mut self, req: &fuser::Request<'_>, inum: u64, mask: i32, reply: fuser::ReplyEmpty) {
        debug_req!(req, self.debug > 1);
        log::debug!("inum {inum} mask {mask:#o}");
        let _mtx = mtx_lock!(MTX);
        reply.ok();
        panic!("access");
    }

    // Not supported on FreeBSD (see fuse_vnop_ioctl()).
    fn ioctl(
        &mut self,
        req: &fuser::Request<'_>,
        inum: u64,
        fh: u64,
        flags: u32,
        cmd: u32,
        in_data: &[u8],
        out_size: u32,
        reply: fuser::ReplyIoctl,
    ) {
        debug_req!(req, self.debug > 1);
        log::debug!(
            "inum {inum} fh {fh} flags {flags:#x} cmd {cmd:#x} in_data {in_data:?} \
            out_size {out_size}"
        );
        let _mtx = mtx_lock!(MTX);
        assert_eq!(inum, fh);
        match u64::from(cmd) {
            libhammer2::ioctl::CMD_VERSION_GET => reply.ioctl(
                0,
                libfs::cast::as_u8_slice(&self.ioctl_version_get(libfs::cast::align_to(in_data))),
            ),
            libhammer2::ioctl::CMD_PFS_GET => {
                match self.ioctl_pfs_get(libfs::cast::align_to(in_data)) {
                    Ok(v) => reply.ioctl(0, libfs::cast::as_u8_slice(&v)),
                    Err(e) => reply.error(h2i(e)),
                }
            }
            libhammer2::ioctl::CMD_PFS_LOOKUP => {
                match self.ioctl_pfs_lookup(libfs::cast::align_to(in_data)) {
                    Ok(v) => reply.ioctl(0, libfs::cast::as_u8_slice(&v)),
                    Err(e) => reply.error(h2i(e)),
                }
            }
            libhammer2::ioctl::CMD_INODE_GET => {
                match self.ioctl_inode_get(inum, libfs::cast::align_to(in_data)) {
                    Ok(v) => reply.ioctl(0, libfs::cast::as_u8_slice(&v)),
                    Err(e) => reply.error(e as i32),
                }
            }
            libhammer2::ioctl::CMD_DEBUG_DUMP => match self.ioctl_debug_dump(inum) {
                Ok(()) => reply.ioctl(0, &[]),
                Err(e) => reply.error(h2i(e)),
            },
            libhammer2::ioctl::CMD_VOLUME_LIST => {
                match self.ioctl_volume_list(libfs::cast::align_to(in_data)) {
                    Ok(v) => reply.ioctl(0, libfs::cast::as_u8_slice(&v)),
                    Err(e) => reply.error(e as i32),
                }
            }
            libhammer2::ioctl::CMD_VOLUME_LIST2 => {
                match self.ioctl_volume_list2(libfs::cast::align_to(in_data)) {
                    Ok(v) => reply.ioctl(0, libfs::cast::as_u8_slice(&v)),
                    Err(e) => reply.error(e as i32),
                }
            }
            libhammer2::ioctl::CMD_CIDPRUNE => {
                match self.ioctl_cidprune(libfs::cast::align_to(in_data)) {
                    Ok(v) => reply.ioctl(0, libfs::cast::as_u8_slice(&v)),
                    Err(e) => reply.error(h2i(e)),
                }
            }
            libhammer2::ioctl::CMD_PFS_CREATE
            | libhammer2::ioctl::CMD_PFS_DELETE
            | libhammer2::ioctl::CMD_PFS_SNAPSHOT
            | libhammer2::ioctl::CMD_INODE_SET
            | libhammer2::ioctl::CMD_BULKFREE_SCAN
            | libhammer2::ioctl::CMD_DESTROY
            | libhammer2::ioctl::CMD_EMERG_MODE
            | libhammer2::ioctl::CMD_GROWFS => reply.error(libc::EOPNOTSUPP),
            _ => {
                log::error!("invalid ioctl command {cmd:#x}");
                reply.error(libc::EINVAL);
            }
        }
    }
}
