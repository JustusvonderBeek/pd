use std::net::{UdpSocket, SocketAddr, IpAddr, Ipv4Addr};
use std::io;
use crate::packets::*;

pub fn bind_to_socket(ip : &String, port : &u32, retry : u32) -> io::Result<UdpSocket, ()> {
    let mut addr = String::from(ip);
    addr.push_str(":");
    addr.push_str(&port.to_string());
    debug!("Binding to {}...", addr);
    if retry <= 0 {
        let sock = UdpSocket::bind(addr);
        return sock;
    } else {
        for i in 0..(retry-1) {
            let sock = match UdpSocket::bind(addr) {
                Ok(s) => Ok(s),
                Err(e) => {
                    if i + 1 == retry {
                        Err(e)
                    }  else {
                        continue;
                    }
                },
            };
            return sock;
        }
        UdpSocket::bind(addr)
    }
}

fn bind_socket(addr : &String) -> io::Result<UdpSocket> {
}

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