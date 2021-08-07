use std::net::{UdpSocket, SocketAddr};
use std::{io, fs, time::Duration, convert::TryInto};
use pretty_hex::*;
use sha2::{Sha256, Digest};
use crate::packets::*;

const PACKET_SIZE : usize = 1280;
const DATA_HEADER : usize = 10;
const DATA_SIZE : usize = PACKET_SIZE - DATA_HEADER;


pub fn bind_to_socket(ip : &String, port : &u32, retry : u32) -> io::Result<UdpSocket> {
    let mut addr = String::from(ip);
    addr.push_str(":");
    addr.push_str(&port.to_string());
    debug!("Binding to {}...", addr);
    if retry <= 0 {
        let sock = UdpSocket::bind(addr);
        return sock;
    } else {
        for i in 0..(retry-1) {
            let sock = match UdpSocket::bind(&addr) {
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

pub fn get_next_packet(sock : &UdpSocket, timeout : f64) -> Result<(Vec<u8>, usize, SocketAddr), ()> {
    let mut buf : [u8; PACKET_SIZE] = [0; PACKET_SIZE];
    debug!("Waiting for new incoming packet on {}", sock.local_addr().unwrap());
    if timeout <= 0.0 {
        let mut timeout_set = true;
        match sock.read_timeout() {
            Ok(_) => timeout_set = false,
            Err(_) => {},
        }
        if timeout_set {
            match sock.set_read_timeout(None) {
                Ok(_) => {},
                Err(e) => {
                    warn!("Failed to unset read timeout on socket: {}", e);
                }
            }
        }
    } else {
        let dur = Duration::from_millis((timeout * 1000.0) as u64);
        match sock.set_read_timeout(Some(dur)) {
            Ok(_) => {},
            Err(e) => {
                warn!("Failed to set read timeout on socket: {}", e);
            }
        }
    }
    let (len, addr) = match sock.recv_from(&mut buf) {
        Ok(l) => l,
        Err(e) => {
            error!("Failed to receive on socket {}", sock.local_addr().unwrap());
            // std::process::exit(1);
            return Err(());
        }
    };
    debug!("Received {} bytes from {}", len, addr);
    // debug!("{}", pretty_hex(&buf));

    Ok((buf.to_vec(), len, addr))
}

pub fn get_file(file : &String) -> Result<(Vec<u8>, String), ()> {
    let filename = String::from(file);
    // Removing whitespace and 0 bytes from the transfer
    let filename = filename.trim().trim_matches(char::from(0));
    debug!("Opening file: {}", filename);
    let file = match fs::read(&filename) {
        Ok(f) => f,
        Err(e) => {
            warn!("Failed to read in the file {}", filename);
            warn!("{}", e);
            return Err(());
        },
    };
    Ok((file, String::from(filename)))
}

pub fn compute_hash(buf : &Vec<u8>) -> Result<[u8; 32],()> {
    let mut hasher = Sha256::new();
    hasher.update(buf);
    let hash = hasher.finalize();
    debug!("Generated hash: {}", pretty_hex(&hash));
    let filehash : [u8;32] = match hash.as_slice().try_into() {
        Ok(h) => h,
        Err(e) => {
            warn!("Failed to convert hash: {}", e);
            return Err(());
        }
    };
    Ok(filehash)
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