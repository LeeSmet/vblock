use std::{
    collections::HashMap,
    fs::OpenOptions,
    io,
    os::{fd::AsRawFd, unix::prelude::OpenOptionsExt},
    path::PathBuf,
    rc::Rc,
};

use clap::{Arg, ArgAction, Command};
use io_uring::{opcode, squeue, types};
use libublk::{
    ctrl::UblkCtrl,
    dev_flags::{UBLK_DEV_F_ADD_DEV, UBLK_DEV_F_ASYNC},
    exe::{Executor, UringOpFuture},
    io::{UblkDev, UblkIOCtx, UblkQueue},
    sys::{
        ublk_param_basic, ublk_params, UBLK_IO_COMMIT_AND_FETCH_REQ, UBLK_IO_FETCH_REQ,
        UBLK_IO_RES_ABORT, UBLK_PARAM_TYPE_BASIC,
    },
    UblkSession, UblkSessionBuilder,
};

mod kernel;
mod layout;

/// -libc::EINVAL error code
const EINVAL: i32 = -22;
/// -libc::EAGAIN error code
const EAGAIN: i32 = -11;

/// libc::O_DIRECT flag
const O_DIRECT: i32 = 0x4000;

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
                )
                .arg(
                    Arg::new("target")
                        .short('t')
                        .long("target")
                        .help("backing device")
                        .action(ArgAction::Set),
                ),
        )
        .subcommand(
            Command::new("del")
                .about("Delete a virtual block device")
                .arg(
                    Arg::new("id")
                        .long("id")
                        .required(true)
                        .help("device id to delete")
                        .action(ArgAction::Set),
                ),
        )
        .subcommand(Command::new("list").about("List all virtual block devices"))
        .subcommand(Command::new("features").about("List all supported features"))
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
            let target = add_matches.get_one::<String>("target").unwrap();
            let depth = 1024;
            add_vblock_device(id, nr_queues, depth, target.into());
        }
        Some(("list", _)) => UblkSession::for_each_dev_id(|dev_id| {
            UblkCtrl::new_simple(dev_id as i32, 0).unwrap().dump();
        }),
        Some(("del", del_matches)) => {
            let id = del_matches
                .get_one::<String>("id")
                .unwrap()
                .parse::<i32>()
                .unwrap();
            let mut ctrl = UblkCtrl::new_simple(id, 0).unwrap();
            // Stop the device
            let _ = ctrl.kill_dev();
            // And remove it
            let _ = ctrl.del_dev();
        }
        Some(("features", _)) => match UblkCtrl::get_features() {
            Some(f) => {
                const NR_FEATURES: usize = 9;
                const FEATURES_TABLE: [&'static str; NR_FEATURES] = [
                    "ZERO_COPY",
                    "COMP_IN_TASK",
                    "NEED_GET_DATA",
                    "USER_RECOVERY",
                    "USER_RECOVERY_REISSUE",
                    "UNPRIVILEGED_DEV",
                    "CMD_IOCTL_ENCODE",
                    "USER_COPY",
                    "ZONED",
                ];
                println!("\t{:<22} {:#12x}", "UBLK FEATURES", f);
                for i in 0..64 {
                    if ((1_u64 << i) & f) == 0 {
                        continue;
                    }

                    let feat = if i < NR_FEATURES {
                        FEATURES_TABLE[i]
                    } else {
                        "unknown"
                    };
                    println!("\t{:<22} {:#12x}", feat, 1_u64 << i);
                }
            }
            None => eprintln!("not support GET_FEATURES, require linux v6.5"),
        },
        _ => println!("Unsupported command"),
    }
}

/// Add a new virtual block device
fn add_vblock_device(id: i32, nr_queues: u32, depth: u32, target: PathBuf) {
    let (backing, target) = Backing::new(target).unwrap();

    let sess = UblkSessionBuilder::default()
        .name("vblock")
        .id(id)
        //.ctrl_flags(libublk::sys::UBLK_F_UNPRIVILEGED_DEV)
        .nr_queues(nr_queues)
        .depth(depth)
        // TODO: figure out good value here
        .io_buf_bytes(1u32 << 19)
        .dev_flags(UBLK_DEV_F_ADD_DEV | UBLK_DEV_F_ASYNC)
        .build()
        .unwrap();

    let (mut ctrl, dev) = sess
        .create_devices(|dev| {
            // Register backing file -> allows uring fixed io
            let tgt = &mut dev.tgt;
            let nr_fds = tgt.nr_fds;
            tgt.fds[nr_fds as usize] = target.as_raw_fd();
            tgt.nr_fds += 1;

            dev.tgt.dev_size = 10 << 30;
            dev.tgt.params = ublk_params {
                types: UBLK_PARAM_TYPE_BASIC,
                basic: ublk_param_basic {
                    // TODO: figure out these params
                    logical_bs_shift: 9,
                    physical_bs_shift: 9,
                    // bitshifts of 1 in sector?
                    io_opt_shift: 9,
                    io_min_shift: 9,
                    max_sectors: dev.dev_info.max_io_buf_bytes >> 9,
                    dev_sectors: dev.tgt.dev_size >> 9,
                    ..Default::default()
                },
                ..Default::default()
            };
            dev.set_target_json(serde_json::json!({"vblock": id}));

            Ok(0)
        })
        .unwrap();

    sess.run_target(&mut ctrl, &dev, backing.as_queue_handler(), |device_id| {
        let mut device_ctrl = UblkCtrl::new_simple(device_id, 0).unwrap();
        device_ctrl.dump();
    })
    .unwrap();
}

#[derive(Clone)]
struct Backing {
    // Map of 1GB areas of Vdisk to actual backing.
    mapping: HashMap<u64, u64>,
}

impl Backing {
    fn as_queue_handler(self) -> impl FnOnce(u16, &UblkDev) + Send + Sync + Clone + 'static {
        move |queue_id, dev| self.queue_handler(queue_id, dev)
    }

    fn new(path: PathBuf) -> Result<(Self, std::fs::File), io::Error> {
        let target = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(O_DIRECT)
            .open(&path)?;

        // TODO: temp for testing
        let mapping = (0..10).into_iter().map(|i| (i, i + 1)).collect();

        Ok((Backing { mapping }, target))
    }

    fn queue_handler(&self, queue_id: u16, dev: &UblkDev) {
        let queue = Rc::new(UblkQueue::new(queue_id, dev).unwrap());
        let exe = Executor::new(dev.get_nr_ios());

        let depth = dev.dev_info.queue_depth;

        for tag in 0..depth as u16 {
            let queue = queue.clone();
            exe.spawn(tag as u16, async move {
                let buf_addr = queue.get_io_buf_addr(tag);
                // This MUST be the first command submitted.
                let mut cmd_op = UBLK_IO_FETCH_REQ;
                let mut res = 0;
                loop {
                    let cmd_res = queue.submit_io_cmd(tag, cmd_op, buf_addr, res).await;
                    if cmd_res == UBLK_IO_RES_ABORT {
                        break;
                    }

                    res = handle_io_cmd(&queue, tag).await;
                    cmd_op = UBLK_IO_COMMIT_AND_FETCH_REQ;
                }
            });
        }

        queue.wait_and_wake_io_tasks(&exe);
        // Sync version?
        //queue.wait_and_handle_io(|queue, tag, io_ctx| {
        //    let io_descriptor = queue.get_iod(tag);
        //    // TODO: Is this mask needed?
        //    let op = io_descriptor.op_flags & 0xff;
        //    let data = UblkIOCtx::build_user_data(tag, op, 0, true);
        //    if io_ctx.is_tgt_io() {
        //        let user_data = io_ctx.user_data();
        //        let res = io_ctx.result();
        //        let cqe_tag = UblkIOCtx::user_data_to_tag(user_data);

        //        assert!(cqe_tag == tag as u32);

        //        // -11 == EAGAIN
        //        if res != -11 {
        //            queue.complete_io_cmd(tag, Ok(UblkIORes::Result(res)));
        //            return;
        //        }
        //    }

        //    // TODO: properly
        //    // let res = todo!();
        //    let res = -5; // EIO
        //    if res < 0 {
        //        queue.complete_io_cmd(tag, Ok(UblkIORes::Result(res)));
        //    } else {
        //        todo!();
        //    }
        //});
    }
}

#[inline]
fn prep_io_cmd_submission(io_descriptor: &libublk::sys::ublksrv_io_desc) -> i32 {
    let op = io_descriptor.op_flags & 0xff;

    match op {
        libublk::sys::UBLK_IO_OP_FLUSH
        | libublk::sys::UBLK_IO_OP_READ
        | libublk::sys::UBLK_IO_OP_WRITE => return 0,
        _ => return EINVAL,
    };
}

#[inline]
fn submit_io_cmd(
    queue: &UblkQueue<'_>,
    tag: u16,
    io_descriptor: &libublk::sys::ublksrv_io_desc,
    data: u64,
) {
    let op = io_descriptor.op_flags & 0xff;
    // either start to handle or retry
    // Add 1 GiB for now
    // TODO: proper offset calculation
    let off = (io_descriptor.start_sector << 9) as u64 + (1 << 30);
    let bytes = (io_descriptor.nr_sectors << 9) as u32;
    let buf_addr = queue.get_io_buf_addr(tag);

    match op {
        libublk::sys::UBLK_IO_OP_FLUSH => {
            let sqe = &opcode::SyncFileRange::new(types::Fixed(1), bytes)
                .offset(off)
                .build()
                .flags(squeue::Flags::FIXED_FILE)
                .user_data(data);
            unsafe {
                queue
                    .q_ring
                    .borrow_mut()
                    .submission()
                    .push(sqe)
                    .expect("flush submission fail");
            }
        }
        libublk::sys::UBLK_IO_OP_READ => {
            let sqe = &opcode::Read::new(types::Fixed(1), buf_addr, bytes)
                .offset(off)
                .build()
                .flags(squeue::Flags::FIXED_FILE)
                .user_data(data);
            unsafe {
                queue
                    .q_ring
                    .borrow_mut()
                    .submission()
                    .push(sqe)
                    .expect("read submission fail");
            }
        }
        libublk::sys::UBLK_IO_OP_WRITE => {
            let sqe = &opcode::Write::new(types::Fixed(1), buf_addr, bytes)
                .offset(off)
                .build()
                .flags(squeue::Flags::FIXED_FILE)
                .user_data(data);
            unsafe {
                queue
                    .q_ring
                    .borrow_mut()
                    .submission()
                    .push(sqe)
                    .expect("write submission fail");
            }
        }
        _ => {}
    };
}

async fn handle_io_cmd(queue: &UblkQueue<'_>, tag: u16) -> i32 {
    let iod = queue.get_iod(tag);
    let op = iod.op_flags & 0xff;
    let user_data = UblkIOCtx::build_user_data_async(tag as u16, op, 0);
    let res = prep_io_cmd_submission(iod);
    if res < 0 {
        return res;
    }

    for _ in 0..4 {
        submit_io_cmd(queue, tag, iod, user_data);
        let res = UringOpFuture { user_data }.await;
        if res != EAGAIN {
            return res;
        }
    }

    return EAGAIN;
}
