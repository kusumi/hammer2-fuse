mod fuse;
mod ioctl;
mod util;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const HAMMER2_HOME: &str = "HAMMER2_HOME";
const HAMMER2_CIDALLOC: &str = "HAMMER2_CIDALLOC";

struct Hammer2Fuse {
    pmp: libhammer2::hammer2::Hammer2,
    total_open: usize,
    debug: i32,
    daemonized: bool,
}

impl Hammer2Fuse {
    fn new(pmp: libhammer2::hammer2::Hammer2, debug: i32, daemonized: bool) -> Self {
        Self {
            pmp,
            total_open: 0,
            debug,
            daemonized,
        }
    }
}

fn init_std_logger() -> std::result::Result<(), log::SetLoggerError> {
    let env = env_logger::Env::default().filter_or(
        "RUST_LOG",
        if util::is_debug_set() {
            "trace"
        } else {
            "info"
        },
    );
    env_logger::try_init_from_env(env)
}

fn init_file_logger(prog: &str) -> Result<()> {
    let dir = util::get_home_path()?;
    let name = format!(
        ".{}.log",
        match util::get_basename(prog) {
            Some(v) => v,
            None => "hammer2-fuse".to_string(),
        }
    );
    let f = match std::env::var(HAMMER2_HOME) {
        Ok(v) => {
            if util::is_dir(&v) {
                util::join_path(&v, &name)?
            } else {
                eprintln!("{HAMMER2_HOME} not a directory, using {dir} instead");
                util::join_path(&dir, &name)?
            }
        }
        Err(_) => return Err(Box::new(nix::errno::Errno::ENOENT)),
    };
    Ok(simplelog::CombinedLogger::init(vec![
        simplelog::WriteLogger::new(
            if util::is_debug_set() {
                simplelog::LevelFilter::Trace
            } else {
                simplelog::LevelFilter::Info
            },
            simplelog::Config::default(),
            std::fs::File::create(f)?,
        ),
    ])?)
}

fn init_syslog_logger(prog: &str) -> Result<()> {
    let formatter = syslog::Formatter3164 {
        facility: syslog::Facility::LOG_USER,
        hostname: None,
        process: match util::get_basename(prog) {
            Some(v) => v,
            None => "hammer2-fuse".to_string(),
        },
        pid: 0,
    };
    let logger = syslog::unix(formatter)?;
    Ok(
        log::set_boxed_logger(Box::new(syslog::BasicLogger::new(logger))).map(|()| {
            log::set_max_level(if util::is_debug_set() {
                //log::LevelFilter::Trace // XXX not traced
                log::LevelFilter::Info
            } else {
                log::LevelFilter::Info
            });
        })?,
    )
}

fn usage(prog: &str, gopt: &getopts::Options) {
    print!(
        "{}",
        gopt.usage(&format!("Usage: {prog} [options] special[@label] node"))
    );
}

#[allow(clippy::too_many_lines)]
fn main() {
    println!(
        "FUSE hammer2 {}.{}.{} (fuser)",
        libhammer2::VERSION[0],
        libhammer2::VERSION[1],
        libhammer2::VERSION[2]
    );

    let args: Vec<String> = std::env::args().collect();
    let prog = &args[0];

    let mut gopt = getopts::Options::new();
    // https://docs.rs/fuser/latest/fuser/enum.MountOption.html
    gopt.optflag(
        "",
        "allow_other",
        "Allow all users to access files on this filesystem. \
        By default access is restricted to the user who mounted it.",
    );
    gopt.optflag(
        "",
        "allow_root",
        "Allow the root user to access this filesystem, \
        in addition to the user who mounted it.",
    );
    gopt.optflag("", "noexec", "Dont allow execution of binaries.");
    if libhammer2::util::is_linux() {
        gopt.optflag(
            "",
            "auto_unmount",
            "Automatically unmount when the mounting process exits. \
            AutoUnmount requires AllowOther or AllowRoot. \
            If AutoUnmount is set and neither Allow... is set, \
            the FUSE configuration must permit allow_other, \
            otherwise mounting will fail. \
            Available on Linux.",
        );
    }
    gopt.optflag("d", "", "Enable env_logger logging and do not daemonize.");
    gopt.optflag("", "nodatacache", "Disable uncompressed data cache");
    gopt.optflag("V", "version", "Print version and copyright.");
    gopt.optflag("h", "help", "Print usage.");

    let matches = match gopt.parse(&args[1..]) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            usage(prog, &gopt);
            std::process::exit(1);
        }
    };
    if matches.opt_present("V") {
        std::process::exit(0);
    }
    if matches.opt_present("help") {
        usage(prog, &gopt);
        std::process::exit(0);
    }

    let args = &matches.free;
    if args.len() != 2 {
        usage(prog, &gopt);
        std::process::exit(1);
    }
    let spec = &args[0];
    let mntpt = &args[1];

    let mut fopt = vec![
        fuser::MountOption::FSName(spec.clone()),
        fuser::MountOption::Subtype("hammer2".to_string()),
        fuser::MountOption::DefaultPermissions,
        #[cfg(target_os = "linux")]
        fuser::MountOption::NoDev,
        fuser::MountOption::NoSuid,
    ];
    let mut mopt = vec![];
    // https://docs.rs/fuser/latest/fuser/enum.MountOption.html
    if matches.opt_present("allow_other") {
        fopt.push(fuser::MountOption::AllowOther);
    }
    if matches.opt_present("allow_root") {
        fopt.push(fuser::MountOption::AllowRoot);
    }
    if matches.opt_present("noexec") {
        fopt.push(fuser::MountOption::NoExec);
    } else {
        fopt.push(fuser::MountOption::Exec);
    }
    if libhammer2::util::is_linux() && matches.opt_present("auto_unmount") {
        fopt.push(fuser::MountOption::AutoUnmount);
    }
    let use_daemon = !matches.opt_present("d"); // not debug

    if util::is_debug_set() {
        mopt.push("--debug");
    }

    if matches.opt_present("nodatacache") {
        mopt.extend_from_slice(&["--nodatacache"]);
    }

    let cidalloc = std::env::var(HAMMER2_CIDALLOC).unwrap_or_default();
    if !cidalloc.is_empty() {
        mopt.extend_from_slice(&["--cidalloc", &cidalloc]);
    }

    if !use_daemon {
        if let Err(e) = init_std_logger() {
            eprintln!("{e}");
            std::process::exit(1);
        }
    } else if init_file_logger(prog).is_err() {
        if let Err(e) = init_syslog_logger(prog) {
            eprintln!("syslog logger: {e}");
        }
    }

    let pmp = match libhammer2::mount(spec, &mopt) {
        Ok(v) => v,
        Err(e) => {
            log::error!("{e}");
            if use_daemon {
                eprintln!("{e}");
            }
            std::process::exit(1);
        }
    };
    fopt.push(fuser::MountOption::RO);
    log::debug!("{fopt:?}");

    if use_daemon {
        // https://docs.rs/daemonize/latest/daemonize/struct.Daemonize.html
        if let Err(e) = daemonize::Daemonize::new().start() {
            log::error!("{e}");
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
    // fuser::mount2 doesn't return, hence after daemonize
    // XXX use fuser::spawn_mount2
    if let Err(e) = fuser::mount2(
        Hammer2Fuse::new(pmp, util::get_debug_level(), use_daemon),
        mntpt,
        &fopt,
    ) {
        log::error!("{e}");
        std::process::exit(1);
    }
}
