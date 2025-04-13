impl crate::Hammer2Fuse {
    pub(crate) fn ioctl_version_get(
        &self,
        ioc: &libhammer2::ioctl::IocVersion,
    ) -> libhammer2::ioctl::IocVersion {
        let mut ioc = *ioc;
        ioc.version = self.pmp.get_volume_data().version;
        ioc
    }

    pub(crate) fn ioctl_pfs_get(
        &mut self,
        ioc: &libhammer2::ioctl::IocPfs,
    ) -> libhammer2::Result<libhammer2::ioctl::IocPfs> {
        let (mut pcid, mut cid) = if ioc.name_key == u64::MAX {
            let cid = self.pmp.get_inode_chain(
                libhammer2::inode::INUM_PFS_ROOT,
                libhammer2::hammer2::RESOLVE_ALWAYS,
            )?;
            (libhammer2::chain::CID_NONE, cid)
        } else {
            let pcid = self.pmp.get_inode_chain(
                libhammer2::inode::INUM_SUP_ROOT,
                libhammer2::hammer2::RESOLVE_ALWAYS,
            )?;
            if pcid == libhammer2::chain::CID_NONE {
                return Err(nix::errno::Errno::EIO.into());
            }
            let (pcid, cid, _) =
                self.pmp
                    .lookup_chain(pcid, ioc.name_key, libhammer2::fs::HAMMER2_KEY_MAX, 0)?;
            (pcid, cid)
        };
        while cid != libhammer2::chain::CID_NONE {
            if self.get_chain(cid)?.get_blockref().typ == libhammer2::fs::HAMMER2_BREF_TYPE_INODE {
                break;
            }
            (pcid, cid, _) =
                self.pmp
                    .get_next_chain(pcid, cid, libhammer2::fs::HAMMER2_KEY_MAX, 0)?;
        }
        if cid == libhammer2::chain::CID_NONE {
            return Err(nix::errno::Errno::ENOENT.into());
        }
        let data = self.get_chain(cid)?.get_data();
        let ipdata = libhammer2::ondisk::media_as_inode_data(data);
        let mut ioc = *ioc;
        ioc.name_key = ipdata.meta.name_key;
        ioc.pfs_type = ipdata.meta.pfs_type;
        ioc.pfs_subtype = ipdata.meta.pfs_subtype;
        ioc.pfs_clid = ipdata.meta.pfs_clid;
        ioc.pfs_fsid = ipdata.meta.pfs_fsid;
        ioc.copy_name(&ipdata.filename);
        if pcid == libhammer2::chain::CID_NONE {
            ioc.name_next = u64::MAX;
        } else {
            (_, cid, _) = self
                .pmp
                .get_next_chain(pcid, cid, libhammer2::fs::HAMMER2_KEY_MAX, 0)?;
            ioc.name_next = if cid == libhammer2::chain::CID_NONE {
                u64::MAX
            } else {
                self.get_chain(cid)?.get_blockref().key
            };
        }
        Ok(ioc)
    }

    pub(crate) fn ioctl_pfs_lookup(
        &mut self,
        ioc: &libhammer2::ioctl::IocPfs,
    ) -> libhammer2::Result<libhammer2::ioctl::IocPfs> {
        let pcid = self.pmp.get_inode_chain(
            libhammer2::inode::INUM_SUP_ROOT,
            libhammer2::hammer2::RESOLVE_ALWAYS,
        )?;
        if pcid == libhammer2::chain::CID_NONE {
            return Err(nix::errno::Errno::EIO.into());
        }
        let lhc = ioc.get_name_lhc()?;
        let (mut pcid, mut cid, _) =
            self.pmp
                .lookup_chain(pcid, lhc, lhc + libhammer2::fs::HAMMER2_DIRHASH_LOMASK, 0)?;
        while cid != libhammer2::chain::CID_NONE {
            if self.get_chain(cid)?.match_name_from_bytes(&ioc.get_name()?) {
                break;
            }
            (pcid, cid, _) = self.pmp.get_next_chain(
                pcid,
                cid,
                lhc + libhammer2::fs::HAMMER2_DIRHASH_LOMASK,
                0,
            )?;
        }
        if cid == libhammer2::chain::CID_NONE {
            return Err(nix::errno::Errno::ENOENT.into());
        }
        let data = self.get_chain(cid)?.get_data();
        let ipdata = libhammer2::ondisk::media_as_inode_data(data);
        let mut ioc = *ioc;
        ioc.name_key = ipdata.meta.name_key;
        ioc.pfs_type = ipdata.meta.pfs_type;
        ioc.pfs_subtype = ipdata.meta.pfs_subtype;
        ioc.pfs_clid = ipdata.meta.pfs_clid;
        ioc.pfs_fsid = ipdata.meta.pfs_fsid;
        Ok(ioc)
    }

    pub(crate) fn ioctl_inode_get(
        &self,
        inum: u64,
        ioc: &libhammer2::ioctl::IocInode,
    ) -> nix::Result<libhammer2::ioctl::IocInode> {
        let Some(ip) = self.pmp.get_inode(inum) else {
            return Err(nix::errno::Errno::ENOENT);
        };
        let stats = self.pmp.get_inode_embed_stats(inum);
        let mut ioc = *ioc;
        ioc.data_count = stats.data_count;
        ioc.inode_count = stats.inode_count;
        ioc.ip_data = libhammer2::fs::Hammer2InodeData::new();
        ioc.ip_data.meta = *ip.get_meta();
        Ok(ioc)
    }

    pub(crate) fn ioctl_debug_dump(&self, inum: u64) -> nix::Result<()> {
        let Some(ip) = self.pmp.get_inode(inum) else {
            return Err(nix::errno::Errno::ENOENT);
        };
        if self.daemonized {
            log::error!("daemonized");
            Err(nix::errno::Errno::EOPNOTSUPP)
        } else {
            self.pmp.dump_inode_chain(ip)
        }
    }

    pub(crate) fn ioctl_volume_list(
        &self,
        ioc: &libhammer2::ioctl::IocVolumeList,
    ) -> nix::Result<libhammer2::ioctl::IocVolumeList> {
        if ioc.nvolumes > libhammer2::fs::HAMMER2_MAX_VOLUMES.into() {
            return Err(nix::errno::Errno::EINVAL);
        }
        Err(nix::errno::Errno::EOPNOTSUPP)
    }

    pub(crate) fn ioctl_volume_list2(
        &self,
        ioc: &libhammer2::ioctl::IocVolumeList2,
    ) -> nix::Result<libhammer2::ioctl::IocVolumeList2> {
        if ioc.nvolumes > libhammer2::fs::HAMMER2_MAX_VOLUMES.into() {
            return Err(nix::errno::Errno::EINVAL);
        }
        let mut ioc = *ioc;
        let mut nvolumes = 0;
        for (i, vol) in self.pmp.get_volumes().iter().enumerate() {
            if i >= ioc.nvolumes.try_into().unwrap() {
                break;
            }
            let entry = &mut ioc.volumes[i];
            entry.id = vol.get_id().try_into().unwrap();
            entry.copy_path(vol.get_path().as_bytes());
            entry.offset = vol.get_offset();
            entry.size = vol.get_size();
            nvolumes += 1;
        }
        ioc.nvolumes = nvolumes;
        ioc.version = self.pmp.get_volume_data().version;
        ioc.copy_pfs_name(self.pmp.get_label().as_bytes());
        Ok(ioc)
    }

    fn get_chain(&self, cid: libhammer2::chain::Cid) -> nix::Result<&libhammer2::chain::Chain> {
        self.pmp.get_chain(cid).ok_or(nix::errno::Errno::ENOENT)
    }
}
