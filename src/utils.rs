use std::net::{UdpSocket, SocketAddr};
use std::{io, fs, time::Duration, convert::TryInto};
use pretty_hex::*;
use sha2::{Sha256, Digest};
use crate::packets::*;

pub const PACKET_SIZE : usize = 1280;
pub const DATA_HEADER : usize = 10;
pub const DATA_SIZE : usize = PACKET_SIZE - DATA_HEADER;

pub fn compute_hostname(ip : &String, port : &u32) -> String {
    let mut addr = String::from(ip.trim().trim_matches(char::from(0)));
    addr.push_str(":");
    addr.push_str(&port.to_string());
    debug!("Hostname: {}", addr);
    addr
}

pub fn bind_to_socket(ip : &String, port : &u32, retry : u32) -> io::Result<UdpSocket> {
    let addr = compute_hostname(ip, port);
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

pub fn get_next_packet(sock : &UdpSocket, timeout_ms : f64) -> Result<(Vec<u8>, usize, SocketAddr), Option<()>> {
    let mut buf : [u8; PACKET_SIZE] = [0; PACKET_SIZE];
    debug!("Waiting for new incoming packet on {}", sock.local_addr().unwrap());
    if timeout_ms <= 0.0 {
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
        let dur = Duration::from_millis((timeout_ms) as u64);
        match sock.set_read_timeout(Some(dur)) {
            Ok(_) => {},
            Err(e) => {
                warn!("Failed to set read timeout on socket: {}", e);
            }
        }
    }
    let (len, addr) = match sock.recv_from(&mut buf) {
        Ok(l) => l,
        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            return Err(None);
        },
        Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
            return Err(None);
        },
        Err(e) => {
            warn!("Failed to receive data: {}", e);
            return Err(Some(()));
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

pub fn compare_hashes(org : &Vec<u8>, new : &Vec<u8>) -> bool {
    if org.len() != new.len() {
        return false;
    }
    org.iter().zip(new.iter()).all(|(a,b)| a == b)
}

pub fn create_next_packet(remain : &usize, window_buffer : &Vec<u8>, seq : &usize) -> io::Result<(Vec<u8>, usize)> {
    // Including space for header
    let mut packet = vec![0; DATA_SIZE];

    // Computing the slice we need to read out of the window buffer
    let start = seq * DATA_SIZE;
    let mut end = start + DATA_SIZE;
    if *remain < DATA_SIZE {
        end = start + remain;
        debug!("Cut the last packet to {} bytes", remain);
    }
    let size = end - start;
    if size > DATA_SIZE {
        error!("During the computation of the packet size something went wrong! Packet size {} is larger than the space available {}", size, DATA_SIZE);
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid size"));
    }
    if end > window_buffer.len() {
        error!("The copying would create an out of bounds error! Computed {} but the buffer is only {} bytes long", end, window_buffer.len());
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid size"));
    }
    packet[0..size].copy_from_slice(&window_buffer[start..end]);
    Ok((packet.to_vec(), size))
}

pub fn send_data(buf : &Vec<u8>, sock : &UdpSocket, addr : &String) {
    match sock.send_to(&buf, &addr) {
        Ok(s) => debug!("Send {} bytes to {}", s, addr),
        Err(e) => error!("Failed to send to {}: {}", addr, e),
    };
}

pub fn send_error(sock : &UdpSocket, connection_id : &u32, addr : &SocketAddr, e : ErrorTypes) {
    let val = match e {
        ErrorTypes::FileUnavailable => 0x01,
        ErrorTypes::ConnectionRefused => 0x02,
        ErrorTypes::FileModified => 0x03,
        ErrorTypes::Abort => 0x04,
    };
    let err = ErrorPacket::serialize(connection_id, &0, &val);
    match sock.send_to(&err, addr) {
        Ok(s) => debug!("Sent {} bytes of error message", s),
        Err(e) => warn!("Failed to send error message: {}", e),
    };
}

/// Sends a Metadata packet to the destination with new block size
pub fn send_metadata(sock : &UdpSocket, connection_id : &u32, addr : &SocketAddr, block_id : &u32, new_block_size : &u16){
    let m_d = MetadataPacket::serialize(connection_id, block_id, new_block_size);
    match sock.send_to(&m_d, addr) {
        Ok(s) => debug!("Sent {} bytes of metadata message", s),
        Err(e) => warn!("Failed to send error message: {}", e),
    };
}