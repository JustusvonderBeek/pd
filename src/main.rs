mod cmdline_handler;
mod packets;

use std::{time, env, io};
use std::error::Error;
use std::collections::HashSet;
use byteorder::BigEndian;
use tokio::net::UdpSocket;
use crate::cmdline_handler::*;

const THREAD_NUMBER : usize = 1 << 3;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // println!("Hello, world!");
    let args: Vec<String> = env::args().collect();
    let opt = parse_cmdline(args).expect("Failed to parse the commandline");

    if opt.server {
        start_server(opt).await?;
    } else {
        start_client(opt).await?;
    }

    Ok(())
}

async fn start_server(opt : Options) -> Result<(), Box<dyn Error>> {
    println!("Starting the server...");

    // Listening on any socket...
    let sock = UdpSocket::bind("0.0.0.0:5001").await.unwrap();
    println!("Server is listening on {}", sock.local_addr()?);

    // Used to keep track of active connections and IDs
    let mut conn_exists = HashSet::new();

    for i in 0..THREAD_NUMBER {
        std::thread::spawn(move || {
            server_task();
        });
    }

    let mut buf : [u8; 1500] = [0; 1500];
    let mut index = 0;

    loop {
        let (len, addr) = sock.recv_from(&mut buf).await.unwrap();
        println!("Received {} bytes from {}", len, addr);

        let raw_conn_id : [u8; 4] = [0, buf[0], buf[1], buf[2]];
        let conn_id : u32 = u32::from_be_bytes(raw_conn_id);
        println!("Read connID: {:x}", conn_id);

        if conn_id == 0 {
            // In this case the client is new

            index = (index + 1) % THREAD_NUMBER;
            // Create a new connectionID
            let conn_id = 10;
            conn_exists.insert(conn_id);


        } else {
            // The client already exists and we need to distribute the
            // packet to the correct thread/task ...
            
            
        }
    } 
    
    // Ok(())
}

fn server_task() {

    let mut buf : [u8; 1500] = [0; 1500];
    
    loop {
        // let (len, addr) = socket.recv_from(&mut buf).await.unwrap();
        // println!("Received {} bytes from {}", len, addr);
    } 

}

async fn start_client(opt: Options) -> Result<(), Box<dyn Error>> {
    println!("Starting the file transfer...");
    
    
    Ok(())
}