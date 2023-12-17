use clap::{Arg, ArgAction, Command};
use libublk::{
    ctrl::UblkCtrl,
    dev_flags::UBLK_DEV_F_ADD_DEV,
    sys::{ublk_param_basic, ublk_params, UBLK_F_UNPRIVILEGED_DEV, UBLK_PARAM_TYPE_BASIC},
    UblkSession, UblkSessionBuilder,
};

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

pub fn main() {
    // TODO: There are way better ways to do this.
    let matches = Command::new("vblock")
        .subcommand_required(true)
        .subcommand(
            Command::new("add")
                .about("Add a new virtual block device")
                .arg(
                    Arg::new("id")
                        .alias("number")
                        .short('n')
                        .long("id")
                        .help("device id")
                        .default_value("-1")
                        .allow_hyphen_values(true)
                        .action(ArgAction::Set),
                )
                .arg(
                    Arg::new("queues")
                        .short('q')
                        .long("queues")
                        .default_value("1")
                        .help("number of hardware queues")
                        .action(ArgAction::Set),
                ),
        )
        .subcommand(
            Command::new("del")
                .about("Delete a virtual block device")
                .arg(
                    Arg::new("number")
                        .short('n')
                        .long("number")
                        .required(true)
                        .help("device id to delete")
                        .action(ArgAction::Set),
                ),
        )
        .subcommand(Command::new("list").about("List all virtual block devices"))
        .get_matches();

    match matches.subcommand() {
        Some(("add", add_matches)) => {
            let id = add_matches
                .get_one::<String>("id")
                .unwrap()
                .parse::<i32>()
                .unwrap_or(-1);
            let nr_queues = add_matches
                .get_one::<String>("queues")
                .unwrap()
                .parse::<u32>()
                .unwrap_or(1);
            let depth = 64;
            add_vblock_device(id, nr_queues, depth);
        }
        Some(("list", _)) => UblkSession::for_each_dev_id(|dev_id| {
            UblkCtrl::new_simple(dev_id as i32, 0).unwrap().dump();
        }),
        _ => println!("Unsupported command"),
    }
}

/// Add a new virtual block device
fn add_vblock_device(id: i32, nr_queues: u32, depth: u32) {
    let sess = UblkSessionBuilder::default()
        .name("vblock")
        .id(id)
        //.ctrl_flags(libublk::sys::UBLK_F_UNPRIVILEGED_DEV)
        .nr_queues(nr_queues)
        .depth(depth)
        // TODO: figure out good value here
        .io_buf_bytes(1u32 << 20)
        .dev_flags(UBLK_DEV_F_ADD_DEV)
        .build()
        .unwrap();

    let (mut ctrl, dev) = sess
        .create_devices(|dev| {
            dev.tgt.dev_size = 10 << 30;
            dev.tgt.params = ublk_params {
                types: UBLK_PARAM_TYPE_BASIC,
                basic: ublk_param_basic {
                    // TODO: figure out these params
                    logical_bs_shift: 9,
                    physical_bs_shift: 9,
                    // bitshifts of 1 in sector?
                    io_opt_shift: 12,
                    io_min_shift: 9,
                    max_sectors: dev.dev_info.max_io_buf_bytes >> 9,
                    dev_sectors: dev.tgt.dev_size >> 9,
                    ..Default::default()
                },
                ..Default::default()
            };
            dev.set_target_json(serde_json::json!({"vlbock": id}));

            Ok(0)
        })
        .unwrap();
}
