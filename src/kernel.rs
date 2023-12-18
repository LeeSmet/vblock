use nix::{ioctl_none, ioctl_read};

/// Identifier for ioctl on block devices, defined in linux/fs.h
const BLK_IOCTL_ID: u8 = 0x12;

/// Ioctl sequence number for BLKSSZGET, defined in linux/fs.h
const BLK_SSZGET_IOCTL_SEQNO: u8 = 104;
/// Ioctl sequence number for BLKGETSIZE64, defined in linux/fs.h
const BLK_GETSIZE64_IOCTL_SEQNO: u8 = 114;
/// Ioctl sequence number for BLKIOMIN, defined in linux/fs.h
const BLK_IOMIN_IOCTL_SEQNO: u8 = 120;
/// Ioctl sequence number for BLKIOOPT, defined in linux/fs.h
const BLK_IOOPT_IOCTL_SEQNO: u8 = 121;
/// Ioctl sequence number for BLKSPBZGET, defined in linux/fs.h
const BLK_PBSZGET_IOCTL_SEQNO: u8 = 123;

ioctl_none! {
    /// Get the sector size / logical block size of a block device.
    ioctl_blksszget,
    BLK_IOCTL_ID,
    BLK_SSZGET_IOCTL_SEQNO
}

ioctl_read! {
    /// Get the size of a file or block device.
    ioctl_blkgetsize64,
    BLK_IOCTL_ID,
    BLK_GETSIZE64_IOCTL_SEQNO,
    u64
}

ioctl_none! {
    /// Get minimum io size of a block device.
    ioctl_blkiomin,
    BLK_IOCTL_ID,
    BLK_IOMIN_IOCTL_SEQNO
}

ioctl_none! {
    /// Get the optimal io size of a block device, if any.
    ioctl_blkioopt,
    BLK_IOCTL_ID,
    BLK_IOOPT_IOCTL_SEQNO
}

ioctl_none! {
    /// Get the physical block size of a block device.
    ioctl_blkpbszget,
    BLK_IOCTL_ID,
    BLK_SSZGET_IOCTL_SEQNO
}
