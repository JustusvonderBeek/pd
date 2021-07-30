mod cmdline_handler;
mod packets;

#[macro_use] extern crate log;
extern crate simplelog;

use std::{time, env, io};
use simplelog::*;
use std::fs::File;
use std::fs;
use pretty_hex::*;
use std::error::Error;
use std::net::UdpSocket;
use std::collections::HashSet;
use byteorder::BigEndian;
use crate::cmdline_handler::*;

enum PacketType {
    Request,
    Response,
    Data,
    Ack,
    Metadata,
    Error
}


fn main() -> std::io::Result<()> {

    // println!("Hello, world!");
    let args: Vec<String> = env::args().collect();
    let opt = parse_cmdline(args).expect("Failed to parse the commandline");

    init_logger(&opt);
    test_functions();

    if opt.server {
        start_server(&opt);
    } else {
        start_client(&opt);
    }

    Ok(())
}

fn start_server(opt : &Options) {

    info!("Starting the server...");

    // Listening on any socket...
    // TODO: Bind on given address
    let sock = UdpSocket::bind("0.0.0.0:5001").unwrap();
    info!("Server is listening on {}", sock.local_addr().unwrap());

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
    
}

fn test_functions() {
    handling_requests(PacketType::Request);

}

fn handling_requests(p_type : PacketType) {
    match p_type  {
        PacketType::Request => {
            debug!("Requst packet");
            let file = fs::read("Test.txt").unwrap();
            // debug!("Länge: {}\n{:?}\n{}", file.len(), file, String::from_utf8_lossy(&file));
            debug!("{}", pretty_hex(&file));
        },
        PacketType::Response => {

        },
        PacketType::Data => {

        },
        PacketType::Ack => {

        },
        PacketType::Metadata => {

        },
        PacketType::Error => {

        }
    }
}

fn start_client(opt : &Options) {
    // TODO: Implement client side
}

fn init_logger(opt : &Options) {
    CombinedLogger::init(
        vec![
            TermLogger::new(LevelFilter::Debug, Config::default(), TerminalMode::Mixed, ColorChoice::Auto),
            WriteLogger::new(LevelFilter::Trace, Config::default(), File::create(opt.logfile.to_string()).unwrap()),
        ]
    ).unwrap();

    debug!("Logger initilized");
}