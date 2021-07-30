mod cmdline_handler;
mod packets;

#[macro_use] extern crate log;
extern crate simplelog;

use std::{time, env, io};
use simplelog::*;
use std::fs::File;
use std::error::Error;
use std::net::UdpSocket;
use std::collections::HashSet;
use byteorder::BigEndian;
use crate::cmdline_handler::*;


fn main() -> std::io::Result<()> {
    init_logger();
    // println!("Hello, world!");
    let args: Vec<String> = env::args().collect();
    let opt = parse_cmdline(args).expect("Failed to parse the commandline");

    if opt.server {
        start_server(opt).expect("Error on server!");
    } else {
        start_client(opt);
    }

    Ok(())
}

fn start_server(opt : Options) -> std::io::Result<()> {
    info!("Starting the server...");

    // Listening on any socket...
    // TODO: Bind on given address
    let sock = UdpSocket::bind("0.0.0.0:5001").unwrap();
    info!("Server is listening on {}", sock.local_addr()?);

    // Used to keep track of active connections and IDs
    let mut conn_exists = HashSet::new();


    let mut buf : [u8; 1500] = [0; 1500];
    let mut index = 0;

    loop {
        // TODO: Error handling
        let (len, addr) = sock.recv_from(&mut buf).expect("Failed to receive data");
        info!("Received {} bytes from {}", len, addr);

        let raw_conn_id : [u8; 4] = [0, buf[0], buf[1], buf[2]];
        let conn_id : u32 = u32::from_be_bytes(raw_conn_id);
        debug!("Read connID: {:x}", conn_id);

        if conn_id == 0 {
            // In this case the client is new

            index = (index + 1) % 10;
            // Create a new connectionID
            let conn_id = 10;
            conn_exists.insert(conn_id);


        } else {
            // The client already exists and we need to distribute the
            // packet to the correct thread/task ...
            
            break;
        }
    } 
    
    Ok(())
}

fn handling_requests() {

}

fn start_client(opt : Options) {
    // TODO: Implement client side
}

fn init_logger() {
    CombinedLogger::init(
        vec![
            TermLogger::new(LevelFilter::Debug, Config::default(), TerminalMode::Mixed, ColorChoice::Auto),
            WriteLogger::new(LevelFilter::Trace, Config::default(), File::create("tbd.log").unwrap()),
        ]
    ).unwrap();

    info!("Logger initilized");
}