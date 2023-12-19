use clap::{Arg, ArgAction, Command};
use libublk::{
    ctrl::UblkCtrl,
    dev_flags::UBLK_DEV_F_ADD_DEV,
    io::{UblkIOCtx, UblkQueue},
    sys::{ublk_param_basic, ublk_params, UBLK_PARAM_TYPE_BASIC},
    UblkIORes, UblkSession, UblkSessionBuilder,
};

mod kernel;
mod layout;

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
            let depth = 64;
            add_vblock_device(id, nr_queues, depth);
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
            dev.set_target_json(serde_json::json!({"vblock": id}));

            Ok(0)
        })
        .unwrap();

    sess.run_target(
        &mut ctrl,
        &dev,
        |queue_id, dev| {
            UblkQueue::new(queue_id, dev)
                .unwrap()
                .wait_and_handle_io(|queue, tag, io_ctx| {
                    let io_descriptor = queue.get_iod(tag);
                    // TODO: Is this mask needed?
                    let op = io_descriptor.op_flags & 0xff;
                    let data = UblkIOCtx::build_user_data(tag, op, 0, true);
                    if io_ctx.is_tgt_io() {
                        let user_data = io_ctx.user_data();
                        let res = io_ctx.result();
                        let cqe_tag = UblkIOCtx::user_data_to_tag(user_data);

                        assert!(cqe_tag == tag as u32);

                        // -11 == EAGAIN
                        if res != -11 {
                            queue.complete_io_cmd(tag, Ok(UblkIORes::Result(res)));
                            return;
                        }
                    }

                    // TODO: properly
                    // let res = todo!();
                    let res = -5; // EIO
                    if res < 0 {
                        queue.complete_io_cmd(tag, Ok(UblkIORes::Result(res)));
                    } else {
                        todo!();
                    }
                });
        },
        move |device_id| {
            let mut device_ctrl = UblkCtrl::new_simple(device_id, 0).unwrap();
            device_ctrl.dump();
        },
    )
    .unwrap();
}
