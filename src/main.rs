mod cmdline_handler;
mod packets;

use std::{time, env, io};
use std::error::Error;
use byteorder::BigEndian;
use tokio::net::UdpSocket;
use crate::cmdline_handler::*;

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

    let mut buf : [u8; 1500] = [0; 1500];

    loop {
        let (len, addr) = sock.recv_from(&mut buf).await.unwrap();
        println!("Received {} bytes from {}", len, addr);

        let rawConnID : [u8; 4] = [0, buf[0], buf[1], buf[2]];
        let connID : u32 = u32::from_be_bytes(rawConnID);
        println!("Read connID: {:x}", connID);

        if connID == 0 {
            // In this case the client is new
            
        } else {
            // The client already exists and we need to distribute the
            // packet to the correct thread/task ...

        }
    } 

    // I would like to move this into its own thread but 
    // I guess for UDP this wont be possible because we just have a
    // socket but not an individual handle per client or connection?

    // tokio::spawn(async move {
    //     server_task().await;
    // });
    
    // Ok(())
}

async fn server_task() {
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