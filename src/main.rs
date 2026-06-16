use std::env;
use std::sync::{ atomic::Ordering };
use signal_hook::{ consts::SIGUSR1, iterator::Signals };

use udc::pipeline::{ self, Pipeline };
use udc::config::Config;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // parse and validate command line arguments
    let args = env::args().collect::<Vec<String>>();
    let config = Config::build(&args[1..]).unwrap_or_else(|message| {
        eprintln!("{}", message);
        std::process::exit(1);
    });

    // initialize metrics and signal handler for SIGUSR1
    let mut signals = Signals::new([SIGUSR1]).unwrap();
    std::thread::spawn({
        move || {
            for _sig in signals.forever() {
                pipeline::PRINT_REQUEST.store(true, Ordering::SeqCst);
            }
        }
    });

    let mut pipeline = Pipeline::build(config)
        .unwrap_or_else(|err| {
            eprintln!("{}", err);
            std::process::exit(1);
        });

    pipeline.run()
        .unwrap_or_else(|err| {
            eprintln!("{}", err);
            std::process::exit(1);
        });

    pipeline.print_metrics();
    Ok(())
}
