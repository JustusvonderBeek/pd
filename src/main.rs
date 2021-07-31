mod cmdline_handler;
mod packets;

#[macro_use] extern crate log;
extern crate simplelog;

use std::{time, env, io};
use simplelog::*;
use std::fs::File;
use std::io::Write;
use std::fs::OpenOptions;
use std::fs;
use pretty_hex::*;
use std::error::Error;
use std::net::UdpSocket;
use std::collections::HashSet;
use byteorder::BigEndian;
use rand::prelude::*;
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

    let mut hostname = String::from(&opt.hostname);
    hostname.push_str(":");
    hostname.push_str(&opt.local_port.to_string());
    let sock = UdpSocket::bind(hostname).unwrap();
    info!("Server is listening on {}", sock.local_addr().unwrap());

    // Used to keep track of active connections and IDs
    let mut conn_exists = HashSet::new();

    let mut buf : [u8; 1300] = [0; 1300];
    let mut index = 0;

    loop {
        // TODO: Error handling
        // 1. Reading a new packet (because this is not threaded block here)
        let (len, addr) = sock.recv_from(&mut buf).expect("Failed to receive data");
        debug!("Received {} bytes from {}", len, addr);
        
        // 2. Check for a new connection


        let raw_conn_id : [u8; 4] = [0, buf[0], buf[1], buf[2]];
        let conn_id : u32 = u32::from_be_bytes(raw_conn_id);
        debug!("Read connID: {:x}", conn_id);

        // 3. Parse the given packet

        if conn_id == 0 {
            // In this case the client is new

            index = (index + 1) % 10;
            // Create a new connectionID
            let conn_id = 10;
            conn_exists.insert(conn_id);

            // 4.1. Handling a new client (Setup)
        } else {
           // 4.2. Handling an old client (Probably Ack or Metadata)
        }
    } 
    
}

fn test_functions() {
    handling_requests(PacketType::Request, &vec![1;1]);
    handling_requests(PacketType::Data, &vec![1;1]);
}

fn handling_requests(p_type : PacketType, packet : &Vec<u8>) {

    match p_type  {
        PacketType::Request => {
            debug!("Requst packet");
            let file = fs::read("Test.txt").unwrap();
            debug!("{}", pretty_hex(&file));
        },
        PacketType::Response => {
            // Connection setup (we ack that we got the request and the file parameters)
        },
        PacketType::Data => {
            // Transfer the next chunk of data to the client
            let mut rnd = rand::thread_rng();
            let mut nums : Vec<u8> = (0..100).collect();
            nums.shuffle(&mut rnd);
            debug!("{}", pretty_hex(&nums));
            let mut output = OpenOptions::new().append(true).create(true).open("output.txt").expect("Failed to open file output.txt");
            output.write_all(&nums).expect("Failed to append to file");
        },
        PacketType::Ack => {
            // Got an ack from the client
            
        },
        PacketType::Metadata => {
            // This type of packet is never received by us !
        },
        PacketType::Error => {
            // Closing the connection and freeing state
            // conn_exists.remove(1);
        }
    }
}

fn start_client(opt : &Options) {
    // TODO: Implement client side
    // 1. Request the file

    // 2. Receive data in the loop

    // 3. Ack the data and process the metadata

    // 4. Close the connection after success
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