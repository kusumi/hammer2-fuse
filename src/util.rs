pub(crate) fn get_home_path() -> crate::Result<String> {
    Ok(home::home_dir()
        .ok_or(nix::errno::Errno::ENOENT)?
        .into_os_string()
        .into_string()
        .unwrap())
}

pub(crate) fn stat2attr(st: &libhammer2::hammer2::Stat) -> fuser::FileAttr {
    let mtime = libfs::time::unix2system(st.st_mtime);
    fuser::FileAttr {
        ino: st.st_ino,
        size: st.st_size,
        blocks: st.st_blocks,
        atime: libfs::time::unix2system(st.st_atime),
        mtime,
        ctime: mtime,
        crtime: mtime,
        kind: mode2kind(st.st_mode),
        perm: (st.st_mode & 0o777).try_into().unwrap(),
        nlink: st.st_nlink,
        uid: st.st_uid,
        gid: st.st_gid,
        rdev: st.st_rdev,
        blksize: st.st_blksize,
        flags: 0,
    }
}

pub(crate) fn mode2kind(mode: libhammer2::hammer2::StatMode) -> fuser::FileType {
    match mode & libc::S_IFMT {
        libc::S_IFDIR => fuser::FileType::Directory,
        libc::S_IFREG => fuser::FileType::RegularFile,
        libc::S_IFIFO => fuser::FileType::NamedPipe,
        libc::S_IFCHR => fuser::FileType::CharDevice,
        libc::S_IFBLK => fuser::FileType::BlockDevice,
        libc::S_IFLNK => fuser::FileType::Symlink,
        libc::S_IFSOCK => fuser::FileType::Socket,
        _ => panic!("{mode:o}"),
    }
}

pub(crate) fn obj2kind(typ: u8) -> fuser::FileType {
    match typ {
        libhammer2::fs::HAMMER2_OBJTYPE_DIRECTORY => fuser::FileType::Directory,
        libhammer2::fs::HAMMER2_OBJTYPE_REGFILE => fuser::FileType::RegularFile,
        libhammer2::fs::HAMMER2_OBJTYPE_FIFO => fuser::FileType::NamedPipe,
        libhammer2::fs::HAMMER2_OBJTYPE_CDEV => fuser::FileType::CharDevice,
        libhammer2::fs::HAMMER2_OBJTYPE_BDEV => fuser::FileType::BlockDevice,
        libhammer2::fs::HAMMER2_OBJTYPE_SOFTLINK => fuser::FileType::Symlink,
        libhammer2::fs::HAMMER2_OBJTYPE_SOCKET => fuser::FileType::Socket,
        _ => panic!("{typ}"),
    }
}
