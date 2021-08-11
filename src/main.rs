mod cmdline_handler;
mod packets;
mod server;
mod client;
mod utils;

#[macro_use] extern crate log;
extern crate simplelog;

use std::env;
use simplelog::*;
use std::fs::File;
use crate::cmdline_handler::*;
use crate::server::*;
use crate::client::*;

fn main() -> std::io::Result<()> {

    // println!("Hello, world!");
    let args: Vec<String> = env::args().collect();
    let opt = parse_cmdline(args).expect("Failed to parse the commandline");

    init_logger(&opt);

    if opt.server {
        let mut server = TBDServer::create(opt);
        return server.start();
    } else {
        let mut client = TBDClient::create(opt);
        return client.start();
    }
}

fn init_logger(opt : &Options) {
    CombinedLogger::init(
        vec![
            TermLogger::new(LevelFilter::Info, Config::default(), TerminalMode::Mixed, ColorChoice::Auto),
            WriteLogger::new(LevelFilter::Debug, Config::default(), File::create(opt.logfile.to_string()).unwrap()),
        ]
    ).unwrap();

    debug!("Logger initilized");
}