mod cmdline_handler;
mod packets;

use std::net::UdpSocket;
use std::{thread, time, env};
use crate::cmdline_handler::*;


fn main() {
    // println!("Hello, world!");
    let args: Vec<String> = env::args().collect();
    let opt = parse_cmdline(args).expect("Failed to parse the commandline");

    if opt.server {
        start_server(opt);
    } else {
        start_client(opt);
    }
}

fn start_server(opt : Options) {
    println!("Starting the server...");

    // Listening on any socket...
    let sock = UdpSocket::bind("0.0.0.0:5001").expect("Failed to bind to 0.0.0.0:5001");

    println!("Server is listening on 0.0.0.0:{}", 5001);

    let mut buf = [0; 1500];
    let (num, addr) = sock.recv_from(&mut buf).expect("Failed to receive message");
    println!("Received {} bytes from {}", num, addr);
}

fn start_client(opt: Options) {
    println!("Starting the file transfer...");

}

fn receive_worker() {

}

fn send_worker() {

}