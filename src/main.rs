use std::env;

use ccdd::pipeline;
use ccdd::config::Config;

fn main() {
    let args = env::args().collect::<Vec<String>>();
    let config = Config::build(&args).unwrap_or_else(|message| {
        eprintln!("{}", message);
        std::process::exit(1);
    });

    let metrics = pipeline::run(
        &config.source, 
        &config.destination, 
        config.get_ibs(), 
        config.get_obs()
    ).unwrap_or_else(|err| {
        eprintln!("ccdd: {}", err);
        std::process::exit(1);
    });

    println!("{}", metrics);
}