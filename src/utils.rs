use std::{
    io::{self, Write},
    fs::{self, OpenOptions, File},
    time::Duration, 
    convert::TryInto,
    net::{UdpSocket, SocketAddr, SocketAddrV6},
};
use pretty_hex::*;
use sha2::{Sha256, Digest};
use pnet::datalink;
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
    info!("Computing the file hash...");
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
    // Including space for header
    let mut packet = vec![0; size];

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

pub fn write_state(offset : &u64, filename : &String) {
    write_full_state(offset, None, filename);
}

pub fn write_full_state(offset : &u64, hash : Option<&mut Vec<u8>>, filename : &String) {
    let mut file = String::from(filename);
    file.push_str(".info");

    let mut vec : Vec<u8> = Vec::new();
    let state = offset.to_be_bytes();
    vec.append(&mut state.to_vec());
    match hash {
        Some(mut h) => {
            // Got a hash - creating new state file
            let mut output = match OpenOptions::new().write(true).append(false).truncate(true).create(true).open(&file) {
                Ok(f) => f,
                Err(e) => {
                    warn!("Failed to create state file {}: {}", file, e);
                    return;
                }
            };
            vec.append(&mut h);
            match output.write_all(&vec) {
                Ok(_) => {},
                Err(e) => {
                    warn!("Failed to create state file {}, {}", file, e);
                },
            }
        },
        None => {
            // Got no hash - overwriting first 8 bytes with new offset
            let mut output = match OpenOptions::new().write(true).append(false).truncate(false).create(true).open(&file) {
                Ok(f) => f,
                Err(e) => {
                    warn!("Failed to create state file {}: {}", file, e);
                    return;
                }
            };
            match output.write_all(&vec) {
                Ok(_) => {},
                Err(e) => {
                    warn!("Failed to create state file {}, {}", file, e);
                },
            }
        },
    }
}

pub fn read_state(filename : &String) -> io::Result<(u64, Vec<u8>)> {
    let mut info_file = String::from(filename);
    info_file.push_str(".info");

    let mut part_file = String::from(filename);
    part_file.push_str(".part");

    // Opening the partially downloaded file
    let file = match File::open(&part_file) {
        Ok(f) => f,
        Err(_) => {
            info!("No state information found");
            return Err(io::Error::new(io::ErrorKind::NotFound, "Cannot read state"));
        }
    };
    let len = match file.metadata() {
        Ok(m) => m.len(),
        Err(_) => 0,
    };

    // Opening our file information
    let file = match fs::read(&info_file) {
        Ok(f) => f,
        Err(_) => {
            info!("No state information found");
            return Err(io::Error::new(io::ErrorKind::NotFound, "Cannot read state"));
        },
    };
    let (int_bytes, rest) = file.split_at(std::mem::size_of::<u64>());
    let mut offset = u64::from_be_bytes(int_bytes.try_into().unwrap());
    warn!("State Information: File {} .Part {} Hash len {}", len, offset, rest.len());
    if len != offset {
        offset = len;
    }
    Ok((offset, rest.to_vec()))
}

pub fn delete_state(filename : &String) {
    let mut file = String::from(filename);
    file.push_str(".info");
    delete_file(&file);
}

pub fn delete_part(filename : &String) {
    let mut file = String::from(filename);
    file.push_str(".part");
    delete_file(&file);
}

pub fn delete_file(filename : &String) {
    match fs::remove_file(filename) {
        Ok(_) => info!("Deleted file {}", filename),
        Err(e) => warn!("Failed to remove file {}: {}", filename, e),
    }
}

pub fn rename_file(filename : &String) {
    let mut new_file = String::from(filename);
    // new_file.push_str("");

    let mut old_file = String::from(filename);
    old_file.push_str(".part");

    match fs::rename(old_file, new_file) {
        _ => {},
    }
}

pub fn socket_up(sock : &UdpSocket) -> bool {
    let interfaces = datalink::interfaces();
    let local_ip = match sock.local_addr() {
        Ok(i) => i.ip(),
        Err(_) => return false,
    };

    for i in interfaces {
        for ip in &i.ips {
            if local_ip == ip.ip() {
                return i.is_up();
            }
        }
    }
    false
}

pub fn get_ip() -> String {
    let interfaces = datalink::interfaces();
    let interface = interfaces.iter().find(|i| i.is_up() && !i.is_loopback() && !i.ips.is_empty());
    let interface = match interface {
        Some(i) => i,
        None => {
            return String::from("127.0.0.1");
        },
    };

    let mut l_ip = interface.ips[0].ip();
    for ip in &interface.ips {
        if ip.is_ipv4() {
            l_ip = ip.ip();
            break;
        }
    }
    l_ip.to_string()
}