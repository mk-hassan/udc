use std::env;
use std::sync::atomic::Ordering;

use udc::config::Config;
use udc::pipeline::{self, Pipeline};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // parse and validate command line arguments
    let args = env::args().collect::<Vec<String>>();
    let config = Config::build(&args[1..]).unwrap_or_else(|message| {
        eprintln!("{}", message);
        std::process::exit(1);
    });

    // initialize signal handler for printing metrics on demand
    setup_signal_handler();

    let mut pipeline = Pipeline::build(config).unwrap_or_else(|err| {
        eprintln!("{}", err);
        std::process::exit(1);
    });

    pipeline.run().unwrap_or_else(|err| {
        eprintln!("{}", err);
        std::process::exit(1);
    });

    pipeline.print_metrics();
    Ok(())
}

#[cfg(target_family = "unix")]
fn setup_signal_handler() {
    use signal_hook::{consts::SIGUSR1, iterator::Signals};
    let mut signals = Signals::new([SIGUSR1]).unwrap();
    std::thread::spawn(move || {
        for _sig in signals.forever() {
            pipeline::PRINT_REQUEST.store(true, Ordering::SeqCst);
        }
    });
}

#[cfg(target_family = "windows")]
fn setup_signal_handler() {
    unsafe extern "system" fn ctrl_handler(ctrl_type: u32) -> i32 {
        const CTRL_BREAK_EVENT: u32 = 1;
        if ctrl_type == CTRL_BREAK_EVENT {
            pipeline::PRINT_REQUEST.store(true, Ordering::SeqCst);
            return 1; // signal handled
        }
        0 // pass to next handler
    }

    unsafe extern "system" {
        fn SetConsoleCtrlHandler(
            handler: Option<unsafe extern "system" fn(u32) -> i32>,
            add: i32,
        ) -> i32;
    }

    unsafe {
        SetConsoleCtrlHandler(Some(ctrl_handler), 1);
    }
}
