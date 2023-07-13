/// Size of a logical block in bytes. LBA offset is always a multiple of LBA size.
const LBA_SIZE: usize = 4096;
/// Amount of contiguous logical blocks in a single cluster.
const CLUSTER_SIZE: usize = 256;

/// Virtual block device used by a single consumer. The VBlock is exposed as a contiguous
/// allocation to the consumer
pub struct VBlock {}

/// Metdata information about a [`VBlock`] stored in the [`MasterBlock`] metadata cluster.
struct VBlockMeta {
    /// Unique numeric ID of the block, created on assignment.
    id: u32,
    /// Maximum allowed size in clusters.
    size: u32,
    /// Allocated clusters.
    allocated: u32,
}

/// The master block is a single virtual block device which governs the underlying storage. Every
/// [`VBlock`] is created from the MasterBlock. Allocations of [`VBlock`]'s can be interleaved.
///
/// The MasterBlock starts with a reserved `Metadata cluster`. This contains metadata about the
/// individual [`VBlock`] devices allocated, as well as the mapping of their allocation.
pub struct MasterBlock {
    /// Total amount of clusters.
    size: u32,
    /// Amount of allocated clusters.
    allocated: u32,
    /// List of block meta loaded.
    vblocks: Vec<VBlockMeta>,
}

pub trait BackingStorage {}

pub fn main() {}
