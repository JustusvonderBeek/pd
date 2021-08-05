use std::net::{UdpSocket, SocketAddr};
use crate::packets::*;

pub fn send_data(buf : &Vec<u8>, sock : &UdpSocket, addr : &String) {
    match sock.send_to(&buf, &addr) {
        Ok(s) => debug!("Send {} bytes to {}", s, addr),
        Err(e) => error!("Failed to send to {}: {}", addr, e),
    };
}

pub fn send_error(sock : &UdpSocket, addr : &SocketAddr, e : ErrorTypes) {
    let val = match e {
        ErrorTypes::FileUnavailable => 0x01,
        ErrorTypes::ConnectionRefused => 0x02,
        ErrorTypes::FileModified => 0x03,
        ErrorTypes::Abort => 0x04,
    };
    let err = ErrorPacket::serialize(&0, &0, &val);
    match sock.send_to(&err, addr) {
        Ok(s) => debug!("Sent {} bytes of error message", s),
        Err(e) => warn!("Failed to send error message: {}", e),
    };
}