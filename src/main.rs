use std::env;
use std::sync::{ atomic::Ordering };
use signal_hook::{ consts::SIGUSR1, iterator::Signals };

use ccdd::pipeline;
use ccdd::config::{Config, PrintOption};

fn main() {
    // parse and validate command line arguments
    let args = env::args().collect::<Vec<String>>();
    let config = Config::build(&args).unwrap_or_else(|message| {
        eprintln!("{}", message);
        std::process::exit(1);
    });

    // initialize metrics and signal handler for SIGUSR1
    let mut signals = Signals::new(&[SIGUSR1]).unwrap();
    std::thread::spawn({
        move || {
            for _sig in signals.forever() {
                pipeline::PRINT_REQUEST.store(true, Ordering::SeqCst);
            }
        }
    });

    // run the pipeline and handle any errors
    let metrics = pipeline::run(&config)
        .unwrap_or_else(|err| {
            eprintln!("ccdd: {}", err);
            std::process::exit(1);
        });

    match config.get_print_option() {
        PrintOption::None => (),
        PrintOption::Noxfer => println!("{}", metrics.input_output_stats()),
        _ => println!("{}", metrics),
    }
}